use core::fmt;
use std::fmt::Debug;

use crate::decoder::{DecodeError, Decoder};
use crate::word::Word;
use paste::paste;

/// A general purpose register, numbered through 0 to 31.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Gpr(pub u8);

impl Debug for Gpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "r{}", self.0)
    }
}

impl Gpr {
    pub const ZERO: Self = Self(0);
    pub const RETURN: Self = Self(3);
    pub const STACK_POINTER: Self = Self(1);

    pub fn is_parameter(&self) -> bool {
        matches!(self.0, 3..=10)
    }

    pub fn is_callee_saved(&self) -> bool {
        matches!(self.0, 14..=31)
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum XerRegister {
    So,
    Ov,
    Ca,
}

impl XerRegister {
    pub const ALL: &[Self] = &[Self::So, Self::Ov, Self::Ca];
}

pub type MicroSpr = Spr<XerRegister>;
pub type MacroSpr = Spr<()>;

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// A special purpose register.
pub enum Spr<Xer> {
    Xer(Xer),
    Lr,
    Ctr,
    Msr,
    Pc,
    /// Only usable in supervisor mode.
    Other(u16),
}

impl Debug for MicroSpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Spr::Xer(XerRegister::So) => write!(f, "XER[SO]"),
            Spr::Xer(XerRegister::Ov) => write!(f, "XER[OV]"),
            Spr::Xer(XerRegister::Ca) => write!(f, "XER[CA]"),
            Spr::Lr => write!(f, "LR"),
            Spr::Ctr => write!(f, "CTR"),
            Spr::Msr => write!(f, "MSR"),
            Spr::Pc => write!(f, "PC"),
            Spr::Other(num) => write!(f, "SPR({})", num),
        }
    }
}

impl Debug for MacroSpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Spr::Xer(()) => write!(f, "XER"),
            Spr::Lr => write!(f, "LR"),
            Spr::Ctr => write!(f, "CTR"),
            Spr::Msr => write!(f, "MSR"),
            Spr::Pc => write!(f, "PC"),
            Spr::Other(num) => write!(f, "SPR({})", num),
        }
    }
}

impl MacroSpr {
    pub fn from_word(word: Word) -> Self {
        match word.u16::<11, 15>() | (word.u16::<16, 20>() << 5) {
            1 => Spr::Xer(()),
            8 => Spr::Lr,
            9 => Spr::Ctr,
            other => Spr::Other(other),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Register {
    Gpr(Gpr),
    Cr(Crf, Crb),
    Spr(MicroSpr),
}
impl Debug for Register {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gpr(arg0) => arg0.fmt(f),
            Self::Cr(arg0, arg1) => write!(f, "CR{}.{:?}", arg0.0, arg1),
            Self::Spr(arg0) => arg0.fmt(f),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// A control register field, numbered CRF0 through CRF7.
pub struct Crf(pub u8);

macro_rules! mk_ordinal_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $variant:ident = $value:expr
            ),*
        }
    ) => {
        $(#[$meta])*
        $vis enum $name {
            $(
                $variant = $value
            ),*
        }
        impl $name {
            pub fn from_repr(v: u8) -> Option<Self> {
                match v {
                    $(
                        $value => Some(Self::$variant),
                    )*
                    _ => None,
                }
            }
        }
    };
}

mk_ordinal_enum! {
    #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    /// A control register bit, numbered CRB0 through CRB3 (four bits).
    pub enum Crb {
        Negative = 0,
        Positive = 1,
        Zero = 2,
        Overflow = 3
    }
}

/// Maps a condition register bit index to the corresponding CRF and CRB.
pub fn crb_from_index(index: u8) -> (Crf, Crb) {
    let crf = index / 4;
    let crb = index % 4;
    (Crf(crf), Crb::from_repr(crb).unwrap())
}

#[derive(Debug, Copy, Clone)]
pub struct Immediate<T>(pub T);

#[derive(Debug, Copy, Clone)]
pub enum AddressingMode {
    Absolute,
    Relative,
}

impl AddressingMode {
    pub fn from_absolute_bit(bit: u32) -> Self {
        if bit != 0 {
            AddressingMode::Absolute
        } else {
            AddressingMode::Relative
        }
    }
}

macro_rules! define_instructions {
    ($($name:ident { $(op: $op:expr,)? $(xform_op: $xform_op:expr ,)? { $( $field:ident: $ty:ty = $decode:expr ),* } }),*) => {
        paste! {
            #[derive(Debug, Copy, Clone)]
            pub enum Instruction {
                $(
                    $name {
                        $( $field: $ty ),*
                    },
                )*
            }
            const _: [(); 12] = [(); size_of::<Instruction>()];

            fn __assert_decode_fn<T: FnOnce(Word) -> R, R>(t: T) -> T { t }

            impl Instruction {
                $(
                    pub fn [<parse_ $name:lower>](#[allow(unused)] word: Word) -> Result<Self, DecodeError> {
                        $(
                            // This dummy function call helps type inference by constraining the argument to be `Word`, so we don't need to put
                            // `word: Word` annotations on each closure.
                            let decode = __assert_decode_fn($decode);
                            let $field = decode(word);
                        )*
                        Ok(Instruction::$name { $( $field ),* })
                    }
                )*
            }

            impl Decoder<'_> {
                pub(crate) fn decode_from_word(&mut self, word: Word) -> Result<Instruction, DecodeError> {
                    macro_rules! opt_pattern {
                        ($pat:pat) => { $pat };
                        () => { _ };
                    }

                    match (word.opcode(), word.xform_opcode()) {
                        $(
                            (opt_pattern!($($op)?), opt_pattern!($($xform_op)?)) => Instruction::[<parse_ $name:lower>](word),
                        )*
                        _ => Err(DecodeError::UnhandledOpcode {
                            word,
                            offset: self.offset() - 4,
                        }),
                    }
                }
            }
        }
    };
}

