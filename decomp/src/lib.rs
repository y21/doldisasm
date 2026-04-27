use std::collections::{BTreeMap, HashSet};

use ppc32::{
    Decoder, Instruction,
    decoder::DecodeError,
    instruction::{BranchOptions, Gpr, Spr, compute_branch_target},
};
use tracing::Level;

use crate::{
    ast::{build::AstBuildParams, write::WriteContext},
    dataflow::{
        InstructionsDeref,
        core::DataflowArgs,
        loops::find_loops,
        ssa::{LocalGenerationAnalysis, compute_preds_and_succs, def_use_map},
        variables::infer_variables,
    },
};

pub mod ast;
pub mod dataflow;
pub mod ti_utils;
pub mod visit;

pub fn decompile_into_ast_writer(
    instructions: &InstructionsDeref,
    fn_address: u32,
    writer: &mut impl ast::write::Writer,
) -> Result<(), DecodeError> {
    let mut preds = BTreeMap::default();
    let mut succs = BTreeMap::default();

    compute_preds_and_succs(&instructions, fn_address, &mut preds, &mut succs);

    let analysis = LocalGenerationAnalysis {
        insts: &instructions,
        fn_address,
    };
    let local_generations = dataflow::core::run(
        &analysis,
        DataflowArgs {
            preds: &preds,
            succs: &succs,
        },
    );

    let def_use_map = def_use_map(&analysis, &local_generations);

    let variables = infer_variables(&local_generations, &analysis, &def_use_map, &succs);

    let loops = find_loops(&preds, &succs);

    let ast = ast::build(AstBuildParams {
        fn_address,
        instructions: &instructions,
        local_generations: &local_generations,
        analysis: &analysis,
        def_use_map: &def_use_map,
        variables: &variables,
        succs: &succs,
        loops: &loops,
    });

    ast::write::write_ast(
        &ast,
        &WriteContext {
            variables: &variables,
        },
        writer,
    );
    Ok(())
}

/// Heuristic to tell if a branch instruction is a tail call (true), or an intra-function branch (false).
pub fn branch_is_fn_call(buf: &[u8], fn_addr: u32, instr: Instruction, inst_addr: u32) -> bool {
    let Instruction::Branch {
        target,
        mode,
        link: _,
    } = instr
    else {
        unreachable!()
    };
    let target_addr = compute_branch_target(inst_addr, mode, target);
    let _span =
        tracing::span!(Level::DEBUG, "tail call detection", inst_addr, target_addr).entered();

    // If the target address is before `fn_addr` (i.e. target_addr < fn_addr and the sub here overflows),
    // it's 100% not part of the same function, i.e. a fn call.
    let Some(target_off) = target_addr.checked_sub(fn_addr) else {
        return true;
    };

    // We have a bunch of heuristics, each of which with a different level of confidence.
    // We prefer incorrectly predicting as infra-fn calls rather than incorrectly predicting as tail calls,
    // since the latter can cause us to miss large chunks of code whereas the former may just end up inlining some code.
    let mut confidence: i32 = 0;

    // Heuristic 1: the farther the target is, the less likely it is to be an intra-function branch,
    // e.g. it's rather unlikely that a function branches 1000 instructions away within the same function.
    let target_distance = inst_addr.abs_diff(target_addr) / 4;
    let distance_confidence = match target_distance {
        256..512 => Some(5),
        512..1024 => Some(10),
        1024.. => Some(20),
        _ => None,
    };
    if let Some(distance_confidence) = distance_confidence {
        tracing::debug!(target_distance, distance_confidence, "heuristic 1");
        confidence += distance_confidence;
    }

    {
        // Next heuristics revolve around checking what's around the target_addr.
        let Some(data) = buf.get(target_off as usize - 4..target_off as usize + 4) else {
            tracing::debug!("target out of bounds, assuming tail call");
            return true;
        };

        let mut decoder = Decoder::new(data);

        // Heuristic 2: check if the instruction just before the target is an invalid instruction.
        // If not, it's probably data or padding bytes and so the target is a fn (...or it might just be unimplemented)
        if let Err(..) = decoder.decode_instruction() {
            tracing::debug!("heuristic 2");
            confidence += 5;
        }

        // Heuristic 3: the instruction at the target address is something like `stwu r1, -xxx(r1)` or `mflr`.
        if let Ok(target_instr) = decoder.decode_instruction()
            && let is_stack_reserve_instr = matches!(
                target_instr,
                Instruction::Stwu {
                    source: Gpr::STACK_POINTER,
                    dest: Gpr::STACK_POINTER,
                    imm
                } if imm.0 < 0
            )
            && let is_mflr_instr = matches!(
                target_instr,
                Instruction::Mfspr {
                    dest: _,
                    spr: Spr::Lr
                }
            )
            && (is_mflr_instr || is_stack_reserve_instr)
        {
            tracing::debug!(is_stack_reserve_instr, is_mflr_instr, "heuristic 3");
            confidence += 50;
        }
    }

    // Heuristic 4: target_addr is 16-byte aligned. Can very well be coincidence, and not all fns are necessarily aligned like that.
    if target_addr.is_multiple_of(16) {
        tracing::debug!("heuristic 4");
        confidence += 5;
    }

    tracing::debug!(confidence);

    confidence >= 10
}

pub fn detect_fn_boundaries(buf: &[u8], fn_address: u32, start: usize) -> &[u8] {
    #[must_use]
    fn visit_block(buf: &[u8], seen: &mut HashSet<usize>, fn_address: u32, start: usize) -> usize {
        if !seen.insert(start) {
            return start;
        }

        let mut decoder = Decoder::new(&buf[start..]);

        loop {
            let instr_off = start + decoder.offset();
            let inst_addr = fn_address + instr_off as u32;
            let instr = decoder.decode_instruction();

            match instr {
                Ok(instr) => {
                    match instr {
                        Instruction::Bclr {
                            bo: BranchOptions::BranchAlways,
                            bi: _,
                            link: false,
                        } => {
                            return instr_off + 4;
                        }
                        Instruction::Bc {
                            bo: _,
                            bi: _,
                            target,
                            mode,
                            link,
                        } => {
                            let branch_end = if !link
                                // If checked_sub fails, the target is before the start of the function,
                                // i.e. definitely a fn call
                                && let target_addr = compute_branch_target(inst_addr, mode, target)
                                && let Some(target) = target_addr
                                    .checked_sub(fn_address)
                            {
                                Some(visit_block(buf, seen, fn_address, target as usize))
                            } else {
                                None
                            };

                            let cur_end = visit_block(buf, seen, fn_address, instr_off + 4);

                            return match (branch_end, cur_end) {
                                (Some(be), ce) => be.max(ce),
                                (None, ce) => ce,
                            };
                        }
                        Instruction::Branch { target, mode, link } => {
                            if !link
                                // If checked_sub fails, the target is before the start of the function,
                                // i.e. definitely a fn call
                                && let target_addr = compute_branch_target(inst_addr, mode, target)
                                && let Some(target) = target_addr.checked_sub(fn_address)
                                && !branch_is_fn_call(buf, fn_address, instr, inst_addr)
                            {
                                return visit_block(buf, seen, fn_address, target as usize);
                            }
                        }
                        _ => {}
                    }
                }
                Err(err) => {
                    tracing::warn!("error decoding instruction at {inst_addr:#x}: {err}");
                    return instr_off;
                }
            }
        }
    }

    let end = visit_block(buf, &mut HashSet::new(), fn_address, start);
    &buf[start..end]
}
