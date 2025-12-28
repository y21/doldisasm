use std::ops::Range;

use anyhow::{Context, ensure};
use dol::Dol;
use ppc32::{
    decoder::Decoder,
    instruction::{BranchOptions, Instruction},
};

use crate::args::{AddrRange, AddrRangeEnd, DisassemblyLanguage};

pub fn disasm(
    dol: &Dol,
    AddrRange(start_addr, end_addr): AddrRange,
    lang: DisassemblyLanguage,
) -> anyhow::Result<()> {
    ensure!(
        matches!(lang, DisassemblyLanguage::Asm),
        "only 'asm' disassembly language is supported currently"
    );

    let section = dol
        .section_of_load_addr(start_addr)
        .context("failed to find section of address")?;

    let file_offset = section.file_offset_of_addr(start_addr);
    let buffer = &dol.as_bytes()[file_offset as usize..];

    let mut decoder = Decoder::new(buffer);

    let mut conditional_ranges: Vec<Range<u32>> = Vec::new();

    loop {
        let offset = decoder.offset_u32();
        let instr_addr = start_addr + offset;

        if let AddrRangeEnd::Bounded(end) = end_addr
            && instr_addr >= end
        {
            break;
        }

        match decoder.decode_instruction() {
            Ok(instruction) => {
                print!("{:#x} {instruction:?}\n", instr_addr);

                if let Instruction::Bclr {
                    bo: BranchOptions::BranchAlways,
                    bi: _,
                    link: false,
                } = instruction
                    && let AddrRangeEnd::Unbounded = end_addr
                {
                    break;
                }
            }
            Err(err) => {
                println!("(stopping due to error: {err:#x?})");
                break;
            }
        }
    }

    Ok(())
}