const EXTENDED_OPCODE: u32 = 0b011111;
define_instructions! {
    Branch {
        op: 0b010010,
        {
            target: i32 = |word| word.i32::<6, 29>() << 2,
            mode: AddressingMode = |word| AddressingMode::from_absolute_bit(word.bit::<30>()),
            link: bool = |word| word.bit::<31>() != 0
        }
    },
    Rlwnm {
        op: 0b010111,
        {
            source: Gpr = |word| Gpr(word.u8::<6, 10>()),
            dest: Gpr = |word| Gpr(word.u8::<11, 15>()),
            rot_bits: Gpr = |word| Gpr(word.u8::<16, 20>()),
            mask_start: Immediate<u8> = |word| Immediate(word.u8::<21, 25>()),
            mask_end: Immediate<u8> = |word| Immediate(word.u8::<26, 30>()),
            rc: bool = |word| word.bit::<31>() != 0
        }
    },
    Rlwinm {
        op: 0b10101,
        {
            source: Gpr = |word| Gpr(word.u8::<6, 10>()),
            dest: Gpr = |word| Gpr(word.u8::<11, 15>()),
            rot_bits: Immediate<u8> = |word| Immediate(word.u8::<16, 20>()),
            mask_start: Immediate<u8> = |word| Immediate(word.u8::<21, 25>()),
            mask_end: Immediate<u8> = |word| Immediate(word.u8::<26, 30>()),
            rc: bool = |word| word.bit::<31>() != 0
        }
    },
    Addis {
        op: 0b001111,
        {
            dest: Gpr = |word| Gpr(word.u8::<6, 10>()),
            add: Option<Gpr> = |word| Some(word.u8::<11, 15>()).filter(|&r| r != 0).map(Gpr),
            imm: Immediate<i16> = |word| Immediate(word.i16::<16, 31>())
        }
    },
    Addi {
        op: 0b001110,
        {
            dest: Gpr = |word| Gpr(word.u8::<6, 10>()),
            source: Gpr = |word| Gpr(word.u8::<11, 15>()),
            imm: Immediate<i16> = |word| Immediate(word.i16::<16, 31>())
        }
    },
    Ori {
        op: 0b011000,
        {
            source: Gpr = |word| Gpr(word.u8::<6, 10>()),
            dest: Gpr = |word| Gpr(word.u8::<11, 15>()),
            imm: Immediate<u16> = |word| Immediate(word.u16::<16, 31>())
        }
    },
    Cmpli {
        op: 0b001010,
        {
            source: Gpr = |word| Gpr(word.u8::<11, 15>()),
            imm: Immediate<u16> = |word| Immediate(word.u16::<16, 31>()),
            crf: Crf = |word| Crf(word.u8::<6, 8>()),
            l: bool = |word| word.bit::<10>() != 0
        }
    },
    Cmpi {
        op: 0b001011,
        {
            source: Gpr = |word| Gpr(word.u8::<11, 15>()),
            imm: Immediate<u16> = |word| Immediate(word.u16::<16, 31>()),
            crf: Crf = |word| Crf(word.u8::<6, 8>())
        }
    },
    Cmpl {
        op: EXTENDED_OPCODE,
        xform_op: 0b100000,
        {
            source_a: Gpr = |word| Gpr(word.u8::<11, 15>()),
            source_b: Gpr = |word| Gpr(word.u8::<16, 20>()),
            crf: Crf = |word| Crf(word.u8::<6, 8>()),
            l: bool = |word| word.bit::<10>() != 0
        }
    },
    Cmp {
        op: EXTENDED_OPCODE,
        xform_op: 0,
        {
            source_a: Gpr = |word| Gpr(word.u8::<11, 15>()),
            source_b: Gpr = |word| Gpr(word.u8::<16, 20>()),
            crf: Crf = |word| Crf(word.u8::<6, 8>()),
            l: bool = |word| word.bit::<10>() != 0
        }
    },
    Bc {
        op: 0b010000,
        {
            bo: BranchOptions = BranchOptions::from_word,
            bi: u8 = |word| word.u8::<11, 15>(),
            target: i32 = |word| word.i32::<16, 29>() << 2,
            mode: AddressingMode = |word| AddressingMode::from_absolute_bit(word.bit::<30>()),
            link: bool = |word| word.bit::<31>() != 0
        }
    },
    Bclr {
        op: 0b010011,
        xform_op: 0b010000,
        {
            bo: BranchOptions = BranchOptions::from_word,
            bi: u8 = |word| word.u8::<11, 15>(),
            link: bool = |word| word.bit::<31>() != 0
        }
    },
    Stwu {
        op: 0b100101,
        {
            source: Gpr = |word| Gpr(word.u8::<6, 10>()),
            dest: Gpr = |word| Gpr(word.u8::<11, 15>()),
            imm: Immediate<i16> = |word| Immediate(word.i16::<16, 31>())
        }
    },
    Stwux {
        op: EXTENDED_OPCODE,
        xform_op: 0b10110111,
        {
            source: Gpr = |word| Gpr(word.u8::<6, 10>()),
            dest: Gpr = |word| Gpr(word.u8::<11, 15>()),
            index: Gpr = |word| Gpr(word.u8::<16, 20>())
        }
    },
    Subf {
        op: EXTENDED_OPCODE,
        xform_op: 0b101000,
        {
            dest: Gpr = |word| Gpr(word.u8::<6, 10>()),
            source_b: Gpr = |word| Gpr(word.u8::<11, 15>()),
            source_a: Gpr = |word| Gpr(word.u8::<16, 20>()),
            oe: bool = |word| word.bit::<21>() != 0,
            rc: bool = |word| word.bit::<31>() != 0
        }
    },
    Subfic {
        op: 0b001000,
        {
            dest: Gpr = |word| Gpr(word.u8::<6, 10>()),
            source: Gpr = |word| Gpr(word.u8::<11, 15>()),
            simm: i16 = |word| word.i16::<16, 31>()
        }
    },
    Subfe {
        op: EXTENDED_OPCODE,
        xform_op: 0b0010001000,
        {
            dest: Gpr = |word| Gpr(word.u8::<6, 10>()),
            source_a: Gpr = |word| Gpr(word.u8::<11, 15>()),
            source_b: Gpr = |word| Gpr(word.u8::<16, 20>()),
            oe: bool = |word| word.bit::<21>() != 0,
            rc: bool = |word| word.bit::<31>() != 0
        }
    },
    Mfspr {
        op: EXTENDED_OPCODE,
        xform_op: 0b101010011,
        {
            dest: Gpr = |word| Gpr(word.u8::<6, 10>()),
            spr: MacroSpr = Spr::from_word
        }
    },
    Mtspr {
        op: EXTENDED_OPCODE,
        xform_op: 0b111010011,
        {
            source: Gpr = |word| Gpr(word.u8::<6, 10>()),
            spr: MacroSpr = Spr::from_word
        }
    },
    Mfmsr {
        op: EXTENDED_OPCODE,
        xform_op: 0b1010011,
        {
            dest: Gpr = |word| Gpr(word.u8::<6, 10>())
        }
    },
    Mtmsr {
        op: EXTENDED_OPCODE,
        xform_op: 0b10010010,
        {
            source: Gpr = |word| Gpr(word.u8::<6, 10>())
        }
    },
    Or {
        op: EXTENDED_OPCODE,
        xform_op: 0b110111100,
        {
            source: Gpr = |word| Gpr(word.u8::<6, 10>()),
            dest: Gpr = |word| Gpr(word.u8::<11, 15>()),
            or_with: Gpr = |word| Gpr(word.u8::<16, 20>()),
            rc: bool = |word| word.bit::<31>() != 0
        }
    },
    And {
        op: EXTENDED_OPCODE,
        xform_op: 0b11100,
        {
            source1: Gpr = |word| Gpr(word.u8::<6, 10>()),
            source2: Gpr = |word| Gpr(word.u8::<16, 20>()),
            dest: Gpr = |word| Gpr(word.u8::<11, 15>())
        }
    },
    Andi {
        op: 0b011100,
        {
            source: Gpr = |word| Gpr(word.u8::<6, 10>()),
            dest: Gpr = |word| Gpr(word.u8::<11, 15>()),
            simm: i16 = |word| word.i16::<16, 31>()
        }
    },
    Stw {
        op: 0b100100,
        {
            source: Gpr = |word| Gpr(word.u8::<6, 10>()),
            dest: Gpr = |word| Gpr(word.u8::<11, 15>()),
            imm: Immediate<i16> = |word| Immediate(word.i16::<16, 31>())
        }
    },
    Stmw {
        op: 0b101111,
        {
            source: Gpr = |word| Gpr(word.u8::<6, 10>()),
            dest: Gpr = |word| Gpr(word.u8::<11, 15>()),
            imm: Immediate<i16> = |word| Immediate(word.i16::<16, 31>())
        }
    },
    Lwz {
        op: 0b100000,
        {
            dest: Gpr = |word| Gpr(word.u8::<6, 10>()),
            source: Gpr = |word| Gpr(word.u8::<11, 15>()),
            imm: Immediate<i16> = |word| Immediate(word.i16::<16, 31>())
        }
    },
    Lwzu {
        op: 0b100001,
        {
            dest: Gpr = |word| Gpr(word.u8::<6, 10>()),
            source: Gpr = |word| Gpr(word.u8::<11, 15>()),
            imm: Immediate<i16> = |word| Immediate(word.i16::<16, 31>())
        }
    },
    Isync {
        op: 0b10011,
        xform_op: 0b10010110,
        {

        }
    },
    Hwsync {
        op: EXTENDED_OPCODE,
        xform_op: 0b1001010110,
        {

        }
    },
    Oris {
        op: 0b11001,
        {
            source: Gpr = |word| Gpr(word.u8::<6, 10>()),
            dest: Gpr = |word| Gpr(word.u8::<11, 15>()),
            imm: Immediate<u16> = |word| Immediate(word.u16::<16, 31>())
        }
    },
    Mtfsb1 {
        op: 0b111111,
        xform_op: 0b100110,
        {
            crf: Crf = |word| Crf(word.u8::<6, 10>()),
            rc: bool = |word| word.bit::<31>() != 0
        }
    },
    Lmw {
        op: 0b101110,
        {
            source: Gpr = |word| Gpr(word.u8::<6, 10>()),
            dest: Gpr = |word| Gpr(word.u8::<11, 15>()),
            imm: Immediate<i16> = |word| Immediate(word.i16::<16, 31>())
        }
    },
    Mftb {
        op: EXTENDED_OPCODE,
        xform_op: 0b101110011,
        {
            dest: Gpr = |word| Gpr(word.u8::<6, 10>()),
            tbr: TimeBaseRegister = TimeBaseRegister::from_word
        }
    },
    Lhz {
        op: 0b101000,
        {
            dest: Gpr = |word| Gpr(word.u8::<6, 10>()),
            source: Gpr = |word| Gpr(word.u8::<11, 15>()),
            imm: Immediate<i16> = |word| Immediate(word.i16::<16, 31>())
        }
    },
    Lbz {
        op: 0b100010,
        {
            dest: Gpr = |word| Gpr(word.u8::<6, 10>()),
            source: Gpr = |word| Gpr(word.u8::<11, 15>()),
            imm: Immediate<i16> = |word| Immediate(word.i16::<16, 31>())
        }
    },
    Neg {
        op: EXTENDED_OPCODE,
        xform_op: 0b1101000,
        {
            dest: Gpr = |word| Gpr(word.u8::<6, 10>()),
            source: Gpr = |word| Gpr(word.u8::<11, 15>()),
            rc: bool = |word| word.bit::<31>() != 0,
            oe: bool = |word| word.bit::<21>() != 0
        }
    },
    Crxor {
        op: 0b010011,
        xform_op: 0b11000001,
        {
            crb_dest: u8 = |word| word.u8::<6, 10>(),
            crb_a: u8 = |word| word.u8::<11, 15>(),
            crb_b: u8 = |word| word.u8::<16, 20>()
        }
    },
    Add {
        op: EXTENDED_OPCODE,
        xform_op: 0b100001010,
        {
            dest: Gpr = |word| Gpr(word.u8::<6, 10>()),
            source_a: Gpr = |word| Gpr(word.u8::<11, 15>()),
            source_b: Gpr = |word| Gpr(word.u8::<16, 20>()),
            oe: bool = |word| word.bit::<21>() != 0,
            rc: bool = |word| word.bit::<31>() != 0
        }
    },
    AddicRc {
        op: 0b001101,
        {
            dest: Gpr = |word| Gpr(word.u8::<6, 10>()),
            source: Gpr = |word| Gpr(word.u8::<11, 15>()),
            simm: i16 = |word| word.i16::<16, 31>()
        }
    }
}

