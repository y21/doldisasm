use std::{
    collections::HashMap,
    iter::{self},
};

use ppc32::{
    Instruction,
    instruction::{Crb, Register, RegisterVisitor, Spr, compute_branch_target},
};

use crate::dataflow::{
    InstId, InstructionsDeref,
    core::{Dataflow, ForEachCtxt, Join, Predecessors, Results, SuccessorTarget, Successors},
    register_state::{CrFieldState, RegisterState},
    ti_iter,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegisterWithGeneration {
    pub reg: Register,
    pub generation: u32,
}

#[derive(Default, PartialEq, Eq, Clone, Copy, Debug)]
pub struct LocalRegisterState {
    pub generation: u32,
    pub highest_generation: u32,
}

// FIXME: this join impl never reaches a fixpoint, so this is going to run into infinite loops with loops
impl Join<u32> for LocalRegisterState {
    fn join(&self, _other: &Self, recording_state: &mut u32) -> Self {
        let generation = *recording_state;
        *recording_state += 1;
        Self {
            generation,
            highest_generation: *recording_state,
        }
    }
}

impl LocalRegisterState {
    pub fn next_generation(&mut self) {
        self.highest_generation += 1;
        self.generation = self.highest_generation;
    }
}

#[derive(Default, PartialEq, Eq, Clone, Debug)]
pub struct BlockState {
    pub registers: RegisterState<LocalRegisterState>,
}

impl Join<RecordingState> for BlockState {
    fn join(&self, other: &Self, arg: &mut RecordingState) -> Self {
        Self {
            registers: RegisterState {
                gprs: Join::join(
                    &self.registers.gprs,
                    &other.registers.gprs,
                    &mut arg.register_generations.gprs,
                ),
                sprs: Join::join(
                    &self.registers.sprs,
                    &other.registers.sprs,
                    &mut arg.register_generations.sprs,
                ),
            },
        }
    }
}

pub struct LocalGenerationAnalysis<'a> {
    pub insts: &'a InstructionsDeref,
    pub fn_address: u32,
}

#[derive(Default)]
pub struct RecordingState {
    pub register_generations: RegisterState<u32>,
}

impl<'a> Dataflow for LocalGenerationAnalysis<'a> {
    type Idx = InstId;
    type BlockState = BlockState;
    type BlockItem = Instruction;
    type RecordingState = RecordingState;

    fn pre_block_record(
        &self,
        record_state: &mut Self::RecordingState,
        block_state: &mut Self::BlockState,
    ) {
        iter::zip(
            record_state.register_generations.states_iter(),
            block_state.registers.states_iter(),
        )
        .for_each(|(record_gen, block_gen)| {
            block_gen.highest_generation = *record_gen;
        });
    }

    fn post_block_record(
        &self,
        record_state: &mut Self::RecordingState,
        block_state: &mut Self::BlockState,
    ) {
        iter::zip(
            record_state.register_generations.states_iter(),
            block_state.registers.states_iter(),
        )
        .for_each(|(record_gen, block_gen)| {
            *record_gen = block_gen.highest_generation;
        });
    }

    fn compute_preds_and_succs(
        &self,
        preds: &mut Predecessors<Self>,
        succs: &mut Successors<Self>,
    ) {
        let mut store_mapping = |from: InstId, to: SuccessorTarget<_>| {
            if let Some(to) = to.idx() {
                preds.entry(to).or_default().push(from);
            }
            succs.entry(from).or_default().push(to);
        };

        for (idx, &(off, inst)) in ti_iter(&self.insts) {
            if let Instruction::Bc {
                bo: _,
                bi: _,
                target,
                mode,
                link: false,
            } = inst
            {
                if let Some(target) =
                    compute_branch_target(off.0, mode, target).checked_sub(self.fn_address)
                {
                    // If we have a conditional branch to an address before the function itself (i.e. checked_sub = None due to overflow),
                    // then that isn't part of this function and thus not something we need to analyze, hence the checked_sub.
                    // The difference is also in bytes, so the instruction difference is that divided by 4.
                    store_mapping(idx, SuccessorTarget::Id(InstId(target / 4)));
                }

                store_mapping(idx, SuccessorTarget::Id(InstId(idx.0 + 1)));
            } else if let Instruction::Bclr { bo: _, bi: _, link } = inst {
                assert!(!link, "linking bclr not supported yet");
                store_mapping(idx, SuccessorTarget::Return);
            } else if let Instruction::Branch {
                target,
                mode,
                link: false,
            } = inst
                && let Some(target) =
                    compute_branch_target(off.0, mode, target).checked_sub(self.fn_address)
            {
                store_mapping(idx, SuccessorTarget::Id(InstId(target / 4)));
            } else {
                store_mapping(idx, SuccessorTarget::Id(InstId(idx.0 + 1)));
            }
        }

        succs.retain(|_, edges| match **edges {
            [SuccessorTarget::Id(target)] => preds[&target].len() > 1,
            // Make sure we always stop at `blr`
            [SuccessorTarget::Return] => true,
            [..] => true,
        });
    }

