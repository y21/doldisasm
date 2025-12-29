use std::{array, iter, ops::Deref};

use anyhow::Context;
use arrayvec::ArrayVec;
use dataflow::{Dataflow, PredecessorsSuccessors};
use dol::Dol;
use ppc32::{
    Instruction,
    instruction::{BranchOptions, Gpr, RegisterVisitor, compute_branch_target},
};
use typed_index_collections::{TiSlice, TiVec};

use crate::{
    args::{AddrRange, DisassemblyLanguage},
    decoder::{Address, Decoder},
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

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct InstId(u32);
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

type Instructions = TiVec<InstId, (Address, Instruction)>;
type InstructionsDeref = <Instructions as Deref>::Target;

fn ti_iter<K, V>(ti: &TiSlice<K, V>) -> impl Iterator<Item = (K, &V)>
where
    K: From<usize>,
{
    ti.iter().enumerate().map(|(i, v)| (K::from(i), v))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegisterState {
    definitely_initialized: bool,
    value_read: bool,
}

impl RegisterState {
    fn join(self, other: Self) -> Self {
        Self {
            definitely_initialized: self.definitely_initialized && other.definitely_initialized,
            value_read: self.value_read || other.value_read,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct BlockState {
    register_states: [RegisterState; 32],
}

impl Default for BlockState {
    fn default() -> Self {
        Self {
            register_states: [RegisterState {
                definitely_initialized: false,
                value_read: false,
            }; 32],
        }
    }
}

impl BlockState {
    fn join(&self, other: &Self) -> Self {
        Self {
            register_states: array::from_fn(|i| {
                self.register_states[i].join(other.register_states[i])
            }),
        }
    }
}

struct Analysis<'a> {
    insts: &'a InstructionsDeref,
    fn_address: u32,
}

impl Dataflow for Analysis<'_> {
    type Idx = InstId;
    type BlockState = BlockState;
    type BlockItem = Instruction;

    fn compute_preds_and_succs(
        &self,
        preds: &mut PredecessorsSuccessors<Self>,
        succs: &mut PredecessorsSuccessors<Self>,
    ) {
        let mut store_mapping = |from: InstId, to: InstId| {
            preds.entry(to).or_default().push(from);
            succs.entry(from).or_default().push(to);
        };

        for (idx, &(off, inst)) in ti_iter(&self.insts) {
            if let Instruction::Bc {
                bo,
                bi: _,
                target,
                mode,
                link: false,
            } = inst
                && bo != BranchOptions::BranchAlways
            {
                if let Some(target) =
                    compute_branch_target(off.0, mode, target).checked_sub(self.fn_address)
                {
                    // If we have a conditional branch to an address before the function itself (i.e. checked_sub = None due to overflow),
                    // then that isn't part of this function and thus not something we need to analyze, hence the checked_sub.
                    // The difference is also in bytes, so the instruction difference is that divided by 4.
                    store_mapping(idx, InstId(target / 4));
                }

                store_mapping(idx, InstId(idx.0 + 1));
            }
        }
    }

    fn initial_idx() -> Self::Idx {
        InstId(0)
    }

    fn join_states(a: &Self::BlockState, b: &Self::BlockState) -> Self::BlockState {
        a.join(b)
    }

    fn iter(&self) -> impl Iterator<Item = (Self::Idx, Self::BlockItem)> {
        ti_iter(&self.insts).map(|(i, &(_, inst))| (i, inst))
    }

    fn iter_block(
        &self,
        InstId(idx): Self::Idx,
    ) -> impl Iterator<Item = (Self::Idx, Self::BlockItem)> {
        self.iter().skip(idx as usize)
    }

    fn apply_effect(&self, state: &mut Self::BlockState, data: &Self::BlockItem) {
        // println!("Process {data:?}");

        struct Visitor<'a> {
            // state: &'a mut BlockState,
            read_registers: &'a mut Vec<Gpr>,
            initialized_registers: &'a mut Vec<Gpr>,
        }

        impl RegisterVisitor for Visitor<'_> {
            fn read_gpr(&mut self, gpr: Gpr) {
                self.read_registers.push(gpr);
            }
            fn write_gpr(&mut self, gpr: Gpr) {
                self.initialized_registers.push(gpr);
            }
        }

        let mut read_registers = Vec::new();
        let mut initialized_registers = Vec::new();
        data.visit_registers(Visitor {
            // state,
            read_registers: &mut read_registers,
            initialized_registers: &mut initialized_registers,
        });

        for Gpr(reg) in initialized_registers {
            state.register_states[reg as usize].definitely_initialized = true;
        }
    }
}

fn disasm_c(decoder: &mut Decoder<'_>) -> anyhow::Result<()> {
    let insts: Instructions = iter::from_fn(|| decoder.next_instruction_with_offset().transpose())
        .collect::<Result<_, _>>()
        .map_err(|err| anyhow::anyhow!("decoder error: {err:#x?}"))?;

    let analysis = Analysis {
        insts: &insts,
        fn_address: decoder.address().0,
    };
    let results = dataflow::run(&analysis);

    let mut parameter_gprs = ArrayVec::new();

    // Iterate over the dataflow results to find uses of r3-r10 before they are initialized, which means they are definitely treated
    // as function parameters.
    let final_state = results.for_each_with_input(&analysis, |_, inst, state| {
        struct Visitor<'a> {
            state: &'a BlockState,
            parameter_gprs: &'a mut ArrayVec<Gpr, 8>,
        }
        impl RegisterVisitor for Visitor<'_> {
            fn read_gpr(&mut self, gpr: Gpr) {
                let reg_state = &self.state.register_states[gpr.0 as usize];
                if !reg_state.definitely_initialized
                    && gpr.is_parameter()
                    && !self.parameter_gprs.contains(&gpr)
                {
                    self.parameter_gprs.push(gpr);
                }
            }
            fn write_gpr(&mut self, _gpr: Gpr) {}
        }

        let vis = Visitor {
            state,
            parameter_gprs: &mut parameter_gprs,
        };
        inst.visit_registers(vis);
    });
    // Finally, check if r3 has been initialized without being read: this means that there's a return value.
    let ret_reg_state = final_state.register_states[3];
    let has_return = ret_reg_state.definitely_initialized && !ret_reg_state.value_read;

    dbg!(&parameter_gprs, has_return);

    Ok(())
}
