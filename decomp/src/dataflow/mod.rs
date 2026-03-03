use std::ops::Deref;

use ppc32::Instruction;
use typed_index_collections::{TiSlice, TiVec};

use crate::decoder::Address;

pub mod core;
pub mod register_state;
pub mod ssa;
pub mod variables;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct InstId(pub u32);
impl Into<usize> for InstId {
    fn into(self) -> usize {
        self.0 as usize
    }
}

impl From<usize> for InstId {
    fn from(value: usize) -> Self {
        Self(value as u32)
    }
}

pub type Instructions = TiVec<InstId, (Address, Instruction)>;
pub type InstructionsDeref = <Instructions as Deref>::Target;

pub fn ti_iter<K, V>(ti: &TiSlice<K, V>) -> impl Iterator<Item = (K, &V)>
where
    K: From<usize>,
{
    ti.iter().enumerate().map(|(i, v)| (K::from(i), v))
}
