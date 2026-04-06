use std::{collections::HashSet, convert::Infallible, iter, ops::ControlFlow};

use ppc32::{Instruction, instruction::Register};

use crate::{
    dataflow::{
        InstId,
        core::{Results, Successors, for_each_transitive_successor},
        ssa::{BlockState, Generation, LocalGenerationAnalysis},
    },
    ti_utils::ti_iter,
};

pub struct VisitorStaticData<'a, 'b> {
    pub analysis: &'a LocalGenerationAnalysis<'b>,
    pub results: &'a Results<LocalGenerationAnalysis<'b>>,
    pub succs: &'a Successors<LocalGenerationAnalysis<'b>>,
}

pub struct VisitorCx<'a, 'b> {
    pub data: &'a VisitorStaticData<'a, 'b>,
    pub id: InstId,
    pub parent: Option<&'a VisitorCx<'a, 'b>>,
}

impl<'a, 'b> VisitorCx<'a, 'b> {
    pub fn analysis(&self) -> &'a LocalGenerationAnalysis<'b> {
        self.data.analysis
    }
    pub fn results(&self) -> &'a Results<LocalGenerationAnalysis<'b>> {
        self.data.results
    }
    pub fn succs(&self) -> &'a Successors<LocalGenerationAnalysis<'b>> {
        self.data.succs
    }
}

pub trait SuccessorsVisitor {
    fn visit_instruction(
        &mut self,
        cx: &mut VisitorCx<'_, '_>,
        instruction: Instruction,
        idx: InstId,
        absolute_idx: InstId,
        end_idx: Option<InstId>,
        inst_addr: u32,
        state: &mut BlockState,
    ) -> ControlFlow<()>;
}

#[derive(Debug)]
pub struct PhiLocal {
    pub register: Register,
    pub next_gen: Generation,
    pub true_gen: Generation,
    pub false_gen: Generation,
}

pub struct JoinResult {
    pub true_res: VisitPathResult,
    pub false_res: VisitPathResult,
    pub phi_locals: Vec<PhiLocal>,
    pub common_merge_inst: Option<InstId>,
}

pub fn visit_and_join_paths<V: SuccessorsVisitor>(
    visitor: &mut V,
    cx: &mut VisitorCx<'_, '_>,
    prev_state: &mut BlockState,
    true_idx: InstId,
    false_idx: InstId,
) -> JoinResult {
    // TODO: maybe compute this one for each entry in successors and then just use the map here?
    let mut true_transitive_successors = HashSet::new();
    for_each_transitive_successor(cx.succs(), true_idx, &mut |inst| {
        true_transitive_successors.insert(inst);
        ControlFlow::<Infallible>::Continue(())
    });

    // The "common merge instruction" (i.e. the instruction that they both "meet" at) is where the paths will stop.
    let common_merge_inst = for_each_transitive_successor(cx.succs(), false_idx, &mut |inst| {
        if true_transitive_successors.contains(&inst) {
            ControlFlow::Break(inst)
        } else {
            ControlFlow::Continue(())
        }
    })
    .break_value();

    let true_res = visit_path(visitor, cx, Some(prev_state), true_idx, common_merge_inst);
    let false_res = visit_path(visitor, cx, Some(prev_state), false_idx, common_merge_inst);

    let mut phi_locals = Vec::new();

    if let Some(common_merge_inst) = common_merge_inst
        && let VisitPathResult::Value { state: true_state } = &true_res
        && let VisitPathResult::Value { state: false_state } = &false_res
    {
        let next_state = cx.results().get(common_merge_inst).unwrap();

        next_state
            .registers
            .register_iter()
            .zip(iter::zip(
                true_state.registers.register_iter(),
                false_state.registers.register_iter(),
            ))
            .for_each(
                |((register, next_state), ((_, true_state), (_, false_state)))| {
                    if let Some(_) = next_state.phi_origins {
                        phi_locals.push(PhiLocal {
                            register,
                            next_gen: next_state.generation,
                            true_gen: true_state.generation,
                            false_gen: false_state.generation,
                        });
                    }
                },
            );
    }

    JoinResult {
        true_res,
        false_res,
        phi_locals,
        common_merge_inst,
    }
}

#[must_use]
#[derive(Debug)]
pub enum VisitPathResult {
    Diverging,
    Value { state: BlockState },
}

pub struct VisitStack<'a> {
    pub idx: InstId,
    pub parent: Option<&'a VisitStack<'a>>,
}

/// Visits the path starting from `start_idx` up until `end_idx` (if provided)
pub fn visit_path<V: SuccessorsVisitor>(
    visitor: &mut V,
    cx: &mut VisitorCx<'_, '_>,
    prev_state: Option<&mut BlockState>,
    start_idx: InstId,
    end_idx: Option<InstId>,
) -> VisitPathResult {
    fn diverging_check(cx: &VisitorCx<'_, '_>, idx: InstId) -> bool {
        if let Some(parent) = cx.parent {
            cx.id == idx || diverging_check(parent, idx)
        } else {
            false
        }
    }

    if diverging_check(cx, start_idx) {
        return VisitPathResult::Diverging;
    }

    let cx = &mut VisitorCx {
        data: cx.data,
        id: start_idx,
        parent: Some(cx),
    };
    let mut block_state = cx.results().get(start_idx).map_or_else(
        || {
            assert_eq!(start_idx, InstId(0));
            BlockState::default()
        },
        Clone::clone,
    );
    if let Some(end_idx) = end_idx
        && start_idx == end_idx
    {
        return VisitPathResult::Value {
            state: prev_state.cloned().unwrap(),
        };
    }

    for (idx, (inst_addr, inst)) in ti_iter(&cx.analysis().insts[start_idx..]) {
        let absolute_idx = InstId(start_idx.0 + idx.0);
        if absolute_idx != start_idx && cx.results().get(absolute_idx).is_some() {
            return visit_path(visitor, cx, Some(&mut block_state), absolute_idx, end_idx);
        }

        if let ControlFlow::Break(()) = visitor.visit_instruction(
            cx,
            *inst,
            idx,
            absolute_idx,
            end_idx,
            inst_addr.0,
            &mut block_state,
        ) {
            break;
        }
    }

    VisitPathResult::Value { state: block_state }
}
