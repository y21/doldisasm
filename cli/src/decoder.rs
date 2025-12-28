use std::ops::Range;

use ppc32::{
    Instruction,
    decoder::DecodeError,
    instruction::{BranchOptions, compute_branch_target},
};

use crate::args::{AddrRange, AddrRangeEnd};

/// A wrapper around the ppc32 decoder that knows when to stop based on the provided address range or guesses based on heuristics.
pub struct Decoder<'a> {
    decoder: ppc32::Decoder<'a>,
    range: AddrRange,
    conditional_ranges: Vec<Range<u32>>,
    reached_end: bool,
}

impl<'a> Decoder<'a> {
    pub fn new(buffer: &'a [u8], range: AddrRange) -> Self {
        Self {
            decoder: ppc32::Decoder::new(buffer),
            range,
            conditional_ranges: Vec::new(),
            reached_end: false,
        }
    }

    pub fn next_instruction_with_offset(
        &mut self,
    ) -> Result<Option<(u32, Instruction)>, DecodeError> {
        let offset = self.decoder.offset_u32();
        let instr_addr = self.range.0 + offset;

        if let AddrRangeEnd::Bounded(end) = self.range.1
            && instr_addr >= end
        {
            return Ok(None);
        }

        if self.reached_end {
            return Ok(None);
        }

        let instruction = self.decoder.decode_instruction()?;

        if let Instruction::Bc {
            bo,
            bi: _,
            target,
            mode,
            link: false,
        } = instruction
            && bo != BranchOptions::BranchAlways
        {
            // Record conditional branch
            let target = compute_branch_target(instr_addr, mode, target);
            self.conditional_ranges.push(instr_addr..target);
        }

        if let Instruction::Bclr {
            bo: BranchOptions::BranchAlways,
            bi: _,
            link: false,
        }
        | Instruction::Branch {
            target: _,
            mode: _,
            link: false,
        } = instruction
            && let AddrRangeEnd::Unbounded = self.range.1
            && self
                .conditional_ranges
                .iter()
                .all(|range| !range.contains(&instr_addr))
        {
            // Unconditional return, likely end of the function
            self.reached_end = true;
        }

        Ok(Some((instr_addr, instruction)))
    }
}
