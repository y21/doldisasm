use std::{collections::HashMap, hash::Hash};

pub type PredecessorsSuccessors<D> = HashMap<<D as Dataflow>::Idx, Vec<<D as Dataflow>::Idx>>;

pub trait Dataflow {
    type Idx: Hash + Eq + Copy;
    type BlockState: Clone + Default + PartialEq;
    type BlockItem;

    fn compute_preds_and_succs(
        &self,
        preds: &mut PredecessorsSuccessors<Self>,
        succs: &mut PredecessorsSuccessors<Self>,
    );
    fn initial_idx() -> Self::Idx;
    fn join_states(a: &Self::BlockState, b: &Self::BlockState) -> Self::BlockState;
    fn iter_block(&self, block: Self::Idx) -> impl Iterator<Item = (Self::Idx, Self::BlockItem)>;
    fn iter(&self) -> impl Iterator<Item = (Self::Idx, Self::BlockItem)>;
    fn apply_effect(&self, state: &mut Self::BlockState, data: &Self::BlockItem);
}

pub fn run<D: Dataflow>(dataflow: &D) -> Results<D> {
    let mut queue = vec![D::initial_idx()];

    let mut preds: HashMap<D::Idx, Vec<D::Idx>> = HashMap::default();
    let mut succs: HashMap<D::Idx, Vec<D::Idx>> = HashMap::default();
    let mut states: HashMap<D::Idx, D::BlockState> = HashMap::default();

    dataflow.compute_preds_and_succs(&mut preds, &mut succs);

    while let Some(idx) = queue.pop() {
        let mut state: D::BlockState = if let Some(preds) = preds.get(&idx) {
            preds
                .iter()
                .map(|block| &states[block])
                .fold(None, |acc, state| {
                    if let Some(acc) = acc {
                        Some(D::join_states(&acc, state))
                    } else {
                        Some(state.clone())
                    }
                })
                .unwrap()
        } else {
            assert!(idx == D::initial_idx());
            D::BlockState::default()
        };

        for (idx, item) in dataflow.iter_block(idx) {
            dataflow.apply_effect(&mut state, &item);

            if let Some(succs) = succs.get(&idx) {
                let state_changed = states.get(&idx).is_none_or(|old| old != &state);
                if state_changed {
                    queue.extend(succs.iter().copied());
                    states.insert(idx, state);
                }
                break;
            }
        }
    }

    Results { states }
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

            analysis.apply_effect(&mut state, &item);

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
