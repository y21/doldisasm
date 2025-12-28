use anyhow::Context;
use dol::Dol;
use ppc32::decoder::Decoder;

use crate::args::DisassemblyLanguage;

pub fn disasm(dol: &Dol, addr: u32, lang: DisassemblyLanguage) -> anyhow::Result<()> {
    let section = dol
        .section_of_load_addr(addr)
        .context("failed to find section of address")?;

    let file_offset = section.file_offset_of_addr(addr);
    let buffer = &dol.as_bytes()[file_offset as usize..];

    let mut decoder = Decoder::new(buffer);

    loop {
        let offset = decoder.offset();
        match decoder.decode_instruction() {
            Ok(instruction) => {
                print!("{:#x} {instruction:?}\n", addr + offset as u32);
            }
            Err(err) => {
                println!("(stopping due to error: {err:#x?})");
                break;
            }
        }
    }

    Ok(())
}
