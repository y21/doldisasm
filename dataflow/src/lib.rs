use std::{collections::HashMap, fmt::Debug, hash::Hash};

#[derive(PartialEq, Eq)]
pub enum SuccessorTarget<D: Dataflow> {
    /// Jump to a block.
    Id(D::Idx),
    /// Return from the function.
    Return,
}

impl<D> Copy for SuccessorTarget<D>
where
    D: Dataflow,
    D::Idx: Copy,
{
}
impl<D> Clone for SuccessorTarget<D>
where
    D: Dataflow,
    D::Idx: Clone,
{
    fn clone(&self) -> Self {
        match self {
            Self::Id(arg0) => Self::Id(arg0.clone()),
            Self::Return => Self::Return,
        }
    }
}

impl<D> Debug for SuccessorTarget<D>
where
    D: Dataflow,
    D::Idx: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SuccessorTarget::Id(idx) => write!(f, "Id({:?})", idx),
            SuccessorTarget::Return => write!(f, "Return"),
        }
    }
}

impl<D: Dataflow> SuccessorTarget<D> {
    pub fn idx(&self) -> Option<D::Idx> {
        match self {
            SuccessorTarget::Id(idx) => Some(*idx),
            SuccessorTarget::Return => None,
        }
    }
}

pub type Predecessors<D> = HashMap<<D as Dataflow>::Idx, Vec<<D as Dataflow>::Idx>>;
pub type Successors<D> = HashMap<<D as Dataflow>::Idx, Vec<SuccessorTarget<D>>>;

pub trait Dataflow: Sized {
    type Idx: Hash + Eq + Copy;
    type BlockState: Clone + Default + PartialEq;
    type BlockItem;

    fn compute_preds_and_succs(&self, preds: &mut Predecessors<Self>, succs: &mut Successors<Self>);
    fn initial_idx() -> Self::Idx;
    fn join_states(a: &Self::BlockState, b: &Self::BlockState) -> Self::BlockState;
    fn iter_block(&self, block: Self::Idx) -> impl Iterator<Item = (Self::Idx, Self::BlockItem)>;
    fn iter(&self) -> impl Iterator<Item = (Self::Idx, Self::BlockItem)>;
    fn apply_effect(&self, state: &mut Self::BlockState, idx: Self::Idx, data: &Self::BlockItem);
}

pub fn run<D: Dataflow>(dataflow: &D) -> Results<D>
where
    D::Idx: std::fmt::Debug,
    D::BlockItem: std::fmt::Debug,
    D::BlockState: std::fmt::Debug,
{
    let mut queue = vec![D::initial_idx()];

    let mut preds = HashMap::default();
    let mut succs = HashMap::default();
    let mut entry_states: HashMap<D::Idx, D::BlockState> = HashMap::default();

    dataflow.compute_preds_and_succs(&mut preds, &mut succs);

    while let Some(idx) = queue.pop() {
        println!("Process next! {idx:?}");
        let mut state = entry_states.get(&idx).cloned().unwrap_or_else(|| {
            assert_eq!(idx, D::initial_idx());
            D::BlockState::default()
        });

        for (idx, item) in dataflow.iter_block(idx) {
            dataflow.apply_effect(&mut state, idx, &item);

            if let Some(succs) = succs.get(&idx) {
                // Note: `succs` may be empty for `blr` (exit blocks).

                for &succ in succs {
                    if let SuccessorTarget::Id(succ) = succ {
                        if let Some(succ_state) = entry_states.get(&succ) {
                            let succ_state_joined = D::join_states(&state, succ_state);
                            let state_changed = &succ_state_joined != succ_state;

                            if state_changed {
                                queue.push(succ);
                                entry_states.insert(succ, succ_state_joined);
                            }
                        } else {
                            // First time visiting this successor.
                            queue.push(succ);
                            entry_states.insert(succ, state.clone());
                        }
                    }
                }

                break;
            }
        }
    }

    Results {
        states: entry_states,
    }
}

pub struct Results<D: Dataflow> {
    states: HashMap<D::Idx, D::BlockState>,
}

impl<D: Dataflow> Results<D>
where
    D::Idx: std::fmt::Debug,
    D::BlockState: std::fmt::Debug,
{
    /// Iterates over the results along with the input items.
    pub fn for_each_with_input(
        &self,
        analysis: &D,
        mut after_effect: impl FnMut(D::Idx, D::BlockItem, &D::BlockState),
    ) -> D::BlockState {
        let mut state = D::BlockState::default();

        for (idx, item) in analysis.iter() {
            if let Some(new_state) = self.states.get(&idx) {
                state = new_state.clone();
            }

            analysis.apply_effect(&mut state, idx, &item);

            after_effect(idx, item, &state);
        }

        state
    }
}

// fn dump_mapping(insts: &InstructionsDeref, mapping: &HashMap<InstId, Vec<InstId>>, name: &str) {
//     for (&from, to) in mapping {
//         for &to in to {
//             println!(
//                 "{name}: {:x?} -> {:x?}",
//                 insts[from as usize], insts[to as usize]
//             );
//         }
//     }
// }
