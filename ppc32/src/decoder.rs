use crate::{instruction::Instruction, word::Word};

#[derive(Debug)]
pub enum DecodeError {
    UnhandledOpcode { word: Word, offset: usize },
    UnexpectedEof { offset: usize },
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
}