pub trait RegisterVisitor {
    fn read_gpr(&mut self, _gpr: Gpr) {}
    fn write_gpr(&mut self, _gpr: Gpr) {}
    fn read_spr(&mut self, _spr: MicroSpr) {}
    fn write_spr(&mut self, _spr: MicroSpr) {}
    fn read_crf(&mut self, _crf: Crf) {}
    fn write_crf(&mut self, _crf: Crf) {}
    fn read_crb(&mut self, _crf: Crf, _crb: Crb) {}
    fn write_crb(&mut self, _crf: Crf, _crb: Crb) {}
    /// Called when the instruction's effect "happens".
    /// This is used to distinguish the "before" and "after" state of an instruction.
    /// Consider `addi r3, r3, 1`:
    /// The function call order for visiting registers will be:
    /// - `read_gpr(r3)`
    /// - `effect()`
    /// - `write_gpr(r3)`
    fn effect(&mut self) {}
}

impl Instruction {
    pub fn branch_target(&self, instr_addr: u32) -> Option<u32> {
        match self {
            Instruction::Branch { target, mode, .. } => {
                Some(compute_branch_target(instr_addr, *mode, *target))
            }
            Instruction::Bc { target, mode, .. } => {
                Some(compute_branch_target(instr_addr, *mode, *target))
            }
            _ => None,
        }
    }

