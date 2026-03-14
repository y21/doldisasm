use std::{collections::HashMap, ops::ControlFlow};

use ppc32::{
    Instruction,
    instruction::{BranchOptions, Crb, Crf, Gpr, Register, Spr, compute_branch_target},
};
use typed_index_collections::TiVec;

use crate::{
    ast::stmt::{VarId, Variable, VariableFlags, VariableVisibility},
    dataflow::{
        InstId,
        core::{Dataflow, Results, Successors},
        ssa::{BlockState, DefUseMap, Generation, LocalGenerationAnalysis, RegisterWithGeneration},
    },
    visit::{self, JoinResult, PhiLocal, SuccessorsVisitor, VisitPathResult, VisitorCx},
};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct StackRelativeAddress {
    offset: i16,
}

#[derive(Debug)]
pub struct Variables {
    list: TiVec<VarId, Variable>,
    reg_to_var: HashMap<RegisterWithGeneration, VarId>,
    mem_to_var: HashMap<StackRelativeAddress, VarId>,
}

impl Variables {
    fn new() -> Self {
        Self {
            list: TiVec::new(),
            reg_to_var: HashMap::new(),
            mem_to_var: HashMap::new(),
        }
    }

    pub fn get(&self, var: VarId) -> &Variable {
        &self.list[var]
    }

    pub fn get_vis(&self, var: VarId) -> VariableVisibility {
        self.get(var).vis()
    }

    #[track_caller]
    pub fn mk_gpr_var(&mut self, gpr: Gpr, state: &BlockState, origin: VarId) -> VarId {
        self.mk_reg_var(
            Register::Gpr(gpr),
            state.registers.gprs[gpr.0 as usize].generation,
            origin,
        )
    }

    #[track_caller]
    pub fn mk_reg_var(&mut self, reg: Register, generation: Generation, origin: VarId) -> VarId {
        self.mk_root_reg_var(reg, generation, self.list[origin].vis())
    }

    #[track_caller]
    pub fn mk_root_gpr_var(
        &mut self,
        gpr: Gpr,
        state: &BlockState,
        vis: VariableVisibility,
    ) -> VarId {
        self.mk_root_reg_var(
            Register::Gpr(gpr),
            state.registers.gprs[gpr.0 as usize].generation,
            vis,
        )
    }

    #[track_caller]
    pub fn mk_root_reg_var(
        &mut self,
        reg: Register,
        generation: Generation,
        vis: VariableVisibility,
    ) -> VarId {
        let mut flags = VariableFlags::from_vis(vis);

        if let Register::Gpr(Gpr::STACK_POINTER) = reg {
            flags |= VariableFlags::RSP;
        }

        let key = self.list.push_and_get_key(Variable::new(flags));
        let reg = RegisterWithGeneration { reg, generation };
        assert!(self.reg_to_var.insert(reg, key).is_none());
        key
    }

    #[track_caller]
    pub fn id_by_gpr(&self, reg: Gpr, state: &BlockState) -> VarId {
        self.id_by_reg(
            Register::Gpr(reg),
            state.registers.gprs[reg.0 as usize].generation,
        )
    }

    pub fn optional_id_by_reg(&self, reg: Register, generation: Generation) -> Option<VarId> {
        let reg = RegisterWithGeneration { reg, generation };
        self.reg_to_var.get(&reg).copied()
    }

    #[track_caller]
    pub fn id_by_reg(&self, reg: Register, generation: Generation) -> VarId {
        match self.optional_id_by_reg(reg, generation) {
            Some(var_id) => var_id,
            None => panic!("no variable for {:?}_{:?}", reg, generation),
        }
    }

    #[track_caller]
    pub fn mk_root_stack_mem_var(&mut self, offset: i16, vis: VariableVisibility) -> VarId {
        let key = self
            .list
            .push_and_get_key(Variable::new(VariableFlags::from_vis(vis)));
        let addr = StackRelativeAddress { offset };
        assert!(
            self.mem_to_var.insert(addr, key).is_none(),
            "duplicate key: {addr:?}"
        );
        key
    }

    #[track_caller]
    pub fn mk_stack_mem_var(&mut self, offset: i16, origin: VarId) -> VarId {
        self.mk_root_stack_mem_var(offset, self.list[origin].vis())
    }

    pub fn optional_id_by_stack_mem(&self, offset: i16) -> Option<VarId> {
        let addr = StackRelativeAddress { offset };
        self.mem_to_var.get(&addr).copied()
    }

    #[track_caller]
    pub fn id_by_stack_mem(&self, offset: i16) -> VarId {
        match self.optional_id_by_stack_mem(offset) {
            Some(var_id) => var_id,
            None => panic!("no variable for stack-relative addr {offset:?}"),
        }
    }
}

