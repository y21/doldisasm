use std::{collections::HashSet, hash::Hash};

use indexmap::IndexSet;
use typed_index_collections::TiVec;

use crate::dataflow::{
    InstId,
    core::{Predecessors, SuccessorTarget, Successors},
    ssa::LocalGenerationAnalysis,
};

fn find_cycle(inst_id: InstId, succs: &Successors<LocalGenerationAnalysis<'_>>) -> bool {
    fn find_cycle_inner(
        start_inst_id: InstId,
        cur_inst_id: InstId,
        succs: &Successors<LocalGenerationAnalysis<'_>>,
        seen: &mut HashSet<InstId>,
    ) -> bool {
        if !seen.insert(cur_inst_id) {
            // We've got a cycle.
            if cur_inst_id == start_inst_id {
                return true;
            } else {
                return false;
            }
        }

        let (_, edges) = succs.range(cur_inst_id..).next().unwrap();
        for edge in edges {
            match *edge {
                SuccessorTarget::Id(id) => {
                    if find_cycle_inner(start_inst_id, id, succs, seen) {
                        return true;
                    }
                }
                SuccessorTarget::Return => {}
            }
        }

        false
    }
    find_cycle_inner(inst_id, inst_id, succs, &mut HashSet::new())
}

fn intersection<T: Eq + Hash>(mut left: IndexSet<T>, right: &IndexSet<T>) -> IndexSet<T> {
    left.retain(|e| right.contains(e));
    left
}

fn find_common_mergepoint_of_loop(
    inst_id: InstId,
    succs: &Successors<LocalGenerationAnalysis<'_>>,
) -> Option<InstId> {
    #[derive(Debug)]
    enum RecResult {
        Frozen(IndexSet<InstId>),
        ExitPath(IndexSet<InstId>),
        Cycle,
    }

    fn find_common_mergepoint_of_loop_inner(
        // start_inst_id: InstId,
        cur_inst_id: InstId,
        succs: &Successors<LocalGenerationAnalysis<'_>>,
        seen: &mut HashSet<InstId>,
    ) -> RecResult {
        if !seen.insert(cur_inst_id) {
            // We've got a cycle.
            return RecResult::Cycle;
        }

        // If there's a return edge, then make a new set and add the current inst id
        // If there's any other id edge, recurse() and add the current id if Some(set)
        // and take the intersection

        let (_, edges) = succs.range(cur_inst_id..).next().unwrap();
        let set = edges
            .iter()
            .map(|edge| match *edge {
                SuccessorTarget::Id(id) => {
                    let rec = find_common_mergepoint_of_loop_inner(id, succs, seen);
                    match rec {
                        RecResult::ExitPath(mut set) => {
                            set.insert(cur_inst_id);
                            RecResult::ExitPath(set)
                        }
                        other => other,
                    }
                }
                SuccessorTarget::Return => RecResult::ExitPath(IndexSet::from([cur_inst_id])),
            })
            .reduce(|acc, element| match (acc, element) {
                (RecResult::Frozen(set), _) | (_, RecResult::Frozen(set)) => RecResult::Frozen(set),
                (RecResult::ExitPath(set1), RecResult::ExitPath(set2)) => {
                    let res = intersection(set1, &set2);
                    RecResult::ExitPath(res)
                }
                (RecResult::ExitPath(mut set), RecResult::Cycle)
                | (RecResult::Cycle, RecResult::ExitPath(mut set)) => {
                    set.shift_remove(&cur_inst_id);
                    RecResult::Frozen(set)
                }
                (RecResult::Cycle, RecResult::Cycle) => RecResult::Cycle,
            })
            .unwrap(); // Every block always has at least one edge

        set
    }

    match find_common_mergepoint_of_loop_inner(inst_id, succs, &mut HashSet::new()) {
        RecResult::Frozen(set) | RecResult::ExitPath(set) => set.last().copied(),
        RecResult::Cycle => None,
    }
}

pub struct Loop {
    pub start: InstId,
    pub common_merge_inst: Option<InstId>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LoopId(u32);

impl From<usize> for LoopId {
    fn from(value: usize) -> Self {
        Self(value.try_into().unwrap())
    }
}
impl From<LoopId> for usize {
    fn from(value: LoopId) -> Self {
        value.0 as usize
    }
}

pub struct LoopMap(TiVec<LoopId, Loop>);

impl LoopMap {
    pub fn get(&self, id: LoopId) -> &Loop {
        &self.0[id]
    }
    pub fn find(&self, inst_id: InstId) -> Option<(LoopId, &Loop)> {
        self.0
            .iter_enumerated()
            .find(|&(_, &Loop { start, .. })| start == inst_id)
    }
}

pub fn find_loops<'a>(
    preds: &Predecessors<LocalGenerationAnalysis<'a>>,
    succs: &Successors<LocalGenerationAnalysis<'a>>,
) -> LoopMap {
    let mut loops = TiVec::new();

    for &inst_id in preds.keys() {
        if find_cycle(inst_id, succs) {
            // Starting at `inst_id`, visit all successor paths, ignore backedges,
            // and for paths that aren't backedges, collect the path into an IndexSet when returning recursively (causing the first one to be inserted last),
            // `&` the results together when visiting multiple successors,
            // then at the end do `.last()` -- that is the common merge point between all paths.
            let common_merge_inst = find_common_mergepoint_of_loop(inst_id, succs);

            loops.push(Loop {
                start: inst_id,
                common_merge_inst,
            });
        }
    }

    LoopMap(loops)
}
