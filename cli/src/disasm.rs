use anyhow::Context;
use decomp::{
    ast::write::StringWriter,
    decoder::{AddrRange, Decoder},
};
use dol::Dol;

use crate::args::DisassemblyLanguage;

pub fn disasm(dol: &Dol, range: AddrRange, lang: DisassemblyLanguage) -> anyhow::Result<()> {
    let buffer = dol
        .slice_from_load_addr(range.0)
        .context("address is not in any section")?;

    let mut decoder = Decoder::new(buffer, range);

    match lang {
        DisassemblyLanguage::Asm => disasm_asm(&mut decoder)?,
        DisassemblyLanguage::C => disasm_c(&mut decoder)?,
    }

    Ok(())
}

/// Disassemble as assembly code.
fn disasm_asm(decoder: &mut Decoder<'_>) -> anyhow::Result<()> {
    loop {
        match decoder.next_instruction_with_offset() {
            Ok(Some((off, ins))) => println!("{off} {ins:?}"),
            Ok(None) => break,
            Err(err) => {
                eprintln!("(stopping due to decoder error: {err:#x?})");
                break;
            }
        }
    }

    Ok(())
}

/// Disassemble as C code.
fn disasm_c(decoder: &mut Decoder<'_>) -> anyhow::Result<()> {
    let mut output = StringWriter::new();
    decomp::decompile_into_ast_writer(decoder, &mut output)
        .map_err(|err| anyhow::anyhow!("decompilation error: {err:#x?}"))?;
    println!("{}", output.into_string());

    Ok(())
}
