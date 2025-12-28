use crate::decoder::{DecodeError, Decoder};
use crate::word::Word;
use paste::paste;

#[derive(Debug, Copy, Clone)]
pub struct Register(pub u8);

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
            source: Register = |word| Register(word.u8::<6, 10>()),
            dest: Register = |word| Register(word.u8::<11, 15>()),
            rot_bits: Register = |word| Register(word.u8::<16, 20>()),
            mask_start: Immediate<u8> = |word| Immediate(word.u8::<21, 25>()),
            mask_end: Immediate<u8> = |word| Immediate(word.u8::<26, 30>()),
            rc: bool = |word| word.bit::<31>() != 0
        }
    },
    Rlwinm {
        op: 0b10101,
        {
            source: Register = |word| Register(word.u8::<6, 10>()),
            dest: Register = |word| Register(word.u8::<11, 15>()),
            rot_bits: Immediate<u8> = |word| Immediate(word.u8::<16, 20>()),
            mask_start: Immediate<u8> = |word| Immediate(word.u8::<21, 25>()),
            mask_end: Immediate<u8> = |word| Immediate(word.u8::<26, 30>()),
            rc: bool = |word| word.bit::<31>() != 0
        }
    },
    Addis {
        op: 0b001111,
        {
            dest: Register = |word| Register(word.u8::<6, 10>()),
            add: Option<Register> = |word| Some(word.u8::<11, 15>()).filter(|&r| r != 0).map(Register),
            imm: Immediate<i16> = |word| Immediate(word.i16::<16, 31>())
        }
    },
    Addi {
        op: 0b001110,
        {
            dest: Register = |word| Register(word.u8::<6, 10>()),
            source: Register = |word| Register(word.u8::<11, 15>()),
            imm: Immediate<i16> = |word| Immediate(word.i16::<16, 31>())
        }
    },
    Ori {
        op: 0b011000,
        {
            source: Register = |word| Register(word.u8::<6, 10>()),
            dest: Register = |word| Register(word.u8::<11, 15>()),
            imm: Immediate<u16> = |word| Immediate(word.u16::<16, 31>())
        }
    },
    Cmpli {
        op: 0b001010,
        {
            source: Register = |word| Register(word.u8::<11, 15>()),
            imm: Immediate<u16> = |word| Immediate(word.u16::<16, 31>()),
            crf: Register = |word| Register(word.u8::<6, 8>()),
            l: bool = |word| word.bit::<10>() != 0
        }
    },
    Cmpi {
        op: 0b001011,
        {
            source: Register = |word| Register(word.u8::<11, 15>()),
            imm: Immediate<u16> = |word| Immediate(word.u16::<16, 31>()),
            crf: Register = |word| Register(word.u8::<6, 8>()),
            l: bool = |word| word.bit::<10>() != 0
        }
    },
    Cmpl {
        op: EXTENDED_OPCODE,
        xform_op: 0b100000,
        {
            source_a: Register = |word| Register(word.u8::<11, 15>()),
            source_b: Register = |word| Register(word.u8::<16, 20>()),
            crf: Register = |word| Register(word.u8::<6, 8>()),
            l: bool = |word| word.bit::<10>() != 0
        }
    },
    Cmp {
        op: EXTENDED_OPCODE,
        xform_op: 0,
        {
            source_a: Register = |word| Register(word.u8::<11, 15>()),
            source_b: Register = |word| Register(word.u8::<16, 20>()),
            crf: Register = |word| Register(word.u8::<6, 8>()),
            l: bool = |word| word.bit::<10>() != 0
        }
    },
    Bc {
        op: 0b010000,
        {
            bo: BranchOptions = BranchOptions::from_word,
            bi: i8 = |word| word.i8::<11, 15>(),
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
            bi: i8 = |word| word.i8::<11, 15>(),
            link: bool = |word| word.bit::<31>() != 0
        }
    },
    Stwu {
        op: 0b100101,
        {
            source: Register = |word| Register(word.u8::<6, 10>()),
            dest: Register = |word| Register(word.u8::<11, 15>()),
            imm: Immediate<i16> = |word| Immediate(word.i16::<16, 31>())
        }
    },
    Stwux {
        op: EXTENDED_OPCODE,
        xform_op: 0b10110111,
        {
            source: Register = |word| Register(word.u8::<6, 10>()),
            dest: Register = |word| Register(word.u8::<11, 15>()),
            index: Register = |word| Register(word.u8::<16, 20>())
        }
    },
    Subf {
        op: EXTENDED_OPCODE,
        xform_op: 0b101000,
        {
            dest: Register = |word| Register(word.u8::<6, 10>()),
            source_b: Register = |word| Register(word.u8::<11, 15>()),
            source_a: Register = |word| Register(word.u8::<16, 20>()),
            oe: bool = |word| word.bit::<21>() != 0,
            rc: bool = |word| word.bit::<31>() != 0
        }
    },
    Mfspr {
        op: EXTENDED_OPCODE,
        xform_op: 0b101010011,
        {
            dest: Register = |word| Register(word.u8::<6, 10>()),
            spr: SpecialPurposeRegister = SpecialPurposeRegister::from_word
        }
    },
    Mtspr {
        op: EXTENDED_OPCODE,
        xform_op: 0b111010011,
        {
            source: Register = |word| Register(word.u8::<6, 10>()),
            spr: SpecialPurposeRegister = SpecialPurposeRegister::from_word
        }
    },
    Mfmsr {
        op: EXTENDED_OPCODE,
        xform_op: 0b1010011,
        {
            dest: Register = |word| Register(word.u8::<6, 10>())
        }
    },
    Mtmsr {
        op: EXTENDED_OPCODE,
        xform_op: 0b10010010,
        {
            source: Register = |word| Register(word.u8::<6, 10>())
        }
    },
    Or {
        op: EXTENDED_OPCODE,
        xform_op: 0b110111100,
        {
            source: Register = |word| Register(word.u8::<6, 10>()),
            dest: Register = |word| Register(word.u8::<11, 15>()),
            or_with: Register = |word| Register(word.u8::<16, 20>()),
            rc: bool = |word| word.bit::<31>() != 0
        }
    },
    And {
        op: EXTENDED_OPCODE,
        xform_op: 0b11100,
        {
            source1: Register = |word| Register(word.u8::<6, 10>()),
            source2: Register = |word| Register(word.u8::<16, 20>()),
            dest: Register = |word| Register(word.u8::<11, 15>())
        }
    },
    Stw {
        op: 0b100100,
        {
            source: Register = |word| Register(word.u8::<6, 10>()),
            dest: Register = |word| Register(word.u8::<11, 15>()),
            imm: Immediate<i16> = |word| Immediate(word.i16::<16, 31>())
        }
    },
    Stmw {
        op: 0b101111,
        {
            source: Register = |word| Register(word.u8::<6, 10>()),
            dest: Register = |word| Register(word.u8::<11, 15>()),
            imm: Immediate<i16> = |word| Immediate(word.i16::<16, 31>())
        }
    },
    Lwz {
        op: 0b100000,
        {
            dest: Register = |word| Register(word.u8::<6, 10>()),
            source: Register = |word| Register(word.u8::<11, 15>()),
            imm: Immediate<i16> = |word| Immediate(word.i16::<16, 31>())
        }
    },
    Lwzu {
        op: 0b100001,
        {
            dest: Register = |word| Register(word.u8::<6, 10>()),
            source: Register = |word| Register(word.u8::<11, 15>()),
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
            source: Register = |word| Register(word.u8::<6, 10>()),
            dest: Register = |word| Register(word.u8::<11, 15>()),
            imm: Immediate<u16> = |word| Immediate(word.u16::<16, 31>())
        }
    },
    Mtfsb1 {
        op: 0b111111,
        xform_op: 0b100110,
        {
            crf: Register = |word| Register(word.u8::<6, 10>()),
            rc: bool = |word| word.bit::<31>() != 0
        }
    },
    Lmw {
        op: 0b101110,
        {
            source: Register = |word| Register(word.u8::<6, 10>()),
            dest: Register = |word| Register(word.u8::<11, 15>()),
            imm: Immediate<i16> = |word| Immediate(word.i16::<16, 31>())
        }
    },
    Mftb {
        op: EXTENDED_OPCODE,
        xform_op: 0b101110011,
        {
            dest: Register = |word| Register(word.u8::<6, 10>()),
            tbr: TimeBaseRegister = TimeBaseRegister::from_word
        }
    },
    Lhz {
        op: 0b101000,
        {
            dest: Register = |word| Register(word.u8::<6, 10>()),
            source: Register = |word| Register(word.u8::<11, 15>()),
            imm: Immediate<i16> = |word| Immediate(word.i16::<16, 31>())
        }
    },
    Lbz {
        op: 0b100010,
        {
            dest: Register = |word| Register(word.u8::<6, 10>()),
            source: Register = |word| Register(word.u8::<11, 15>()),
            imm: Immediate<i16> = |word| Immediate(word.i16::<16, 31>())
        }
    },
    Neg {
        op: EXTENDED_OPCODE,
        xform_op: 0b1101000,
        {
            dest: Register = |word| Register(word.u8::<6, 10>()),
            source: Register = |word| Register(word.u8::<11, 15>()),
            rc: bool = |word| word.bit::<31>() != 0,
            oe: bool = |word| word.bit::<21>() != 0
        }
    },
    Crxor {
        op: 0b010011,
        xform_op: 0b11000001,
        {
            crb_dest: Register = |word| Register(word.u8::<6, 10>()),
            crb_a: Register = |word| Register(word.u8::<11, 15>()),
            crb_b: Register = |word| Register(word.u8::<16, 20>())
        }
    },
    Add {
        op: EXTENDED_OPCODE,
        xform_op: 0b100001010,
        {
            dest: Register = |word| Register(word.u8::<6, 10>()),
            source_a: Register = |word| Register(word.u8::<11, 15>()),
            source_b: Register = |word| Register(word.u8::<16, 20>()),
            oe: bool = |word| word.bit::<21>() != 0,
            rc: bool = |word| word.bit::<31>() != 0
        }
    }
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
}

#[derive(Debug, Copy, Clone)]
pub enum SpecialPurposeRegister {
    Xer,
    Lr,
    Ctr,
    Other(u16),
}

impl SpecialPurposeRegister {
    pub fn from_word(word: Word) -> Self {
        match word.u16::<11, 15>() | (word.u16::<16, 20>() << 5) {
            1 => SpecialPurposeRegister::Xer,
            8 => SpecialPurposeRegister::Lr,
            9 => SpecialPurposeRegister::Ctr,
            other => SpecialPurposeRegister::Other(other),
        }
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