pub fn cr_bits_variables(
    state: &BlockState,
    def_use_map: &DefUseMap,
    crf: Crf,
) -> [(Crb, VariableVisibility); 4] {
    [Crb::Negative, Crb::Positive, Crb::Zero, Crb::Overflow].map(|crb| {
        let generation = state.registers.sprs.cr(crf, crb).generation;
        let uses = def_use_map.has_uses(Register::Cr(crf, crb), generation);
        let vis = if uses {
            VariableVisibility::Visible
        } else {
            VariableVisibility::Hidden
        };
        (crb, vis)
    })
}

struct CollectVariables<'a> {
    variables: &'a mut Variables,
    def_use_map: &'a DefUseMap,
}

impl SuccessorsVisitor for CollectVariables<'_> {
    fn visit_instruction(
        &mut self,
        cx: &mut VisitorCx<'_, '_, '_>,
        inst: Instruction,
        idx: InstId,
        absolute_idx: InstId,
        end_idx: Option<InstId>,
        inst_addr: u32,
        state: &mut BlockState,
    ) -> ControlFlow<()> {
        match inst {
            Instruction::Stwu {
                source,
                dest,
                imm: _,
            } => {
                let source = self.variables.id_by_gpr(source, state);
                cx.analysis.apply_effect(state, idx, &inst);
                self.variables.mk_gpr_var(dest, &state, source);
                ControlFlow::Continue(())
            }
            Instruction::Cmpi {
                source,
                imm: _,
                crf,
            } => {
                let source_vis = self
                    .variables
                    .get_vis(self.variables.id_by_gpr(source, &state));
                cx.analysis.apply_effect(state, idx, &inst);
                for (crb, vis) in cr_bits_variables(&state, self.def_use_map, crf) {
                    self.variables.mk_root_reg_var(
                        Register::Cr(crf, crb),
                        state.registers.sprs.cr(crf, crb).generation,
                        source_vis & vis,
                    );
                }
                ControlFlow::Continue(())
            }
            Instruction::Or {
                source,
                dest,
                or_with: _,
                rc,
            } => {
                let source = self.variables.id_by_gpr(source, &state);
                cx.analysis.apply_effect(state, idx, &inst);
                self.variables.mk_gpr_var(dest, &state, source);

                if rc {
                    let crf = Crf(0);
                    for (crb, vis) in cr_bits_variables(&state, self.def_use_map, crf) {
                        self.variables.mk_root_reg_var(
                            Register::Cr(crf, crb),
                            state.registers.sprs.cr(crf, crb).generation,
                            vis,
                        );
                    }
                }
                ControlFlow::Continue(())
            }
            Instruction::Mfspr { dest, spr } => {
                if let Spr::Lr = spr {
                    let spr = self
                        .variables
                        .id_by_reg(Register::Spr(Spr::Lr), state.registers.sprs.lr.generation);
                    cx.analysis.apply_effect(state, idx, &inst);
                    self.variables.mk_gpr_var(dest, &state, spr);
                } else {
                    todo!()
                }
                ControlFlow::Continue(())
            }
            Instruction::Addi { dest, source, imm } => {
                if source == Gpr::ZERO {
                    // addi with r0 is just a load immediate
                    cx.analysis.apply_effect(state, idx, &inst);
                    self.variables
                        .mk_root_gpr_var(dest, &state, VariableVisibility::Visible);
                } else {
                    let source = if source == Gpr::STACK_POINTER {
                        if let Some(var) = self.variables.optional_id_by_stack_mem(imm.0) {
                            var
                        } else {
                            self.variables
                                .mk_root_stack_mem_var(imm.0, VariableVisibility::Visible)
                        }
                    } else {
                        self.variables.id_by_gpr(source, &state)
                    };
                    cx.analysis.apply_effect(state, idx, &inst);
                    self.variables.mk_gpr_var(dest, &state, source);
                }
                ControlFlow::Continue(())
            }
            Instruction::Stw { source, dest, imm } => {
                let source = self.variables.id_by_gpr(source, &state);
                cx.analysis.apply_effect(state, idx, &inst);
                // We only create variables that are stack-relative.
                // TODO!: normalize address!!!
                if dest == Gpr::STACK_POINTER {
                    self.variables.mk_stack_mem_var(imm.0, source);
                }
                ControlFlow::Continue(())
            }
            Instruction::Branch { target, mode, link } => {
                let target = compute_branch_target(inst_addr, mode, target);
                // TODO: anything to do with args???
                cx.analysis.apply_effect(state, idx, &inst);
                if link {
                    // TODO: check if r3 is used with this generation and only then create the variable
                    self.variables.mk_root_gpr_var(
                        Gpr::RETURN,
                        &state,
                        VariableVisibility::Visible,
                    );
                    ControlFlow::Continue(())
                } else {
                    let idx = InstId((target - cx.analysis.fn_address) / 4);
                    let VisitPathResult { state: _ } =
                        visit::visit_path(self, cx, Some(state), idx, end_idx);
                    ControlFlow::Break(())
                }
            }
            Instruction::Bc {
                bo: _,
                bi: _,
                target,
                mode,
                link,
            } => {
                assert!(!link);

                let true_idx = InstId(
                    (compute_branch_target(inst_addr, mode, target) - cx.analysis.fn_address) / 4,
                );
                let false_idx = InstId(absolute_idx.0 + 1);

                cx.analysis.apply_effect(state, idx, &inst);

                let JoinResult {
                    true_res: _,
                    false_res: _,
                    phi_locals,
                    common_merge_inst,
                } = visit::visit_and_join_paths(self, cx, state, true_idx, false_idx);

                for PhiLocal {
                    register,
                    next_gen,
                    true_gen,
                    false_gen,
                } in phi_locals
                {
                    let true_vis = self
                        .variables
                        .optional_id_by_reg(register, true_gen)
                        .map_or_else(|| VariableVisibility::Hidden, |v| self.variables.get_vis(v));
                    let false_vis = self
                        .variables
                        .optional_id_by_reg(register, false_gen)
                        .map_or_else(|| VariableVisibility::Hidden, |v| self.variables.get_vis(v));

                    if self
                        .variables
                        .optional_id_by_reg(register, next_gen)
                        .is_none()
                    {
                        // The variable may already exist if the common instruction has multiple (other) predecessors
                        self.variables
                            .mk_root_reg_var(register, next_gen, true_vis | false_vis);
                    }
                }

                if let Some(common_merge_inst) = common_merge_inst {
                    let VisitPathResult { state: _ } =
                        visit::visit_path(self, cx, Some(state), common_merge_inst, end_idx);
                }
                ControlFlow::Break(())
            }
            Instruction::Lwz { dest, source, imm } => {
                // TODO: normalize address
                if source == Gpr::STACK_POINTER {
                    let mem_var = self.variables.id_by_stack_mem(imm.0);
                    cx.analysis.apply_effect(state, idx, &inst);
                    self.variables.mk_gpr_var(dest, &state, mem_var);
                } else {
                    cx.analysis.apply_effect(state, idx, &inst);
                    self.variables
                        .mk_root_gpr_var(dest, &state, VariableVisibility::Visible);
                }
                ControlFlow::Continue(())
            }
            Instruction::Mtspr { source, spr } => {
                if let Spr::Lr = spr {
                    let source = self.variables.id_by_gpr(source, &state);
                    cx.analysis.apply_effect(state, idx, &inst);
                    self.variables.mk_reg_var(
                        Register::Spr(Spr::Lr),
                        state.registers.sprs.lr.generation,
                        source,
                    );
                } else {
                    todo!()
                }
                ControlFlow::Continue(())
            }
            Instruction::Bclr { bo, bi: _, link } => {
                assert!(!link);

                cx.analysis.apply_effect(state, idx, &inst);
                if bo == BranchOptions::BranchAlways {
                    ControlFlow::Break(())
                } else {
                    visit::visit_path(self, cx, Some(state), absolute_idx + 1, end_idx);
                    ControlFlow::Break(())
                }
            }
            _ => todo!("{inst:x?}"),
        }
    }
}

