use std::collections::HashMap;

use arrayvec::ArrayVec;
use ppc32::{
    Instruction,
    instruction::{BranchOptions, Crb, Crf, Gpr, Register, Spr, compute_branch_target},
};
use typed_index_collections::TiVec;

use crate::{
    ast::stmt::{VarId, Variable, VariableFlags, VariableVisibility},
    dataflow::{
        InstId, InstructionsDeref,
        core::{Dataflow, Results},
        ssa::{BlockState, DefUseMap, LocalGenerationAnalysis, RegisterWithGeneration},
        ti_iter,
    },
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

    #[track_caller]
    pub fn mk_gpr_var(&mut self, gpr: Gpr, state: &BlockState, origin: VarId) -> VarId {
        self.mk_reg_var(
            Register::Gpr(gpr),
            state.registers.gprs[gpr.0 as usize].generation,
            origin,
        )
    }

    #[track_caller]
    pub fn mk_reg_var(&mut self, reg: Register, generation: u32, origin: VarId) -> VarId {
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
        generation: u32,
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

    pub fn optional_id_by_reg(&self, reg: Register, generation: u32) -> Option<VarId> {
        let reg = RegisterWithGeneration { reg, generation };
        self.reg_to_var.get(&reg).copied()
    }

    #[track_caller]
    pub fn id_by_reg(&self, reg: Register, generation: u32) -> VarId {
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

pub fn cr_bits_need_variable(
    state: &BlockState,
    def_use_map: &DefUseMap,
    crf: Crf,
) -> ArrayVec<Crb, 4> {
    let mut bits = ArrayVec::new();

    for crb in [Crb::Negative, Crb::Positive, Crb::Zero, Crb::Overflow] {
        let generation = state.registers.sprs.cr(crf, crb).generation;
        let uses = def_use_map.has_uses(Register::Cr(crf, crb), generation);
        if uses {
            bits.push(crb);
        }
    }

    bits
}

fn visit_path(
    instructions: &InstructionsDeref,
    start_index: InstId,
    variables: &mut Variables,
    local_generations: &Results<LocalGenerationAnalysis>,
    analysis: &LocalGenerationAnalysis<'_>,
    def_use_map: &DefUseMap,
) {
    let mut state = local_generations.get(start_index).map_or_else(
        || {
            assert_eq!(start_index, InstId(0));
            BlockState::default()
        },
        Clone::clone,
    );

    for (idx, (inst_addr, inst)) in ti_iter(&instructions[start_index..]) {
        match *inst {
            Instruction::Stwu {
                source,
                dest,
                imm: _,
            } => {
                let source = variables.id_by_gpr(source, &state);
                analysis.apply_effect(&mut state, idx, inst);
                variables.mk_gpr_var(dest, &state, source);
            }
            Instruction::Or {
                source,
                dest,
                or_with: _,
                rc,
            } => {
                let source = variables.id_by_gpr(source, &state);
                analysis.apply_effect(&mut state, idx, inst);
                variables.mk_gpr_var(dest, &state, source);

                if rc {
                    // create variables for those CR bits that are "used"
                    let crf = Crf(0);
                    for crb in cr_bits_need_variable(&state, def_use_map, crf) {
                        variables.mk_root_reg_var(
                            Register::Cr(crf, crb),
                            state.registers.sprs.cr(crf, crb).generation,
                            VariableVisibility::Visible,
                        );
                    }
                }
            }
            Instruction::Mfspr { dest, spr } => {
                if let Spr::Lr = spr {
                    let spr = variables
                        .id_by_reg(Register::Spr(Spr::Lr), state.registers.sprs.lr.generation);
                    analysis.apply_effect(&mut state, idx, inst);
                    variables.mk_gpr_var(dest, &state, spr);
                } else {
                    todo!()
                }
            }
            Instruction::Addi { dest, source, imm } => {
                if source == Gpr::ZERO {
                    // addi with r0 is just a load immediate
                    analysis.apply_effect(&mut state, idx, inst);
                    variables.mk_root_gpr_var(dest, &state, VariableVisibility::Visible);
                } else {
                    let source = if source == Gpr::STACK_POINTER {
                        if let Some(var) = variables.optional_id_by_stack_mem(imm.0) {
                            var
                        } else {
                            variables.mk_root_stack_mem_var(imm.0, VariableVisibility::Visible)
                        }
                    } else {
                        variables.id_by_gpr(source, &state)
                    };
                    analysis.apply_effect(&mut state, idx, inst);
                    variables.mk_gpr_var(dest, &state, source);
                }
            }
            Instruction::Stw { source, dest, imm } => {
                let source = variables.id_by_gpr(source, &state);
                analysis.apply_effect(&mut state, idx, inst);
                // We only create variables that are stack-relative.
                // TODO!: normalize address!!!
                if dest == Gpr::STACK_POINTER {
                    variables.mk_stack_mem_var(imm.0, source);
                }
            }
            Instruction::Branch {
                target: _,
                mode: _,
                link,
            } => {
                // TODO: anything to do with args???
                analysis.apply_effect(&mut state, idx, inst);
                if link {
                    // TODO: check if r3 is used with this generation and only then create the variable
                    variables.mk_root_gpr_var(Gpr::RETURN, &state, VariableVisibility::Visible);
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
                    (compute_branch_target(inst_addr.0, mode, target) - analysis.fn_address) / 4,
                );
                let false_idx = InstId(idx.0 + 1);

                analysis.apply_effect(&mut state, idx, inst);

                visit_path(
                    instructions,
                    true_idx,
                    variables,
                    local_generations,
                    analysis,
                    def_use_map,
                );
                visit_path(
                    instructions,
                    false_idx,
                    variables,
                    local_generations,
                    analysis,
                    def_use_map,
                );
                break;
            }
            Instruction::Lwz { dest, source, imm } => {
                // TODO: normalize address
                if source == Gpr::STACK_POINTER {
                    let mem_var = variables.id_by_stack_mem(imm.0);
                    analysis.apply_effect(&mut state, idx, inst);
                    variables.mk_gpr_var(dest, &state, mem_var);
                } else {
                    analysis.apply_effect(&mut state, idx, inst);
                    variables.mk_root_gpr_var(dest, &state, VariableVisibility::Visible);
                }
            }
            Instruction::Mtspr { source, spr } => {
                if let Spr::Lr = spr {
                    let source = variables.id_by_gpr(source, &state);
                    analysis.apply_effect(&mut state, idx, inst);
                    variables.mk_reg_var(
                        Register::Spr(Spr::Lr),
                        state.registers.sprs.lr.generation,
                        source,
                    );
                } else {
                    todo!()
                }
            }
            Instruction::Bclr { bo, bi: _, link } => {
                assert!(!link);
                assert!(bo == BranchOptions::BranchAlways);

                analysis.apply_effect(&mut state, idx, inst);
                break;
            }
            _ => todo!("{inst:x?}"),
        }
    }
}

pub fn infer_variables(
    instructions: &InstructionsDeref,
    local_generations: &Results<LocalGenerationAnalysis>,
    analysis: &LocalGenerationAnalysis<'_>,
    def_use_map: &DefUseMap,
) -> Variables {
    fn add_initial_hidden_root_var(variables: &mut Variables, register: Register) {
        variables.mk_root_reg_var(register, 0, VariableVisibility::Hidden);
    }

    let mut variables = Variables::new();

    variables.mk_root_reg_var(
        Register::Gpr(Gpr::STACK_POINTER),
        0,
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
        if def_use_map.has_uses(register, 0) {
            // TODO: this might actually be reachable if the function just doesn't use the second parameter but uses the third one.
            assert!(!end_of_params);

            variables.mk_root_reg_var(register, 0, VariableVisibility::Visible);
        } else {
            end_of_params = true;
        }
    }

    visit_path(
        instructions,
        InstId(0),
        &mut variables,
        local_generations,
        analysis,
        def_use_map,
    );

    variables
}
