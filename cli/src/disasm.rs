use std::{array, collections::HashMap, iter, ops::Deref};

use anyhow::Context;
use dol::Dol;
use ppc32::{
    Instruction,
    instruction::{BranchOptions, Gpr, RegisterVisitor, compute_branch_target},
};

use crate::{
    args::{AddrRange, DisassemblyLanguage},
    decoder::Decoder,
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
            Ok(Some((off, ins))) => println!("{off:#x} {ins:?}"),
            Ok(None) => break,
            Err(err) => {
                eprintln!("(stopping due to decoder error: {err:#x?})");
                break;
            }
        }
    }

    Ok(())
}

type InstId = u32;
type Instructions = Vec<(u32, Instruction)>;
type InstructionsDeref = <Instructions as Deref>::Target;

#[derive(Debug, Clone, Copy)]
pub struct RegisterState {
    definitely_initialized: bool,
}

impl RegisterState {
    fn join(self, other: Self) -> Self {
        Self {
            definitely_initialized: self.definitely_initialized && other.definitely_initialized,
        }
    }
}

#[derive(Debug, Clone)]
struct BlockState {
    register_states: [RegisterState; 32],
}

impl BlockState {
    fn empty() -> Self {
        Self {
            register_states: [RegisterState {
                definitely_initialized: false,
            }; 32],
        }
    }

    fn join(self, other: Self) -> Self {
        Self {
            register_states: array::from_fn(|i| {
                self.register_states[i].join(other.register_states[i])
            }),
        }
    }
}

fn disasm_c(decoder: &mut Decoder<'_>) -> anyhow::Result<()> {
    let insts: Instructions = iter::from_fn(|| decoder.next_instruction_with_offset().transpose())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| anyhow::anyhow!("decoder error: {err:#x?}"))?;

    let mut precedessors = HashMap::<InstId, Vec<InstId>>::default();
    let mut successors = HashMap::<InstId, Vec<InstId>>::default();

    compute_preds_and_succs(
        &mut precedessors,
        &mut successors,
        &insts,
        decoder.address(),
    );

    dump_mapping(&insts, &precedessors, "predecessors");
    dump_mapping(&insts, &successors, "successors");

    // ===== Dataflow analysis ======
    let mut queue: Vec<InstId> = vec![0];
    let mut states = HashMap::<InstId, BlockState>::default();

    println!("{:?}", precedessors);
    println!("{:?}", successors);

    while let Some(inst_id) = queue.pop() {
        dbg!(inst_id);
        // Step 1: get the state of all of this instruction's predecessors and join them together.
        let preds = precedessors.get(&inst_id).map(|v| v.iter());
        assert!(
            inst_id == 0 || preds.is_some(),
            "{:x?} did not have a predecessor list recorded",
            insts[inst_id as usize]
        );

        let mut pred_state = preds
            .into_iter()
            .flatten()
            .copied()
            .map(|inst| states[&inst].clone())
            .reduce(BlockState::join)
            .unwrap_or_else(BlockState::empty);

        for (inst_id, &(_, inst)) in insts.iter().enumerate().skip(inst_id as usize) {
            println!(
                "Process instruction {inst:x?} with state {:?}",
                /*pred_state*/ ()
            );

            struct Visitor<'a> {
                state: &'a mut BlockState,
                uninitialized_registers_read: &'a mut Vec<Gpr>,
                initialized_registers: &'a mut Vec<Gpr>,
            }
            impl RegisterVisitor for Visitor<'_> {
                fn read_gpr(&mut self, gpr: Gpr) {
                    if !self.state.register_states[gpr.0 as usize].definitely_initialized {
                        self.uninitialized_registers_read.push(gpr);
                    }
                }
                fn write_gpr(&mut self, gpr: Gpr) {
                    self.initialized_registers.push(gpr);
                }
            }

            let mut uninit_registers_read = Vec::new();
            let mut initialized_registers = Vec::new();
            inst.for_each_register(Visitor {
                state: &mut pred_state,
                uninitialized_registers_read: &mut uninit_registers_read,
                initialized_registers: &mut initialized_registers,
            });

            if !uninit_registers_read.is_empty() {
                println!(
                    "  Warning: instruction reads uninitialized registers: {:?}",
                    uninit_registers_read
                );
            }

            for Gpr(reg) in initialized_registers {
                pred_state.register_states[reg as usize].definitely_initialized = true;
            }

            if let Some(succs) = successors.get(&(inst_id as u32)) {
                queue.extend(succs);
                states.insert(inst_id as u32, pred_state);
                break;
            }
        }
    }

    Ok(())
}

fn dump_mapping(insts: &InstructionsDeref, mapping: &HashMap<InstId, Vec<InstId>>, name: &str) {
    for (&from, to) in mapping {
        for &to in to {
            println!(
                "{name}: {:x?} -> {:x?}",
                insts[from as usize], insts[to as usize]
            );
        }
    }
}

fn compute_preds_and_succs(
    preds: &mut HashMap<InstId, Vec<InstId>>,
    succs: &mut HashMap<InstId, Vec<InstId>>,
    insts: &InstructionsDeref,
    address: AddrRange,
) {
    let mut store_mapping = |from: InstId, to: InstId| {
        preds.entry(to).or_default().push(from);
        succs.entry(from).or_default().push(to);
    };

    for (idx, &(off, inst)) in insts.iter().enumerate() {
        let idx = u32::try_from(idx).unwrap();

        if let Instruction::Bc {
            bo,
            bi: _,
            target,
            mode,
            link: false,
        } = inst
            && bo != BranchOptions::BranchAlways
        {
            if let Some(target) = compute_branch_target(off, mode, target).checked_sub(address.0) {
                // If we have a conditional branch to an address before the function itself (i.e. checked_sub = None due to overflow),
                // then that isn't part of this function and thus not something we need to analyze, hence the checked_sub.
                // The difference is also in bytes, so the instruction difference is that divided by 4.
                store_mapping(idx, target / 4);
            }

            store_mapping(idx, idx + 1);
        }
    }
}