pub fn infer_variables<'a>(
    local_generations: &Results<LocalGenerationAnalysis<'a>>,
    analysis: &LocalGenerationAnalysis<'a>,
    def_use_map: &DefUseMap,
    succs: &Successors<LocalGenerationAnalysis<'a>>,
) -> Variables {
    fn add_initial_hidden_root_var(variables: &mut Variables, register: Register) {
        variables.mk_root_reg_var(register, Generation::INITIAL, VariableVisibility::Hidden);
    }

    let mut variables = Variables::new();

    variables.mk_root_reg_var(
        Register::Gpr(Gpr::STACK_POINTER),
        Generation::INITIAL,
        VariableVisibility::Visible,
    );
    add_initial_hidden_root_var(&mut variables, Register::Spr(Spr::Lr));

    for reg in 14..=31 {
        // Callee saved registers are hidden
        add_initial_hidden_root_var(&mut variables, Register::Gpr(Gpr(reg)));
    }

    // Parameters
    let mut end_of_params = false;
    for reg in 3..=8 {
        let register = Register::Gpr(Gpr(reg));
        if def_use_map.has_uses(register, Generation::INITIAL) {
            // TODO: this might actually be reachable if the function just doesn't use the second parameter but uses the third one.
            assert!(!end_of_params);

            variables.mk_root_reg_var(register, Generation::INITIAL, VariableVisibility::Visible);
        } else {
            end_of_params = true;
        }
    }

    let mut vars = CollectVariables {
        variables: &mut variables,
        def_use_map,
    };
    let mut cx = VisitorCx {
        analysis,
        results: local_generations,
        succs,
    };
    visit::visit_path(&mut vars, &mut cx, None, InstId(0), None);

    variables
}