    fn initial_idx() -> Self::Idx {
        InstId(0)
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

    fn apply_effect(&self, state: &mut Self::BlockState, _: Self::Idx, data: &Self::BlockItem) {
        struct Vis<'a, 'b> {
            state: &'a mut <LocalGenerationAnalysis<'b> as Dataflow>::BlockState,
        }
        impl<'a, 'b> RegisterVisitor for Vis<'a, 'b> {
            fn write_crb(&mut self, crf: ppc32::instruction::Crf, crb: ppc32::instruction::Crb) {
                self.state.registers.sprs.cr_mut(crf, crb).next_generation();
            }
            fn write_crf(&mut self, crf: ppc32::instruction::Crf) {
                let CrFieldState { lt, gt, eq, so } =
                    &mut self.state.registers.sprs.cr[crf.0 as usize];
                lt.next_generation();
                gt.next_generation();
                eq.next_generation();
                so.next_generation();
            }
            fn write_gpr(&mut self, gpr: ppc32::instruction::Gpr) {
                self.state.registers.gprs[gpr.0 as usize].next_generation();
            }
            fn write_spr(&mut self, spr: ppc32::instruction::Spr) {
                match spr {
                    ppc32::instruction::Spr::Xer => todo!(),
                    ppc32::instruction::Spr::Lr => self.state.registers.sprs.lr.next_generation(),
                    ppc32::instruction::Spr::Ctr => todo!(),
                    ppc32::instruction::Spr::Msr => self.state.registers.sprs.msr.next_generation(),
                    ppc32::instruction::Spr::Pc => todo!(),
                    ppc32::instruction::Spr::Other(_) => todo!(),
                }
            }
        }
        data.visit_registers(Vis { state });
    }
}

type InnerDefUseMap = HashMap<RegisterWithGeneration, Vec<InstId>>;

#[derive(Debug)]
pub struct DefUseMap {
    map: InnerDefUseMap,
}

impl DefUseMap {
    pub fn uses_of(&self, reg: Register, generation: u32) -> &[InstId] {
        self.map
            .get(&RegisterWithGeneration { reg, generation })
            .map(|v| v.as_slice())
            .unwrap_or_default()
    }

    pub fn has_uses(&self, reg: Register, generation: u32) -> bool {
        !self.uses_of(reg, generation).is_empty()
    }
}

pub fn def_use_map<'a>(
    analysis: &LocalGenerationAnalysis<'a>,
    results: &Results<LocalGenerationAnalysis<'a>>,
) -> DefUseMap {
    let mut map = HashMap::new();

    results.for_each_with_input(analysis, |cx| {
        struct Vis<'a, 'b, 'c, 'd> {
            cx: &'a mut ForEachCtxt<'b, 'c, LocalGenerationAnalysis<'d>>,
            map: &'a mut InnerDefUseMap,
        }

        impl Vis<'_, '_, '_, '_> {
            pub fn register_use(&mut self, reg: Register, generation: u32) {
                let uses = self
                    .map
                    .entry(RegisterWithGeneration { reg, generation })
                    .or_default();
                if !uses.contains(&self.cx.idx()) {
                    uses.push(self.cx.idx());
                }
            }
        }

        impl RegisterVisitor for Vis<'_, '_, '_, '_> {
            fn read_crb(&mut self, crf: ppc32::instruction::Crf, crb: ppc32::instruction::Crb) {
                self.register_use(
                    Register::Cr(crf, crb),
                    self.cx.state().registers.sprs.cr(crf, crb).generation,
                );
            }
            fn read_crf(&mut self, crf: ppc32::instruction::Crf) {
                let field = &self.cx.state().registers.sprs.cr[crf.0 as usize];
                let lt = field.lt.generation;
                let gt = field.gt.generation;
                let eq = field.eq.generation;
                let so = field.so.generation;
                self.register_use(Register::Cr(crf, Crb::Negative), lt);
                self.register_use(Register::Cr(crf, Crb::Positive), gt);
                self.register_use(Register::Cr(crf, Crb::Zero), eq);
                self.register_use(Register::Cr(crf, Crb::Overflow), so);
            }
            fn read_gpr(&mut self, gpr: ppc32::instruction::Gpr) {
                self.register_use(
                    Register::Gpr(gpr),
                    self.cx.state().registers.gprs[gpr.0 as usize].generation,
                );
            }
            fn read_spr(&mut self, spr: ppc32::instruction::Spr) {
                self.register_use(
                    Register::Spr(spr),
                    match spr {
                        Spr::Xer => todo!(),
                        Spr::Lr => self.cx.state().registers.sprs.lr.generation,
                        Spr::Ctr => todo!(),
                        Spr::Msr => self.cx.state().registers.sprs.msr.generation,
                        Spr::Pc => todo!(),
                        Spr::Other(_) => todo!(),
                    },
                );
            }
            fn effect(&mut self) {
                self.cx.effect();
            }
        }

        cx.item().visit_registers(Vis { cx, map: &mut map });
    });

    DefUseMap { map }
}
