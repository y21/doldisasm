use std::{error::Error, fmt::Display, iter};

use crate::{instruction::Instruction, word::Word};

#[derive(Debug, Copy, Clone)]
pub enum AddrRangeEnd {
    Unbounded,
    Bounded(u32),
}

#[derive(Debug, Copy, Clone)]
pub struct Address(pub u32);

#[derive(Debug, Copy, Clone)]
pub struct AddrRange(pub u32, pub AddrRangeEnd);

impl Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:08x}", self.0)
    }
}

#[derive(Debug)]
pub enum DecodeError {
    UnhandledOpcode { word: Word, offset: usize },
    UnexpectedEof { offset: usize },
}

impl Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::UnhandledOpcode { word, offset } => {
                write!(f, "unhandled opcode {word:x?} at +{offset:x?}")
            }
            DecodeError::UnexpectedEof { offset } => write!(f, "unexpected eof at +{offset:x?}"),
        }
    }
}

impl Error for DecodeError {}

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

    pub fn offset_u32(&self) -> u32 {
        self.offset as u32
    }

    pub fn decode_instruction(&mut self) -> Result<Instruction, DecodeError> {
        let Some(word) = self.word() else {
            return Err(DecodeError::UnexpectedEof {
                offset: self.offset,
            });
        };

        self.decode_from_word(word)
    }

    /// Returns an iterator over instructions until the end of the input is reached.
    /// `DecodeError::UnexpectedEof` is never returned by this iterator.
    pub fn iter_until_eof(
        &mut self,
        fn_addr: u32,
    ) -> impl Iterator<Item = Result<(Address, Instruction), DecodeError>> {
        iter::from_fn(move || {
            let offset = self.offset_u32();
            match self.decode_instruction() {
                Ok(instr) => Some(Ok((Address(fn_addr + offset), instr))),
                Err(DecodeError::UnexpectedEof { .. }) => None,
                Err(err) => Some(Err(err)),
            }
        })
    }
}
