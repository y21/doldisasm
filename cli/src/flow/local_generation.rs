use dataflow::{Dataflow, Predecessors, SuccessorTarget, Successors};
use ppc32::{
    Instruction,
    instruction::{Register, RegisterVisitor, compute_branch_target},
};

use crate::flow::{InstId, InstructionsDeref, register_state::RegisterState, ti_iter};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegisterWithGeneration {
    pub reg: Register,
    pub generation: u32,
}

#[derive(Default, PartialEq, Eq, Clone, Debug)]
pub struct LocalRegisterState {
    pub generation: u32,
}

#[derive(Default, PartialEq, Eq, Clone, Debug)]
pub struct BlockState {
    pub registers: RegisterState<LocalRegisterState>,
}

pub struct LocalGenerationAnalysis<'a> {
    pub insts: &'a InstructionsDeref,
    pub fn_address: u32,
}

impl<'a> Dataflow for LocalGenerationAnalysis<'a> {
    type Idx = InstId;
    type BlockState = BlockState;
    type BlockItem = Instruction;

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
            }
        }
    }

    fn initial_idx() -> Self::Idx {
        InstId(0)
    }

    fn join_states(a: &Self::BlockState, b: &Self::BlockState) -> Self::BlockState {
        todo!()
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

    fn apply_effect(&self, state: &mut Self::BlockState, idx: Self::Idx, data: &Self::BlockItem) {
        struct Vis<'a, 'b> {
            state: &'a mut <LocalGenerationAnalysis<'b> as Dataflow>::BlockState,
        }
        impl<'a, 'b> RegisterVisitor for Vis<'a, 'b> {
            fn write_crb(&mut self, crb: ppc32::instruction::Crb) {
                // let field = crb.0 / 8;
                // let bit = crb.0 % 8;
                // self.state.registers.sprs.cr[]
                todo!()
            }
            fn write_crf(&mut self, _crf: ppc32::instruction::Crf) {
                // println!("Write to {_crf:?}");
                todo!()
            }
            fn write_gpr(&mut self, gpr: ppc32::instruction::Gpr) {
                self.state.registers.gprs[gpr.0 as usize].generation += 1;
            }
            fn write_spr(&mut self, spr: ppc32::instruction::Spr) {
                match spr {
                    ppc32::instruction::Spr::Xer => todo!(),
                    ppc32::instruction::Spr::Lr => self.state.registers.sprs.lr.generation += 1,
                    ppc32::instruction::Spr::Ctr => todo!(),
                    ppc32::instruction::Spr::Msr => self.state.registers.sprs.msr.generation += 1,
                    ppc32::instruction::Spr::Pc => todo!(),
                    ppc32::instruction::Spr::Other(_) => todo!(),
                }
            }
        }
        data.visit_registers(Vis { state });
    }
}
