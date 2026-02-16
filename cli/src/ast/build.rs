use std::collections::HashMap;

use dataflow::{Dataflow, Results};
use ppc32::{
    Instruction,
    instruction::{
        BranchOptions, Crb, Crf, Gpr, Register, Spr, compute_branch_target, crb_from_index,
    },
};
use typed_index_collections::TiVec;

use crate::{
    ast::{
        Ast,
        expr::{BinaryExpr, BinaryOp, Expr, ExprKind, FnCallTarget, UnaryExpr, UnaryOp},
        item::{Function, Item, ItemKind, Parameter},
        stmt::{Stmt, StmtKind, VarId, Variable, VariableVisibility},
        ty::{Ty, TyKind},
    },
    flow::{
        InstId, InstructionsDeref,
        local_generation::{
            BlockState, DefUseMap, LocalGenerationAnalysis, RegisterWithGeneration,
        },
        ti_iter,
    },
};

pub struct AstBuildParams<'a, 'b> {
    pub instructions: &'a InstructionsDeref,
    pub local_generations: &'a Results<LocalGenerationAnalysis<'b>>,
    pub analysis: &'a LocalGenerationAnalysis<'a>,
    pub def_use_map: &'a DefUseMap,
}

struct Variables {
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

    fn get(&self, var: VarId) -> &Variable {
        &self.list[var]
    }

    #[track_caller]
    fn mk_gpr_var(&mut self, gpr: Gpr, state: &BlockState, origin: VarId) -> VarId {
        self.mk_reg_var(
            Register::Gpr(gpr),
            state.registers.gprs[gpr.0 as usize].generation,
            origin,
        )
    }

    #[track_caller]
    fn mk_reg_var(&mut self, reg: Register, generation: u32, origin: VarId) -> VarId {
        let key = self.list.push_and_get_key(Variable {
            vis: self.list[origin].vis,
        });
        let reg = RegisterWithGeneration { reg, generation };
        assert!(
            self.reg_to_var.insert(reg, key).is_none(),
            "variable for {:?}_{:?} already exists",
            reg.reg,
            reg.generation
        );
        key
    }

    #[track_caller]
    fn mk_root_gpr_var(&mut self, gpr: Gpr, state: &BlockState, vis: VariableVisibility) -> VarId {
        self.mk_root_reg_var(
            Register::Gpr(gpr),
            state.registers.gprs[gpr.0 as usize].generation,
            vis,
        )
    }

    #[track_caller]
    fn mk_root_reg_var(
        &mut self,
        reg: Register,
        generation: u32,
        vis: VariableVisibility,
    ) -> VarId {
        let key = self.list.push_and_get_key(Variable { vis });
        let reg = RegisterWithGeneration { reg, generation };
        assert!(self.reg_to_var.insert(reg, key).is_none());
        key
    }

    #[track_caller]
    fn id_by_gpr(&mut self, reg: Gpr, state: &BlockState) -> VarId {
        self.id_by_reg(
            Register::Gpr(reg),
            state.registers.gprs[reg.0 as usize].generation,
        )
    }

    #[track_caller]
    fn id_by_reg(&mut self, reg: Register, generation: u32) -> VarId {
        let reg = RegisterWithGeneration { reg, generation };

        match self.reg_to_var.get(&reg) {
            Some(var_id) => *var_id,
            None => panic!("no variable for {:?}_{:?}", reg.reg, reg.generation),
        }
    }

    fn mk_root_stack_mem_var(&mut self, offset: i16, vis: VariableVisibility) -> VarId {
        let key = self.list.push_and_get_key(Variable { vis });
        let addr = StackRelativeAddress { offset };
        assert!(self.mem_to_var.insert(addr, key).is_none());
        key
    }

    fn mk_stack_mem_var(&mut self, offset: i16, origin: VarId) -> VarId {
        let key = self.list.push_and_get_key(Variable {
            vis: self.list[origin].vis,
        });
        let addr = StackRelativeAddress { offset };
        assert!(self.mem_to_var.insert(addr, key).is_none());
        key
    }

