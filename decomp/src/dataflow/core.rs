use std::{array, collections::HashMap, fmt::Debug, hash::Hash};

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

pub trait Join<A>: Sized {
    fn join(&self, other: &Self, arg: &mut A) -> Self;
}

impl<const N: usize, S: Join<T>, T> Join<[T; N]> for [S; N] {
    fn join(&self, other: &Self, arg: &mut [T; N]) -> Self {
        array::from_fn(|i| Join::join(&self[i], &other[i], &mut arg[i]))
    }
}

pub trait Dataflow: Sized {
    type Idx: Hash + Eq + Copy;
    type BlockState: Clone + Default + PartialEq + Join<Self::RecordingState>;
    type BlockItem: Copy;
    type RecordingState: Default;

    fn compute_preds_and_succs(&self, preds: &mut Predecessors<Self>, succs: &mut Successors<Self>);
    fn initial_idx() -> Self::Idx;
    fn iter_block(&self, block: Self::Idx) -> impl Iterator<Item = (Self::Idx, Self::BlockItem)>;
    fn iter(&self) -> impl Iterator<Item = (Self::Idx, Self::BlockItem)>;
    fn apply_effect(&self, state: &mut Self::BlockState, idx: Self::Idx, data: &Self::BlockItem);
    fn pre_block_record(
        &self,
        rec_state: &mut Self::RecordingState,
        block_state: &mut Self::BlockState,
    );
    fn post_block_record(
        &self,
        rec_state: &mut Self::RecordingState,
        block_state: &mut Self::BlockState,
    );
}

pub fn run<D: Dataflow>(dataflow: &D) -> Results<D>
where
    D::Idx: std::fmt::Debug,
{
    let mut queue = vec![D::initial_idx()];

    let mut preds = HashMap::default();
    let mut succs = HashMap::default();
    let mut entry_states: HashMap<D::Idx, D::BlockState> = HashMap::default();

    dataflow.compute_preds_and_succs(&mut preds, &mut succs);

    let mut record_state = D::RecordingState::default();

    while let Some(idx) = queue.pop() {
        let mut state = entry_states.get(&idx).cloned().unwrap_or_else(|| {
            assert_eq!(idx, D::initial_idx());
            D::BlockState::default()
        });

        dataflow.pre_block_record(&mut record_state, &mut state);
        entry_states.insert(idx, state.clone());

        for (idx, item) in dataflow.iter_block(idx) {
            dataflow.apply_effect(&mut state, idx, &item);

            if let Some(succs) = succs.get(&idx) {
                dataflow.post_block_record(&mut record_state, &mut state);
                // Note: `succs` may be empty for `blr` (exit blocks).

                for &succ in succs {
                    if let SuccessorTarget::Id(succ) = succ {
                        if let Some(succ_state) = entry_states.get(&succ) {
                            let succ_state_joined = state.join(succ_state, &mut record_state);
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

pub struct ForEachCtxt<'analysis, 'iter, D: Dataflow> {
    analysis: &'analysis D,
    idx: D::Idx,
    item: D::BlockItem,
    state: &'iter mut D::BlockState,
    apply_effect_called: bool,
}

impl<'analysis, 'iter, D: Dataflow> ForEachCtxt<'analysis, 'iter, D> {
    pub fn idx(&self) -> D::Idx {
        self.idx
    }
    pub fn item(&self) -> D::BlockItem {
        self.item
    }
    pub fn state(&self) -> &D::BlockState {
        self.state
    }
    pub fn effect(&mut self) {
        assert!(
            !self.apply_effect_called,
            "attempted to apply effect multiple times for the same item"
        );
        self.apply_effect_called = true;
        self.analysis.apply_effect(self.state, self.idx, &self.item);
    }
}

impl<D: Dataflow> Results<D>
where
    D::Idx: std::fmt::Debug,
    D::BlockState: std::fmt::Debug,
{
    pub fn get(&self, idx: D::Idx) -> Option<&D::BlockState> {
        self.states.get(&idx)
    }

    /// Iterates over the results along with the input items.
    pub fn for_each_with_input<'analysis>(
        &self,
        analysis: &'analysis D,
        mut callback: impl FnMut(&mut ForEachCtxt<'analysis, '_, D>),
    ) -> D::BlockState {
        let mut state: D::BlockState = D::BlockState::default();

        for (idx, item) in analysis.iter() {
            if let Some(new_state) = self.states.get(&idx) {
                state = new_state.clone();
            }

            let mut cx = ForEachCtxt {
                analysis,
                state: &mut state,
                apply_effect_called: false,
                idx,
                item,
            };
            callback(&mut cx);
            assert!(
                cx.apply_effect_called,
                "callback did not call effect() for idx {:?}",
                idx
            );
        }

        state
    }
}