    #[rustfmt::skip]
    pub fn visit_registers(&self, mut visitor: impl RegisterVisitor) {
        match *self {
            Instruction::Branch { target: _, mode: _, link } => {
                visitor.effect();
                if link {
                    visitor.write_gpr(Gpr::RETURN);
                }
            },
            Instruction::Rlwnm { source, dest, rot_bits, mask_start: _, mask_end: _, rc } => {
                visitor.read_gpr(source);
                visitor.read_gpr(rot_bits);
                visitor.effect();
                visitor.write_gpr(dest);
                if rc {
                    visitor.write_crf(Crf(0));
                }
            },
            Instruction::Rlwinm { source, dest, rot_bits: _, mask_start: _, mask_end: _, rc } => {
                visitor.read_gpr(source);
                visitor.effect();
                visitor.write_gpr(dest);
                if rc {
                    visitor.write_crf(Crf(0));
                }
            },
            Instruction::Addis { dest, add, imm: _ } => {
                if let Some(gpr) = add {
                    visitor.read_gpr(gpr);
                }
                visitor.effect();
                visitor.write_gpr(dest);
            },
            Instruction::Addi { dest, source, imm: _ } => {
                visitor.read_gpr(source);
                visitor.effect();
                visitor.write_gpr(dest);
            },
            Instruction::AddicRc { dest, source, simm: _ } => {
                visitor.read_gpr(source);
                visitor.effect();
                visitor.write_gpr(dest);
                visitor.write_spr(Spr::Xer(XerRegister::Ca));
                visitor.write_crf(Crf(0));
            }
            Instruction::Ori { source, dest, imm: _ } => {
                visitor.read_gpr(source);
                visitor.effect();
                visitor.write_gpr(dest);
            },
            Instruction::Cmpli { source, imm: _, crf, l: _ } => {
                visitor.read_gpr(source);
                visitor.effect();
                visitor.write_crf(crf);
            },
            Instruction::Cmpi { source, imm: _, crf,  } => {
                visitor.read_gpr(source);
                visitor.effect();
                visitor.write_crf(crf);
            },
            Instruction::Cmpl { source_a, source_b, crf, l: _ } => {
                visitor.read_gpr(source_a);
                visitor.read_gpr(source_b);
                visitor.effect();
                visitor.write_crf(crf);
            },
            Instruction::Cmp { source_a, source_b, crf, l: _ } => {
                visitor.read_gpr(source_a);
                visitor.read_gpr(source_b);
                visitor.effect();
                visitor.write_crf(crf);
            },
            Instruction::Bc { bo, bi, target: _, mode: _, link: _ } => {
                if !matches!(bo, BranchOptions::BranchAlways) {
                    let (crf, crb) = crb_from_index(bi);
                    visitor.read_crb(crf, crb);
                }
                visitor.effect();
            },
            Instruction::Bclr { bo, bi, link: _ } => {
                if !matches!(bo, BranchOptions::BranchAlways) {
                    let (crf, crb) = crb_from_index(bi);
                    visitor.read_crb(crf, crb);
                }
                visitor.effect();
            },
            Instruction::Stwu { source, dest, imm: _ } => {
                visitor.read_gpr(source);
                visitor.read_gpr(dest);
                visitor.effect();
                visitor.write_gpr(dest);
            },
            Instruction::Stwux { source, dest, index } => {
                visitor.read_gpr(source);
                visitor.read_gpr(dest);
                visitor.read_gpr(index);
                visitor.effect();
            },
            Instruction::Subf { dest, source_b, source_a, oe: _, rc } => {
                visitor.read_gpr(source_a);
                visitor.read_gpr(source_b);
                visitor.effect();
                visitor.write_gpr(dest);
                if rc {
                    visitor.write_crf(Crf(0));
                }
            },
            Instruction::Subfic { dest, source, simm: _ } => {
                visitor.read_gpr(source);
                visitor.effect();
                visitor.write_gpr(dest);
                visitor.write_spr(Spr::Xer(XerRegister::Ca));
            }
            Instruction::Subfe { dest, source_a, source_b, oe, rc } => {
                visitor.read_gpr(source_a);
                visitor.read_gpr(source_b);
                visitor.read_spr(Spr::Xer(XerRegister::Ca));
                visitor.effect();
                visitor.write_gpr(dest);
                if rc {
                    visitor.write_crf(Crf(0));
                }
                visitor.write_spr(Spr::Xer(XerRegister::Ca));
                if oe {
                    visitor.write_spr(Spr::Xer(XerRegister::So));
                    visitor.write_spr(Spr::Xer(XerRegister::Ov));
                }
            }
            Instruction::Mfspr { dest, spr } => {
                match spr {
                    Spr::Xer(()) => {
                        visitor.read_spr(Spr::Xer(XerRegister::So));
                        visitor.read_spr(Spr::Xer(XerRegister::Ca));
                        visitor.read_spr(Spr::Xer(XerRegister::Ov));
                    },
                    Spr::Lr => visitor.read_spr(Spr::Lr),
                    Spr::Ctr => visitor.read_spr(Spr::Ctr),
                    Spr::Msr => visitor.read_spr(Spr::Msr),
                    Spr::Pc => visitor.read_spr(Spr::Pc),
                    Spr::Other(other) => visitor.read_spr(Spr::Other(other)),
                }
                visitor.effect();
                visitor.write_gpr(dest);
            },
            Instruction::Mtspr { source, spr } => {
                visitor.read_gpr(source);
                visitor.effect();
                match spr {
                    Spr::Xer(()) => {
                        visitor.write_spr(Spr::Xer(XerRegister::So));
                        visitor.write_spr(Spr::Xer(XerRegister::Ca));
                        visitor.write_spr(Spr::Xer(XerRegister::Ov));
                    },
                    Spr::Lr => visitor.write_spr(Spr::Lr),
                    Spr::Ctr => visitor.write_spr(Spr::Ctr),
                    Spr::Msr => visitor.write_spr(Spr::Msr),
                    Spr::Pc => visitor.write_spr(Spr::Pc),
                    Spr::Other(other) => visitor.write_spr(Spr::Other(other)),
                }
            },
            Instruction::Mfmsr { dest } => {
                visitor.read_spr(Spr::Msr);
                visitor.effect();
                visitor.write_gpr(dest);
            },
            Instruction::Mtmsr { source } => {
                visitor.read_gpr(source);
                visitor.effect();
                visitor.write_spr(Spr::Msr);
            },
            Instruction::Or { source, dest, or_with, rc } => {
                visitor.read_gpr(source);
                visitor.read_gpr(or_with);
                visitor.effect();
                visitor.write_gpr(dest);
                if rc {
                    visitor.write_crf(Crf(0));
                }
            },
            Instruction::And { source1, source2, dest } => {
                visitor.read_gpr(source1);
                visitor.read_gpr(source2);
                visitor.effect();
                visitor.write_gpr(dest);
            },
            Instruction::Andi { source, dest, simm: _ } => {
                visitor.read_gpr(source);
                visitor.effect();
                visitor.write_gpr(dest);
            }
            Instruction::Stw { source, dest, imm: _ } => {
                visitor.read_gpr(source);
                visitor.read_gpr(dest);
                visitor.effect();
            },
            Instruction::Stmw { source, dest, imm: _ } => {
                visitor.read_gpr(source);
                visitor.read_gpr(dest);
                visitor.effect();
            },
            Instruction::Lwz { dest, source, imm: _ } => {
                visitor.read_gpr(source);
                visitor.effect();
                visitor.write_gpr(dest);
            },
            Instruction::Lwzu { dest, source, imm: _ } => {
                visitor.read_gpr(source);
                visitor.effect();
                visitor.write_gpr(dest);
            },
            Instruction::Isync {} => {
                visitor.effect();
            },
            Instruction::Hwsync {} => {
                visitor.effect();
            },
            Instruction::Oris { source, dest, imm: _ } => {
                visitor.read_gpr(source);
                visitor.effect();
                visitor.write_gpr(dest);
            },
            Instruction::Mtfsb1 { crf, rc } => {
                visitor.effect();
                visitor.write_crf(crf);
                if rc {
                    visitor.write_crf(Crf(1));
                }
            },
            Instruction::Lmw { source, dest, imm: _ } => {
                visitor.read_gpr(source);
                visitor.effect();
                visitor.write_gpr(dest);
            },
            Instruction::Mftb { dest, tbr: _ } => {
                visitor.effect();
                visitor.write_gpr(dest);
            },
            Instruction::Lhz { dest, source, imm: _ } => {
                visitor.read_gpr(source);
                visitor.effect();
                visitor.write_gpr(dest);
            },
            Instruction::Lbz { dest, source, imm: _ } => {
                visitor.read_gpr(source);
                visitor.effect();
                visitor.write_gpr(dest);
            },
            Instruction::Neg { dest, source, rc, oe } => {
                visitor.read_gpr(source);
                visitor.effect();
                visitor.write_gpr(dest);
                if oe {
                    visitor.write_spr(Spr::Xer(XerRegister::So));
                    visitor.write_spr(Spr::Xer(XerRegister::Ov));
                }
                if rc {
                    visitor.write_crf(Crf(0));
                }
            },
            Instruction::Crxor { crb_dest, crb_a, crb_b } => {
                let (crf_dest, crb_dest) = crb_from_index(crb_dest);
                let (crf_a, crb_a) = crb_from_index(crb_a);
                let (crf_b, crb_b) = crb_from_index(crb_b);

                visitor.read_crb(crf_a, crb_a);
                visitor.read_crb(crf_b, crb_b);
                visitor.effect();
                visitor.write_crb(crf_dest, crb_dest);
            },
            Instruction::Add { dest, source_a, source_b, oe: _, rc } => {
                visitor.read_gpr(source_a);
                visitor.read_gpr(source_b);
                visitor.effect();
                visitor.write_gpr(dest);
                if rc {
                    visitor.write_crf(Crf(0));
                }
            },
        }
    }

