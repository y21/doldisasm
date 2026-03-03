use anyhow::Context;
use decomp::{
    ast::{
        self,
        build::AstBuildParams,
        write::{StringWriter, WriteContext},
    },
    dataflow::{
        self, Instructions,
        ssa::{LocalGenerationAnalysis, def_use_map},
        variables::infer_variables,
    },
    decoder::{AddrRange, Decoder},
};
use dol::Dol;
use std::iter;

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
    let fn_address = decoder.address().0;
    let insts: Instructions = iter::from_fn(|| decoder.next_instruction_with_offset().transpose())
        .collect::<Result<_, _>>()
        .map_err(|err| anyhow::anyhow!("decoder error: {err:#x?}"))?;

    let analysis = LocalGenerationAnalysis {
        insts: &insts,
        fn_address,
    };
    let local_generations = dataflow::core::run(&analysis);

    let def_use_map = def_use_map(&analysis, &local_generations);

    let variables = infer_variables(&insts, &local_generations, &analysis, &def_use_map);

    let ast = ast::build(AstBuildParams {
        fn_address,
        instructions: &insts,
        local_generations: &local_generations,
        analysis: &analysis,
        def_use_map: &def_use_map,
        variables: &variables,
    });

    let mut output = StringWriter::new();
    ast::write::write_ast(
        &ast,
        &WriteContext {
            variables: &variables,
        },
        &mut output,
    );
    println!("{}", output.into_string());

    Ok(())
}