    #[track_caller]
    fn id_by_stack_mem(&mut self, offset: i16) -> VarId {
        let addr = StackRelativeAddress { offset };

        match self.mem_to_var.get(&addr) {
            Some(var_id) => *var_id,
            None => panic!("no variable for stack-relative addr {addr:?}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct StackRelativeAddress {
    offset: i16,
}

fn build_path(
    instructions: &InstructionsDeref,
    idx: InstId,
    local_generations: &Results<LocalGenerationAnalysis<'_>>,
    analysis: &LocalGenerationAnalysis<'_>,
    variables: &mut Variables,
    def_use_map: &DefUseMap,
) -> Vec<Stmt> {
    let mut state = local_generations.get(idx).map_or_else(
        || {
            assert_eq!(idx, InstId(0));
            BlockState::default()
        },
        Clone::clone,
    );

    let mut stmts = Vec::new();

    for (idx, (inst_addr, instruction)) in ti_iter(&instructions[idx..]) {
        match *instruction {
            Instruction::Stwu { source, dest, imm } => {
                assert!(source == Gpr::STACK_POINTER && dest == Gpr::STACK_POINTER);

                let source = variables.id_by_gpr(source, &state);
                analysis.apply_effect(&mut state, idx, instruction);

                // ???
                variables.mk_gpr_var(dest, &state, source);
            }
            Instruction::Or {
                source,
                dest,
                or_with,
                rc,
            } => {
                if or_with == source {
                    // This is a move.

                    let source = variables.id_by_gpr(source, &state);

                    analysis.apply_effect(&mut state, idx, instruction);

                    let dest = variables.mk_gpr_var(dest, &state, source);
                    let visibility = variables.get(dest).vis;

                    if visibility == VariableVisibility::Visible {
                        stmts.push(Stmt {
                            kind: StmtKind::Assign {
                                dest: Expr {
                                    kind: ExprKind::Var(dest),
                                },
                                value: Expr {
                                    kind: ExprKind::Var(source),
                                },
                            },
                        });
                    }

                    if rc {
                        // Make sure code doesn't try to branch on a hidden variable. This could happen, but I'm not sure how to deal with that yet.
                        assert!(visibility == VariableVisibility::Visible);

                        let crf = Crf(0);
                        for crb in [Crb::Negative, Crb::Positive, Crb::Zero, Crb::Overflow] {
                            let generation = state.registers.sprs.cr(crf, crb).generation;

                            let register = Register::Cr(crf, crb);
                            let uses = def_use_map.uses_of(register, generation);
                            if !uses.is_empty() {
                                // This bit is used, we must create a variable for it and assign it here based on the value of `source`.
                                let var = variables.mk_root_reg_var(
                                    register,
                                    generation,
                                    VariableVisibility::Visible,
                                );

                                stmts.push(Stmt {
                                    kind: StmtKind::Assign {
                                        dest: Expr {
                                            kind: ExprKind::Var(var),
                                        },
                                        value: Expr {
                                            kind: ExprKind::Binary(BinaryExpr {
                                                op: match crb {
                                                    Crb::Negative => BinaryOp::Lt,
                                                    Crb::Positive => BinaryOp::Gt,
                                                    Crb::Zero => BinaryOp::Eq,
                                                    Crb::Overflow => todo!(),
                                                },
                                                left: Box::new(Expr {
                                                    // TODO: should we do a cast to i16 here?
                                                    kind: ExprKind::Var(dest),
                                                }),
                                                right: Box::new(Expr {
                                                    kind: ExprKind::Immediate16(0),
                                                }),
                                            }),
                                        },
                                    },
                                });
                            }
                        }
                    }
                } else {
                    // TODO: once we implement this, most of the above also applies here (Rc=1).
                    todo!()
                }
            }
            Instruction::Mfspr { dest, spr } => {
                if let Spr::Lr = spr {
                    // Probably nothing to do?
                    let generation = state.registers.sprs.lr.generation;
                    let spr = variables.id_by_reg(Register::Spr(spr), generation);
                    analysis.apply_effect(&mut state, idx, instruction);
                    variables.mk_gpr_var(dest, &state, spr);
                } else {
                    todo!("{instruction:?}"); // TODO: make sure to have apply_effect here too
                }
            }
            Instruction::Addi { dest, source, imm } => {
                let source = variables.id_by_gpr(source, &state);

                analysis.apply_effect(&mut state, idx, instruction);

                let dest = variables.mk_gpr_var(dest, &state, source);
                let visibility = variables.get(dest).vis;

                if visibility == VariableVisibility::Visible {
                    stmts.push(Stmt {
                        kind: StmtKind::Assign {
                            dest: Expr {
                                kind: ExprKind::Var(dest),
                            },
                            value: Expr {
                                kind: ExprKind::Binary(BinaryExpr {
                                    op: BinaryOp::Add,
                                    left: Box::new(Expr {
                                        kind: ExprKind::Var(source),
                                    }),
                                    right: Box::new(Expr {
                                        kind: ExprKind::Immediate16(imm.0),
                                    }),
                                }),
                            },
                        },
                    });
                }
            }
            Instruction::Stw { source, dest, imm } => {
                if dest == Gpr::STACK_POINTER {
                    // Writing to a stack-relative address - probably a write to a variable

                    let source = variables.id_by_gpr(source, &state);

                    analysis.apply_effect(&mut state, idx, instruction);

                    let dest: VarId = variables.mk_stack_mem_var(imm.0, source);
                    let vis = variables.get(dest).vis;

                    // Don't create an assignment if this is just saving a callee-saved register
                    if vis == VariableVisibility::Visible {
                        stmts.push(Stmt {
                            kind: StmtKind::Assign {
                                dest: Expr {
                                    kind: ExprKind::Var(dest),
                                },
                                value: Expr {
                                    kind: ExprKind::Var(source),
                                },
                            },
                        });
                    }
                } else {
                    todo!()
                }
            }
            Instruction::Branch { target, mode, link } => {
                let target = compute_branch_target(inst_addr.0, mode, target);
                if link {
                    // Function call. Probably.
                    analysis.apply_effect(&mut state, idx, instruction);

                    // TODO: check if r3 with this generation is used anywhere to tell if it even returns a value at all.
                    let return_var =
                        variables.mk_root_gpr_var(Gpr::RETURN, &state, VariableVisibility::Visible);
                    stmts.push(Stmt {
                        kind: StmtKind::Assign {
                            dest: Expr {
                                kind: ExprKind::Var(return_var),
                            },
                            value: Expr {
                                kind: ExprKind::FnCall(
                                    FnCallTarget::Addr(target),
                                    Vec::new(), // TODO: figure out how to detect what parameters the fn takes?
                                ),
                            },
                        },
                    });
                } else {
                    todo!()
                }
            }
            Instruction::Bc {
                bo,
                bi,
                target,
                mode,
                link,
            } => {
                assert!(!link);

                let true_idx = InstId(
                    (compute_branch_target(inst_addr.0, mode, target) - analysis.fn_address) / 4,
                );
                let false_idx = InstId(idx.0 + 1);

                // TODO!! find the next common instruction between the two paths and only build statements until there,
                // then build the path from *that* common instruction.

                let (crf, crb) = crb_from_index(bi);
                let generation = state.registers.sprs.cr(crf, crb).generation;
                let condition = variables.id_by_reg(Register::Cr(crf, crb), generation);

                analysis.apply_effect(&mut state, idx, instruction);

                let then_stmts = build_path(
                    instructions,
                    true_idx,
                    local_generations,
                    analysis,
                    variables,
                    def_use_map,
                );
                let else_stmts = build_path(
                    instructions,
                    false_idx,
                    local_generations,
                    analysis,
                    variables,
                    def_use_map,
                );

                let mut condition = Expr {
                    kind: ExprKind::Var(condition),
                };
                match bo {
                    BranchOptions::DecCTRBranchIfFalse => todo!(),
                    BranchOptions::BranchIfFalse => {
                        condition = Expr {
                            kind: ExprKind::Unary(UnaryExpr {
                                op: UnaryOp::Not,
                                operand: Box::new(condition),
                            }),
                        }
                    }
                    BranchOptions::DecCTRBranchIfTrue => todo!(),
                    BranchOptions::BranchIfTrue => {}
                    BranchOptions::DecCTRBranchIfNotZero => todo!(),
                    BranchOptions::DecCTRBranchIfZero => todo!(),
                    BranchOptions::BranchAlways => todo!(),
                }

                stmts.push(Stmt {
                    kind: StmtKind::If {
                        condition,
                        then_stmts,
                        else_stmts,
                    },
                });
                break;
            }
            Instruction::Lwz { dest, source, imm } => {
                assert!(source != Gpr(0)); // TODO: source == 0 means no register and it loads from imm alone. handle this.

                if source == Gpr::STACK_POINTER {
                    // Stack-relative load

                    let source = variables.id_by_stack_mem(imm.0);

                    analysis.apply_effect(&mut state, idx, instruction);

                    let dest = variables.mk_gpr_var(dest, &state, source);
                    stmts.push(Stmt {
                        kind: StmtKind::Assign {
                            dest: Expr {
                                kind: ExprKind::Var(dest),
                            },
                            value: Expr {
                                kind: ExprKind::Var(source),
                            },
                        },
                    });
                } else {
                    todo!();
                }
            }
            Instruction::Mtspr { source, spr } => {
                if let Spr::Lr = spr {
                    let source = variables.id_by_gpr(source, &state);
                    analysis.apply_effect(&mut state, idx, instruction);
                    variables.mk_reg_var(
                        Register::Spr(spr),
                        state.registers.sprs.lr.generation,
                        source,
                    );
                } else {
                    todo!("{instruction:?}"); // Make sure to add apply_effect here too
                }
            }
            Instruction::Bclr { bo, bi, link } => {
                assert!(!link);
                assert!(bo == BranchOptions::BranchAlways);

                analysis.apply_effect(&mut state, idx, instruction);

                // TODO: dont hardcode None, there may be a return value
                stmts.push(Stmt {
                    kind: StmtKind::Return(None),
                });
                break;
            }
            _ => todo!("{instruction:?}"),
        }
    }
    stmts
}

pub fn build(
    AstBuildParams {
        instructions,
        local_generations,
        analysis,
        def_use_map,
    }: AstBuildParams,
) -> Ast {
    fn add_initial_hidden_root_var(variables: &mut Variables, register: Register) {
        variables.mk_root_reg_var(register, 0, VariableVisibility::Hidden);
    }
    let mut variables = Variables::new();

    add_initial_hidden_root_var(&mut variables, Register::Gpr(Gpr::STACK_POINTER));
    add_initial_hidden_root_var(&mut variables, Register::Spr(Spr::Lr));

    for reg in 14..=31 {
        // Callee saved registers are hidden
        add_initial_hidden_root_var(&mut variables, Register::Gpr(Gpr(reg)));
    }

    // Infer parameters
    // This needs to happen before we begin to build the AST, since this makes variables for the parameters

    let mut params = Vec::new();
    let mut end_of_params = false;
    for reg in 3..=8 {
        let register = Register::Gpr(Gpr(reg));
        let uses = def_use_map.uses_of(register, 0);
        if uses.is_empty() {
            // No uses of this register with generation=0. No parameter.
            end_of_params = true;
        } else {
            // TODO: this might actually be reachable if the function just doesn't use the second parameter but uses the third one.
            assert!(!end_of_params);

            let var_id = variables.mk_root_reg_var(register, 0, VariableVisibility::Visible);

            params.push(Parameter {
                var_id,
                ty: Ty { kind: TyKind::U32 }, // TODO: figure out the type based on its uses?
            });
        }
    }

    let stmts = build_path(
        instructions,
        InstId(0),
        local_generations,
        analysis,
        &mut variables,
        def_use_map,
    );

    let function = Function {
        stmts,
        return_ty: Ty { kind: TyKind::Void }, // TODO
        params,
    };
    let items = vec![Item {
        kind: ItemKind::Function(function),
    }];

    Ast { items }
}
