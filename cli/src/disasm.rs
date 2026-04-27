use anyhow::Context;
use decomp::{
    ast::write::StringWriter,
    dataflow::{Instructions, InstructionsDeref},
    detect_fn_boundaries,
};
use dol::Dol;
use ppc32::{
    Decoder,
    decoder::{AddrRange, AddrRangeEnd},
};

use crate::args::DisassemblyLanguage;

pub fn disasm(dol: &Dol, range: AddrRange, lang: DisassemblyLanguage) -> anyhow::Result<()> {
    let fn_addr = range.0;
    let buffer = dol
        .slice_from_load_addr(fn_addr)
        .context("address is not in any section")?;

    let boundary = match range.1 {
        AddrRangeEnd::Unbounded => detect_fn_boundaries(buffer, fn_addr, 0),
        AddrRangeEnd::Bounded(end_addr) => {
            let end = end_addr - fn_addr;
            &buffer[..end as usize]
        }
    };
    let mut decoder = Decoder::new(boundary);
    let instructions = decoder
        .iter_until_eof(fn_addr)
        .collect::<Result<Instructions, _>>()
        .context("decode error")?;

    match lang {
        DisassemblyLanguage::Asm => disasm_asm(&instructions)?,
        DisassemblyLanguage::C => disasm_c(&instructions, fn_addr)?,
    }

    Ok(())
}

/// Disassemble as assembly code.
fn disasm_asm(instructions: &InstructionsDeref) -> anyhow::Result<()> {
    for (addr, ins) in instructions {
        println!("{addr} {ins:?}")
    }

    Ok(())
}

/// Disassemble as C code.
fn disasm_c(instructions: &InstructionsDeref, fn_addr: u32) -> anyhow::Result<()> {
    let mut output = StringWriter::new();
    decomp::decompile_into_ast_writer(instructions, fn_addr, &mut output)
        .map_err(|err| anyhow::anyhow!("decompilation error: {err:#x?}"))?;
    println!("{}", output.into_string());

    Ok(())
}
