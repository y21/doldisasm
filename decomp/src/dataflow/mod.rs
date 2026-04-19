use std::ops::{Add, Deref};

use ppc32::Instruction;
use typed_index_collections::TiVec;

use crate::decoder::Address;

pub mod core;
pub mod loops;
pub mod register_state;
pub mod ssa;
pub mod variables;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InstId(pub u32);
impl Into<usize> for InstId {
    fn into(self) -> usize {
        self.0 as usize
    }
}

impl Add<u32> for InstId {
    type Output = Self;

    fn add(self, rhs: u32) -> Self::Output {
        Self(self.0 + rhs)
    }
}

impl From<usize> for InstId {
    fn from(value: usize) -> Self {
        Self(value as u32)
    }
}

pub type Instructions = TiVec<InstId, (Address, Instruction)>;
pub type InstructionsDeref = <Instructions as Deref>::Target;