    pub fn for_each_read_gpr(&self, mut f: impl FnMut(Gpr)) {
        struct Visitor<'a, F: FnMut(Gpr)> {
            f: &'a mut F,
        }

        impl<'a, F: FnMut(Gpr)> RegisterVisitor for Visitor<'a, F> {
            fn read_gpr(&mut self, gpr: Gpr) {
                (self.f)(gpr);
            }
        }

        let visitor = Visitor { f: &mut f };
        self.visit_registers(visitor);
    }

    pub fn disasm_fmt(&self, instr_addr: u32) -> impl fmt::Display + use<'_> {
        fmt::from_fn(move |fmt| match self {
            Instruction::Branch { target, mode, link } => {
                let mnemonic = if *link { "bl" } else { "b" };
                let addr = compute_branch_target(instr_addr, *mode, *target);
                match mode {
                    AddressingMode::Absolute => write!(fmt, "{}a {:#x}", mnemonic, addr),
                    AddressingMode::Relative => write!(fmt, "{} {:#x}", mnemonic, addr),
                }
            }
            Instruction::Rlwnm {
                source,
                dest,
                rot_bits,
                mask_start,
                mask_end,
                rc,
            } => {
                let dot = if *rc { "." } else { "" };
                write!(
                    fmt,
                    "rlwnm{} {:?},{:?},{:?},{},{}",
                    dot, dest, source, rot_bits, mask_start.0, mask_end.0
                )
            }
            Instruction::Rlwinm {
                source,
                dest,
                rot_bits,
                mask_start,
                mask_end,
                rc,
            } => {
                let dot = if *rc { "." } else { "" };
                write!(
                    fmt,
                    "rlwinm{} {:?},{:?},{},{},{}",
                    dot, dest, source, rot_bits.0, mask_start.0, mask_end.0
                )
            }
            Instruction::Addis { dest, add, imm } => match add {
                Some(gpr) => write!(fmt, "addis {:?},{:?},{}", dest, gpr, imm.0),
                None => write!(fmt, "lis {:?},{}", dest, imm.0),
            },
            Instruction::Addi { dest, source, imm } => {
                if source.0 == 0 {
                    write!(fmt, "li {:?},{}", dest, imm.0)
                } else {
                    write!(fmt, "addi {:?},{:?},{}", dest, source, imm.0)
                }
            }
            Instruction::Ori { source, dest, imm } => {
                if source.0 == 0 && dest.0 == 0 && imm.0 == 0 {
                    write!(fmt, "nop")
                } else {
                    write!(fmt, "ori {:?},{:?},{}", dest, source, imm.0)
                }
            }
            Instruction::Cmpli {
                source,
                imm,
                crf,
                l,
            } => {
                if crf.0 == 0 && !l {
                    write!(fmt, "cmplwi {:?},{}", source, imm.0)
                } else {
                    write!(fmt, "cmplwi cr{},{:?},{}", crf.0, source, imm.0)
                }
            }
            Instruction::Cmpi { source, imm, crf } => {
                if crf.0 == 0 {
                    write!(fmt, "cmpwi {:?},{}", source, imm.0 as i16)
                } else {
                    write!(fmt, "cmpwi cr{},{:?},{}", crf.0, source, imm.0 as i16)
                }
            }
            Instruction::Cmpl {
                source_a,
                source_b,
                crf,
                l: _,
            } => {
                if crf.0 == 0 {
                    write!(fmt, "cmplw {:?},{:?}", source_a, source_b)
                } else {
                    write!(fmt, "cmplw cr{},{:?},{:?}", crf.0, source_a, source_b)
                }
            }
            Instruction::Cmp {
                source_a,
                source_b,
                crf,
                l: _,
            } => {
                if crf.0 == 0 {
                    write!(fmt, "cmpw {:?},{:?}", source_a, source_b)
                } else {
                    write!(fmt, "cmpw cr{},{:?},{:?}", crf.0, source_a, source_b)
                }
            }
            Instruction::Bc {
                bo,
                bi,
                target,
                mode,
                link,
            } => {
                let (crf, crb) = crb_from_index(*bi);
                let addr = compute_branch_target(instr_addr, *mode, *target);
                let link_suffix = if *link { "l" } else { "" };
                let abs_suffix = if matches!(mode, AddressingMode::Absolute) {
                    "a"
                } else {
                    ""
                };
                let crb_name = match crb {
                    Crb::Negative => "lt",
                    Crb::Positive => "gt",
                    Crb::Zero => "eq",
                    Crb::Overflow => "so",
                };
                match bo {
                    BranchOptions::BranchAlways => {
                        write!(fmt, "b{}{} {:#x}", link_suffix, abs_suffix, addr)
                    }
                    BranchOptions::BranchIfTrue | BranchOptions::DecCTRBranchIfTrue => {
                        if crf.0 == 0 {
                            write!(
                                fmt,
                                "b{}{}{} {:#x}",
                                crb_name, link_suffix, abs_suffix, addr
                            )
                        } else {
                            write!(
                                fmt,
                                "b{}{}{} cr{},{:#x}",
                                crb_name, link_suffix, abs_suffix, crf.0, addr
                            )
                        }
                    }
                    BranchOptions::BranchIfFalse | BranchOptions::DecCTRBranchIfFalse => {
                        let neg_crb_name = match crb {
                            Crb::Negative => "ge",
                            Crb::Positive => "le",
                            Crb::Zero => "ne",
                            Crb::Overflow => "ns",
                        };
                        if crf.0 == 0 {
                            write!(
                                fmt,
                                "b{}{}{} {:#x}",
                                neg_crb_name, link_suffix, abs_suffix, addr
                            )
                        } else {
                            write!(
                                fmt,
                                "b{}{}{} cr{},{:#x}",
                                neg_crb_name, link_suffix, abs_suffix, crf.0, addr
                            )
                        }
                    }
                    BranchOptions::DecCTRBranchIfNotZero => {
                        write!(fmt, "bdnz{}{} {:#x}", link_suffix, abs_suffix, addr)
                    }
                    BranchOptions::DecCTRBranchIfZero => {
                        write!(fmt, "bdz{}{} {:#x}", link_suffix, abs_suffix, addr)
                    }
                }
            }
            Instruction::Bclr { bo, bi, link } => {
                let (crf, crb) = crb_from_index(*bi);
                let link_suffix = if *link { "l" } else { "" };
                let crb_name = match crb {
                    Crb::Negative => "lt",
                    Crb::Positive => "gt",
                    Crb::Zero => "eq",
                    Crb::Overflow => "so",
                };
                match bo {
                    BranchOptions::BranchAlways => write!(fmt, "blr{}", link_suffix),
                    BranchOptions::BranchIfTrue | BranchOptions::DecCTRBranchIfTrue => {
                        if crf.0 == 0 {
                            write!(fmt, "b{}lr{}", crb_name, link_suffix)
                        } else {
                            write!(fmt, "b{}lr{} cr{}", crb_name, link_suffix, crf.0)
                        }
                    }
                    BranchOptions::BranchIfFalse | BranchOptions::DecCTRBranchIfFalse => {
                        let neg_crb_name = match crb {
                            Crb::Negative => "ge",
                            Crb::Positive => "le",
                            Crb::Zero => "ne",
                            Crb::Overflow => "ns",
                        };
                        if crf.0 == 0 {
                            write!(fmt, "b{}lr{}", neg_crb_name, link_suffix)
                        } else {
                            write!(fmt, "b{}lr{} cr{}", neg_crb_name, link_suffix, crf.0)
                        }
                    }
                    BranchOptions::DecCTRBranchIfNotZero => write!(fmt, "bdnzlr{}", link_suffix),
                    BranchOptions::DecCTRBranchIfZero => write!(fmt, "bdzlr{}", link_suffix),
                }
            }
            Instruction::Stwu { source, dest, imm } => {
                write!(fmt, "stwu {:?},{}({:?})", source, imm.0, dest)
            }
            Instruction::Stwux {
                source,
                dest,
                index,
            } => {
                write!(fmt, "stwux {:?},{:?},{:?}", source, dest, index)
            }
            Instruction::Subf {
                dest,
                source_b,
                source_a,
                oe,
                rc,
            } => {
                let dot = if *rc { "." } else { "" };
                let o = if *oe { "o" } else { "" };
                write!(
                    fmt,
                    "subf{}{} {:?},{:?},{:?}",
                    o, dot, dest, source_b, source_a
                )
            }
            Instruction::Subfic { dest, source, simm } => {
                write!(fmt, "subfic {:?},{:?},{}", dest, source, simm)
            }
            Instruction::Subfe {
                dest,
                source_a,
                source_b,
                oe,
                rc,
            } => {
                let dot = if *rc { "." } else { "" };
                let o = if *oe { "o" } else { "" };
                write!(
                    fmt,
                    "subfe{}{} {:?},{:?},{:?}",
                    o, dot, dest, source_a, source_b
                )
            }
            Instruction::Mfspr { dest, spr } => match spr {
                Spr::Lr => write!(fmt, "mflr {:?}", dest),
                Spr::Ctr => write!(fmt, "mfctr {:?}", dest),
                _ => write!(fmt, "mfspr {:?},{:?}", dest, spr),
            },
            Instruction::Mtspr { source, spr } => match spr {
                Spr::Lr => write!(fmt, "mtlr {:?}", source),
                Spr::Ctr => write!(fmt, "mtctr {:?}", source),
                _ => write!(fmt, "mtspr {:?},{:?}", spr, source),
            },
            Instruction::Mfmsr { dest } => {
                write!(fmt, "mfmsr {:?}", dest)
            }
            Instruction::Mtmsr { source } => {
                write!(fmt, "mtmsr {:?}", source)
            }
            Instruction::Or {
                source,
                dest,
                or_with,
                rc,
            } => {
                let dot = if *rc { "." } else { "" };
                if source == or_with {
                    write!(fmt, "mr{} {:?},{:?}", dot, dest, source)
                } else {
                    write!(fmt, "or{} {:?},{:?},{:?}", dot, dest, source, or_with)
                }
            }
            Instruction::And {
                source1,
                source2,
                dest,
            } => {
                write!(fmt, "and {:?},{:?},{:?}", dest, source1, source2)
            }
            Instruction::Andi { source, dest, simm } => {
                write!(fmt, "andi. {:?},{:?},{}", dest, source, simm)
            }
            Instruction::Stw { source, dest, imm } => {
                write!(fmt, "stw {:?},{}({:?})", source, imm.0, dest)
            }
            Instruction::Stmw { source, dest, imm } => {
                write!(fmt, "stmw {:?},{}({:?})", source, imm.0, dest)
            }
            Instruction::Lwz { dest, source, imm } => {
                write!(fmt, "lwz {:?},{}({:?})", dest, imm.0, source)
            }
            Instruction::Lwzu { dest, source, imm } => {
                write!(fmt, "lwzu {:?},{}({:?})", dest, imm.0, source)
            }
            Instruction::Isync {} => {
                write!(fmt, "isync")
            }
            Instruction::Hwsync {} => {
                write!(fmt, "sync")
            }
            Instruction::Oris { source, dest, imm } => {
                write!(fmt, "oris {:?},{:?},{}", dest, source, imm.0)
            }
            Instruction::Mtfsb1 { crf, rc } => {
                let dot = if *rc { "." } else { "" };
                write!(fmt, "mtfsb1{} {}", dot, crf.0)
            }
            Instruction::Lmw { source, dest, imm } => {
                write!(fmt, "lmw {:?},{}({:?})", source, imm.0, dest)
            }
            Instruction::Mftb { dest, tbr } => match tbr {
                TimeBaseRegister::Tbl => write!(fmt, "mftb {:?}", dest),
                TimeBaseRegister::Tbu => write!(fmt, "mftbu {:?}", dest),
            },
            Instruction::Lhz { dest, source, imm } => {
                write!(fmt, "lhz {:?},{}({:?})", dest, imm.0, source)
            }
            Instruction::Lbz { dest, source, imm } => {
                write!(fmt, "lbz {:?},{}({:?})", dest, imm.0, source)
            }
            Instruction::Neg {
                dest,
                source,
                rc,
                oe,
            } => {
                let dot = if *rc { "." } else { "" };
                let o = if *oe { "o" } else { "" };
                write!(fmt, "neg{}{} {:?},{:?}", o, dot, dest, source)
            }
            Instruction::Crxor {
                crb_dest,
                crb_a,
                crb_b,
            } => {
                if crb_dest == crb_a && crb_a == crb_b {
                    write!(fmt, "crclr {}", crb_dest)
                } else {
                    write!(fmt, "crxor {},{},{}", crb_dest, crb_a, crb_b)
                }
            }
            Instruction::Add {
                dest,
                source_a,
                source_b,
                oe,
                rc,
            } => {
                let dot = if *rc { "." } else { "" };
                let o = if *oe { "o" } else { "" };
                write!(
                    fmt,
                    "add{}{} {:?},{:?},{:?}",
                    o, dot, dest, source_a, source_b
                )
            }
            Instruction::AddicRc { dest, source, simm } => {
                write!(fmt, "addic. {:?},{:?},{}", dest, source, simm)
            }
        })
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum BranchOptions {
    DecCTRBranchIfFalse,
    BranchIfFalse,
    DecCTRBranchIfTrue,
    BranchIfTrue,
    DecCTRBranchIfNotZero,
    DecCTRBranchIfZero,
    BranchAlways,
}

