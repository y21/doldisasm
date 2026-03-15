use std::{
    collections::HashMap,
    iter::{self},
};

use ppc32::{
    Instruction,
    instruction::{
        BranchOptions, Crb, Crf, Gpr, MicroSpr, Register, RegisterVisitor, Spr, XerRegister,
        compute_branch_target,
    },
};

use crate::{
    dataflow::{
        InstId, InstructionsDeref,
        core::{Dataflow, ForEachCtxt, Join, Predecessors, Results, SuccessorTarget, Successors},
        register_state::{CrFieldState, RegisterState},
    },
    ti_utils::ti_iter,
};

pub fn compute_preds_and_succs(
    insts: &InstructionsDeref,
    fn_address: u32,
    preds: &mut Predecessors<LocalGenerationAnalysis<'_>>,
    succs: &mut Successors<LocalGenerationAnalysis<'_>>,
) {
    let mut store_mapping = |from: InstId, to: SuccessorTarget<_>| {
        if let Some(to) = to.idx() {
            preds.entry(to).or_default().push(from);
        }
        succs.entry(from).or_default().push(to);
    };

    for (idx, &(off, inst)) in ti_iter(insts) {
        let next_instruction_idx = InstId(idx.0 + 1);
        if let Instruction::Bc {
            bo: _,
            bi: _,
            target,
            mode,
            link: false,
        } = inst
        {
            if let Some(target) = compute_branch_target(off.0, mode, target).checked_sub(fn_address)
            {
                // If we have a conditional branch to an address before the function itself (i.e. checked_sub = None due to overflow),
                // then that isn't part of this function and thus not something we need to analyze, hence the checked_sub.
                // The difference is also in bytes, so the instruction difference is that divided by 4.
                store_mapping(idx, SuccessorTarget::Id(InstId(target / 4)));
            }

            store_mapping(idx, SuccessorTarget::Id(next_instruction_idx));
        } else if let Instruction::Bclr { bo, bi: _, link } = inst {
            assert!(!link, "linking bclr not supported yet");

            store_mapping(idx, SuccessorTarget::Return);
            match bo {
                BranchOptions::BranchIfFalse | BranchOptions::BranchIfTrue => {
                    store_mapping(idx, SuccessorTarget::Id(next_instruction_idx))
                }
                BranchOptions::BranchAlways => {}
                BranchOptions::DecCTRBranchIfFalse => todo!(),
                BranchOptions::DecCTRBranchIfTrue => todo!(),
                BranchOptions::DecCTRBranchIfNotZero => todo!(),
                BranchOptions::DecCTRBranchIfZero => todo!(),
            }
        } else if let Instruction::Branch {
            target,
            mode,
            link: false,
        } = inst
            && let Some(target) = compute_branch_target(off.0, mode, target).checked_sub(fn_address)
        {
            store_mapping(idx, SuccessorTarget::Id(InstId(target / 4)));
        } else {
            store_mapping(idx, SuccessorTarget::Id(next_instruction_idx));
        }
    }

    succs.retain(|_, edges| match **edges {
        [SuccessorTarget::Id(target)] => preds[&target].len() > 1,
        // Make sure we always stop at `blr`
        [SuccessorTarget::Return] => true,
        [..] => true,
    });
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Generation(u32);

impl Generation {
    pub const INITIAL: Self = Self(0);

    #[must_use]
    pub fn next(self) -> Generation {
        Self(self.0 + 1)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegisterWithGeneration {
    pub reg: Register,
    pub generation: Generation,
}

#[derive(Default, PartialEq, Eq, Clone, Copy, Debug)]
pub struct LocalRegisterState {
    pub generation: Generation,
    /// If this generation is the result of joining two *different* generations together (phi), then this is `Some(_)` with said generations.
    pub phi_origins: Option<[Generation; 2]>,
    /// The highest generation currently in use
    pub highest_generation: Generation,
}

// FIXME: this join impl never reaches a fixpoint, so this is going to run into infinite loops with loops
impl Join<Generation> for LocalRegisterState {
    fn join(&self, other: &Self, recording_state: &mut Generation) -> Self {
        if self.generation == other.generation {
            Self {
                generation: self.generation,
                highest_generation: *recording_state,
                phi_origins: None, // no phi necessary, they are the same variable
            }
        } else {
            *recording_state = recording_state.next();
            Self {
                generation: *recording_state,
                phi_origins: Some([self.generation, other.generation]),
                highest_generation: *recording_state,
            }
        }
    }
}

impl LocalRegisterState {
    pub fn next_generation(&mut self) {
        self.highest_generation = self.highest_generation.next();
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
    pub register_generations: RegisterState<Generation>,
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
            record_state.register_generations.register_iter_mut(),
            block_state.registers.register_iter_mut(),
        )
        .for_each(|((record_reg, record_gen), (block_reg, block_gen))| {
            assert_eq!(record_reg, block_reg);
            block_gen.highest_generation = *record_gen;
        });
    }

    fn post_block_record(
        &self,
        record_state: &mut Self::RecordingState,
        block_state: &mut Self::BlockState,
    ) {
        iter::zip(
            record_state.register_generations.register_iter_mut(),
            block_state.registers.register_iter_mut(),
        )
        .for_each(|((record_reg, record_gen), (block_reg, block_gen))| {
            assert_eq!(record_reg, block_reg);
            *record_gen = block_gen.highest_generation;
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
            fn write_crb(&mut self, crf: Crf, crb: Crb) {
                self.state.registers.sprs.cr_mut(crf, crb).next_generation();
            }
            fn write_crf(&mut self, crf: Crf) {
                let CrFieldState { lt, gt, eq, so } =
                    &mut self.state.registers.sprs.cr[crf.0 as usize];
                lt.next_generation();
                gt.next_generation();
                eq.next_generation();
                so.next_generation();
            }
            fn write_gpr(&mut self, gpr: Gpr) {
                self.state.registers.gprs[gpr.0 as usize].next_generation();
            }
            fn write_spr(&mut self, spr: MicroSpr) {
                match spr {
                    Spr::Xer(XerRegister::So) => self.state.registers.sprs.xer.so.next_generation(),
                    Spr::Xer(XerRegister::Ov) => self.state.registers.sprs.xer.ov.next_generation(),
                    Spr::Xer(XerRegister::Ca) => self.state.registers.sprs.xer.ca.next_generation(),
                    Spr::Lr => self.state.registers.sprs.lr.next_generation(),
                    Spr::Ctr => todo!(),
                    Spr::Msr => self.state.registers.sprs.msr.next_generation(),
                    Spr::Pc => todo!(),
                    Spr::Other(_) => todo!(),
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
    pub fn uses_of(&self, reg: Register, generation: Generation) -> &[InstId] {
        self.map
            .get(&RegisterWithGeneration { reg, generation })
            .map(|v| v.as_slice())
            .unwrap_or_default()
    }

    pub fn has_uses(&self, reg: Register, generation: Generation) -> bool {
        !self.uses_of(reg, generation).is_empty()
    }
}

pub fn def_use_map<'a>(
    analysis: &LocalGenerationAnalysis<'a>,
    results: &Results<LocalGenerationAnalysis<'a>>,
) -> DefUseMap {
    // FIXME: currently we don't track transitive uses. This is probably important for joined visibilities to work correctly.
    let mut map = HashMap::new();

    results.for_each_with_input(analysis, |cx| {
        struct Vis<'a, 'b, 'c, 'd> {
            cx: &'a mut ForEachCtxt<'b, 'c, LocalGenerationAnalysis<'d>>,
            map: &'a mut InnerDefUseMap,
        }

        impl Vis<'_, '_, '_, '_> {
            pub fn register_use(&mut self, reg: Register, generation: Generation) {
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
            fn read_spr(&mut self, spr: MicroSpr) {
                self.register_use(
                    Register::Spr(spr),
                    match spr {
                        Spr::Xer(XerRegister::So) => {
                            self.cx.state().registers.sprs.xer.so.generation
                        }
                        Spr::Xer(XerRegister::Ov) => {
                            self.cx.state().registers.sprs.xer.ov.generation
                        }
                        Spr::Xer(XerRegister::Ca) => {
                            self.cx.state().registers.sprs.xer.ca.generation
                        }
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
