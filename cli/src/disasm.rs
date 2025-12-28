use anyhow::{Context, ensure};
use dol::Dol;

use crate::{
    args::{AddrRange, DisassemblyLanguage},
    decoder::Decoder,
};

pub fn disasm(dol: &Dol, range: AddrRange, lang: DisassemblyLanguage) -> anyhow::Result<()> {
    ensure!(
        matches!(lang, DisassemblyLanguage::Asm),
        "only 'asm' disassembly language is supported currently"
    );

    let buffer = dol
        .slice_from_load_addr(range.0)
        .context("address is not in any section")?;

    let mut decoder = Decoder::new(buffer, range);

    loop {
        match decoder.next_instruction_with_offset() {
            Ok(Some((off, ins))) => println!("{off:#x} {ins:?}"),
            Ok(None) => break,
            Err(err) => eprintln!("(stopping due to decoder error: {err:#x?})"),
        }
    }

    Ok(())
}