impl BranchOptions {
    pub fn from_word(word: Word) -> Self {
        let mask = word.u8::<6, 10>();

        if let 0b00010 | 0 = mask & 0b11110 {
            BranchOptions::DecCTRBranchIfFalse
        } else if mask & 0b11100 == 0b00100 {
            BranchOptions::BranchIfFalse
        } else if let 0b01000 | 0b01010 = mask & 0b11110 {
            BranchOptions::DecCTRBranchIfTrue
        } else if mask & 0b11100 == 0b01100 {
            BranchOptions::BranchIfTrue
        } else if mask & 0b10110 == 0b10000 {
            BranchOptions::DecCTRBranchIfNotZero
        } else if mask & 0b10110 == 0b10010 {
            BranchOptions::DecCTRBranchIfZero
        } else {
            assert!((mask & 0b10100) == 0b10100, "invalid BO operand");
            BranchOptions::BranchAlways
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum TimeBaseRegister {
    /// Upper Time Base
    Tbu,
    /// Lower Time Base
    Tbl,
}

impl TimeBaseRegister {
    pub fn from_word(word: Word) -> Self {
        match word.u16::<11, 15>() | (word.u16::<16, 20>() << 5) {
            268 => TimeBaseRegister::Tbu,
            269 => TimeBaseRegister::Tbl,
            other => panic!("invalid TBR register code: {} (word: {word:x?})", other),
        }
    }
}

pub fn compute_branch_target(base: u32, mode: AddressingMode, target: i32) -> u32 {
    match mode {
        AddressingMode::Absolute => target as u32,
        AddressingMode::Relative => base.checked_add_signed(target).unwrap(),
    }
}
