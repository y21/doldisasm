//! Reference used: https://www.nxp.com/docs/en/reference-manual/MPC82XINSET.pdf

use paste::paste;

#[derive(Debug, Copy, Clone)]
pub struct Register(u8);

#[derive(Debug, Copy, Clone)]
pub struct Immediate<T>(T);

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
    (
        $($name:ident { $(op: $op:expr,)? $(xform_op: $xform_op:expr ,)? { $( $field:ident: $ty:ty = $decode:expr ),* } }),*
    ) => {
        paste! {
            #[derive(Debug)]
            pub enum Instruction {
                $(
                    $name {
                        $( $field: $ty ),*
                    },
                )*
            }
            fn __assert_decode_fn<T: FnOnce(Word) -> R, R>(t: T) -> T { t }

            impl Instruction {
                $(
                    pub fn [<parse_ $name:lower>](#[allow(unused)] word: Word) -> Result<Self, DecodeError> {
                        $(
                            // Guide type inference to avoid having to put `: Word` on each closure.
                            let decode = __assert_decode_fn($decode);
                            let $field = decode(word);
                        )*
                        Ok(Instruction::$name { $( $field ),*  })
                    }
                )*
            }

            impl Decoder<'_> {
                fn decode_from_word(&mut self, word: Word) -> Result<Instruction, DecodeError> {
                    match word.opcode() {
                        EXTENDED_OPCODE => {
                            // extended opcode
                            match word.xform_opcode() {
                                $(
                                    $( $xform_op => Instruction::[<parse_ $name:lower>](word), )?
                                )*
                                _ => todo!(),
                            }
                        }
                        $(
                            $( $op => Instruction::[<parse_ $name:lower>](word), )?
                        )*
                        _ => Err(DecodeError::UnhandledOpcode {
                            word,
                            offset: self.offset - 4,
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
    Rlwinm {
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
    BranchConditional {
        op: 0b010000,
        {
            bo: BranchOptions = |word| {
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
            },
            bi: i8 = |word| word.i8::<11, 15>(),
            target: i32 = |word| word.i32::<16, 29>() << 2,
            mode: AddressingMode = |word| AddressingMode::from_absolute_bit(word.bit::<30>()),
            link: bool = |word| word.bit::<31>() != 0
        }
    },
    Subf {
        xform_op: 0b101000,
        {
            dest: Register = |word| Register(word.u8::<6, 10>()),
            source_b: Register = |word| Register(word.u8::<11, 15>()),
            source_a: Register = |word| Register(word.u8::<16, 20>()),
            oe: bool = |word| word.bit::<21>() != 0,
            rc: bool = |word| word.bit::<31>() != 0
        }
    }
}

#[derive(Debug)]
pub enum BranchOptions {
    DecCTRBranchIfFalse,
    BranchIfFalse,
    DecCTRBranchIfTrue,
    BranchIfTrue,
    DecCTRBranchIfNotZero,
    DecCTRBranchIfZero,
    BranchAlways,
}

#[derive(Debug)]
pub enum DecodeError {
    UnhandledOpcode { word: Word, offset: usize },
    UnexpectedEof { offset: usize },
}

#[derive(Debug, Copy, Clone)]
pub struct Word(u32);

impl Word {
    fn opcode(self) -> u32 {
        self.0 >> 26
    }

    fn xform_opcode(self) -> u32 {
        // Bits 21-30
        self.u32::<21, 30>()
    }

    /// Extracts a u32 bit range in big endian (reversed) from this word.
    fn u32<const FROM: u32, const TO: u32>(self) -> u32 {
        const { assert!(TO >= FROM && TO - FROM < 32) };

        let mask = const { (!0u32) >> (FROM + (31 - TO)) << (31 - TO) };
        (self.0 & mask) >> (31 - TO)
    }

    fn i32<const FROM: u32, const TO: u32>(self) -> i32 {
        self.u32::<FROM, TO>() as i32
    }

    fn u16<const FROM: u32, const TO: u32>(self) -> u16 {
        const { assert!(TO >= FROM && TO - FROM < 16) };
        self.u32::<FROM, TO>() as u16
    }

    fn i16<const FROM: u32, const TO: u32>(self) -> i16 {
        self.u16::<FROM, TO>() as i16
    }

    fn u8<const FROM: u32, const TO: u32>(self) -> u8 {
        const { assert!(TO >= FROM && TO - FROM < 8) };
        self.u32::<FROM, TO>() as u8
    }

    fn i8<const FROM: u32, const TO: u32>(self) -> i8 {
        self.u8::<FROM, TO>() as i8
    }

    fn bit<const BIT: u32>(self) -> u32 {
        self.0 & (1 << (31 - BIT))
    }
}

pub struct Decoder<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> Decoder<'a> {
    pub fn new(input: &'a [u8]) -> Self {
        Self { input, offset: 0 }
    }

    /// Decodes the next word.
    fn word(&mut self) -> Option<Word> {
        self.input.split_at_checked(4).map(|(bytes, rest)| {
            self.input = rest;
            self.offset += 4;
            Word(u32::from_be_bytes(bytes.try_into().unwrap()))
        })
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn decode_instruction(&mut self) -> Result<Instruction, DecodeError> {
        let Some(word) = self.word() else {
            return Err(DecodeError::UnexpectedEof {
                offset: self.offset,
            });
        };

        self.decode_from_word(word)
    }
}

#[test]
fn test_powerpc() {
    let ins = powerpc::Ins::new(0x4082003c, powerpc::Extensions::gekko_broadway());
    dbg!(ins.simplified().to_string(), ins.basic().to_string());
}
