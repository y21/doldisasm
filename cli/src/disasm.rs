use anyhow::Context;
use arrayvec::ArrayVec;
use bumpalo::Bump;
use dataflow::{Dataflow, Predecessors, SuccessorTarget, Successors};
use dol::Dol;
use ppc32::{
    Instruction,
    instruction::{BranchOptions, Gpr, Immediate, RegisterVisitor, Spr, compute_branch_target},
};
use std::{array, collections::BTreeMap, iter, ops::Deref, panic::Location};
use std::{fmt::Write, marker::PhantomData};
use typed_index_collections::{TiSlice, TiVec};

use crate::{
    args::{AddrRange, DisassemblyLanguage},
    ast::{self, build::AstBuildParams, write::StringWriter},
    decoder::{Address, Decoder},
    flow::{
        Instructions,
        local_generation::{BlockState, LocalGenerationAnalysis},
    },
    value::{Parameter, Value, ValueInner},
};

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

fn disasm_c(decoder: &mut Decoder<'_>) -> anyhow::Result<()> {
    let fn_address = decoder.address().0;
    let insts: Instructions = iter::from_fn(|| decoder.next_instruction_with_offset().transpose())
        .collect::<Result<_, _>>()
        .map_err(|err| anyhow::anyhow!("decoder error: {err:#x?}"))?;

    let analysis = LocalGenerationAnalysis {
        insts: &insts,
        fn_address,
    };
    let mut local_generations = dataflow::run(&analysis);

    let ast = ast::build(AstBuildParams {
        instructions: &insts,
        local_generations: &local_generations,
        analysis: &analysis,
    });

    let mut output = StringWriter::new();
    ast::write::write_ast(&ast, &mut output);
    println!("{}", output.into_string());

    Ok(())
}

/*
// local_generations.for_each_with_input(&analysis, |id, inst, state| {
    //     //
    //     println!("\n{inst:?}");

    //     struct Vis<'a> {
    //         state: &'a BlockState,
    //     }
    //     impl<'a> RegisterVisitor for Vis<'a> {
    //         fn read_crb(&mut self, _crb: ppc32::instruction::Crb) {
    //             todo!()
    //         }
    //         fn read_crf(&mut self, _crf: ppc32::instruction::Crf) {
    //             todo!()
    //         }
    //         fn read_gpr(&mut self, gpr: Gpr) {
    //             print!(
    //                 "{gpr:?}_{}, ",
    //                 self.state.registers.gprs[gpr.0 as usize].generation
    //             );
    //         }
    //         fn read_spr(&mut self, spr: Spr) {
    //             print!("{spr:?}_");
    //             match spr {
    //                 Spr::Xer => todo!(),
    //                 Spr::Lr => print!("{}, ", self.state.registers.sprs.lr.generation),
    //                 Spr::Ctr => todo!(),
    //                 Spr::Msr => todo!(),
    //                 Spr::Pc => todo!(),
    //                 Spr::Other(_) => todo!(),
    //             }
    //         }
    //     }

    //     print!("    ");
    //     inst.visit_registers(Vis { state });

    //     println!();
    // });
*/
