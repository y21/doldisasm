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
    decoder::{Address, Decoder},
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
    Ok(())
}
