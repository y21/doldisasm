use std::{collections::HashSet, convert::Infallible, ops::ControlFlow};

use ppc32::{
    Instruction,
    instruction::{
        BranchOptions, Crb, Crf, Gpr, Register, Spr, compute_branch_target, crb_from_index,
    },
};

use crate::{
    ast::{
        Ast,
        expr::{BinaryExpr, BinaryOp, Expr, ExprKind, FnCallTarget, UnaryExpr, UnaryOp},
        item::{Function, Item, ItemKind, Parameter},
        stmt::{Stmt, StmtKind, VarId, VariableVisibility},
        ty::{Ty, TyKind},
    },
    dataflow::{
        InstId, InstructionsDeref,
        core::{Dataflow, Results, Successors, for_each_transitive_successor},
        ssa::{BlockState, DefUseMap, Generation, LocalGenerationAnalysis},
        ti_iter,
        variables::{Variables, cr_bits_need_variable},
    },
};

pub struct AstBuildParams<'a, 'b> {
    pub fn_address: u32,
    pub instructions: &'a InstructionsDeref,
    pub local_generations: &'a Results<LocalGenerationAnalysis<'b>>,
    pub analysis: &'a LocalGenerationAnalysis<'b>,
    pub def_use_map: &'a DefUseMap,
    pub variables: &'a Variables,
    pub succs: &'a Successors<LocalGenerationAnalysis<'b>>,
}

struct BuildPathResult {
    stmts: Vec<Stmt>,
    has_return_value: bool,
}

fn build_path(
    instructions: &InstructionsDeref,
    start_index: InstId,
    end_index: Option<InstId>,
    local_generations: &Results<LocalGenerationAnalysis<'_>>,
    analysis: &LocalGenerationAnalysis<'_>,
    variables: &Variables,
    def_use_map: &DefUseMap,
    succs: &Successors<LocalGenerationAnalysis<'_>>,
) -> BuildPathResult {
    if let Some(end_index) = end_index
        && start_index == end_index
    {
        return BuildPathResult {
            stmts: Vec::new(),
            has_return_value: false,
        };
    }

    let mut state = local_generations.get(start_index).map_or_else(
        || {
            assert_eq!(start_index, InstId(0));
            BlockState::default()
        },
        Clone::clone,
    );

    let mut stmts = Vec::new();
    let mut has_return_value = false;

    let block_instructions = &instructions[start_index..];

    for (idx, (inst_addr, instruction)) in ti_iter(block_instructions) {
        let absolute_index = InstId(start_index.0 + idx.0);
        if absolute_index != start_index && local_generations.get(absolute_index).is_some() {
            let next_result = build_path(
                instructions,
                absolute_index,
                end_index,
                local_generations,
                analysis,
                variables,
                def_use_map,
                succs,
            );

            stmts.extend(next_result.stmts);
            has_return_value |= next_result.has_return_value;

            return BuildPathResult {
                stmts,
                has_return_value,
            };
        }
        match *instruction {
            Instruction::Stwu {
                source,
                dest,
                imm: _,
            } => {
                assert!(source == Gpr::STACK_POINTER && dest == Gpr::STACK_POINTER);

                analysis.apply_effect(&mut state, idx, instruction);
            }
            Instruction::Cmpi { source, imm, crf } => {
                let source = variables.id_by_gpr(source, &state);

                analysis.apply_effect(&mut state, idx, instruction);

                for crb in cr_bits_need_variable(&state, def_use_map, crf) {
                    let var = variables.id_by_reg(
                        Register::Cr(crf, crb),
                        state.registers.sprs.cr(crf, crb).generation,
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
                                        kind: ExprKind::Var(source),
                                    }),
                                    right: Box::new(Expr {
                                        // TODO: cast to signed (i16)
                                        kind: ExprKind::Immediate16(imm.0 as i16),
                                    }),
                                }),
                            },
                        },
                    });
                }
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

                    let dest = variables.id_by_gpr(dest, &state);
                    let visibility = variables.get(dest).vis();

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
                        for crb in cr_bits_need_variable(&state, def_use_map, crf) {
                            let var = variables.id_by_reg(
                                Register::Cr(crf, crb),
                                state.registers.sprs.cr(crf, crb).generation,
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
                } else {
                    // TODO: once we implement this, most of the above also applies here (Rc=1).
                    todo!()
                }
            }
            Instruction::Mfspr { dest: _, spr } => {
                if let Spr::Lr = spr {
                    // Probably nothing to do?
                    analysis.apply_effect(&mut state, idx, instruction);
                } else {
                    todo!("{instruction:?}"); // TODO: make sure to have apply_effect here too
                }
            }
            Instruction::Addi {
                dest: Gpr::STACK_POINTER,
                source: Gpr::STACK_POINTER,
                imm: _,
            } => {
                // Just adjusting the stack pointer.
                analysis.apply_effect(&mut state, idx, instruction);
            }
            Instruction::Addi { dest, source, imm } => {
                let source = if source == Gpr::ZERO {
                    ExprKind::Immediate16(imm.0)
                } else if source == Gpr::STACK_POINTER {
                    ExprKind::AddrOf(variables.id_by_stack_mem(imm.0))
                } else {
                    ExprKind::Binary(BinaryExpr {
                        op: BinaryOp::Add,
                        left: Box::new(Expr {
                            kind: ExprKind::Var(variables.id_by_gpr(source, &state)),
                        }),
                        right: Box::new(Expr {
                            kind: ExprKind::Immediate16(imm.0),
                        }),
                    })
                };

                analysis.apply_effect(&mut state, idx, instruction);

                let dest = variables.id_by_gpr(dest, &state);
                let visibility = variables.get(dest).vis();

                if visibility == VariableVisibility::Visible {
                    stmts.push(Stmt {
                        kind: StmtKind::Assign {
                            dest: Expr {
                                kind: ExprKind::Var(dest),
                            },
                            value: Expr { kind: source },
                        },
                    });
                }
            }
            Instruction::Stw { source, dest, imm } => {
                if dest == Gpr::STACK_POINTER {
                    // Writing to a stack-relative address - probably a write to a variable

                    let source = variables.id_by_gpr(source, &state);

                    analysis.apply_effect(&mut state, idx, instruction);

                    let dest: VarId = variables.id_by_stack_mem(imm.0);
                    let vis = variables.get(dest).vis();

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

                    // Figure out arguments.
                    let mut arguments = Vec::new();
                    for reg in 3..=8 {
                        let register = Register::Gpr(Gpr(reg));
                        let generation = state.registers.gprs[reg as usize].generation;
                        if let Some(var_id) = variables.optional_id_by_reg(register, generation)
                            && !def_use_map.has_uses(register, generation)
                        {
                            arguments.push(Expr {
                                kind: ExprKind::Var(var_id),
                            });
                        }
                    }

                    analysis.apply_effect(&mut state, idx, instruction);

                    // TODO: check if r3 with this generation is used anywhere to tell if it even returns a value at all.
                    let return_var = variables.id_by_gpr(Gpr::RETURN, &state);
                    stmts.push(Stmt {
                        kind: StmtKind::Assign {
                            dest: Expr {
                                kind: ExprKind::Var(return_var),
                            },
                            value: Expr {
                                kind: ExprKind::FnCall(FnCallTarget::Addr(target), arguments),
                            },
                        },
                    });
                } else {
                    analysis.apply_effect(&mut state, idx, instruction);

                    let idx = InstId((target - analysis.fn_address) / 4);
                    let path_result = build_path(
                        instructions,
                        idx,
                        end_index,
                        local_generations,
                        analysis,
                        variables,
                        def_use_map,
                        succs,
                    );
                    stmts.extend(path_result.stmts);
                    has_return_value |= path_result.has_return_value;
                    break;
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
                let false_idx = InstId(absolute_index.0 + 1);

                let mut true_transitive_successors = HashSet::new();
                for_each_transitive_successor(succs, true_idx, &mut |inst| {
                    true_transitive_successors.insert(inst);
                    ControlFlow::<Infallible>::Continue(())
                });

                // The "common merge instruction" (i.e. the instruction that they both "meet" at) is where the paths will stop.
                let common_merge_inst =
                    for_each_transitive_successor(succs, false_idx, &mut |inst| {
                        if true_transitive_successors.contains(&inst) {
                            ControlFlow::Break(inst)
                        } else {
                            ControlFlow::Continue(())
                        }
                    })
                    .break_value();

                let (crf, crb) = crb_from_index(bi);
                let generation = state.registers.sprs.cr(crf, crb).generation;
                let condition = variables.id_by_reg(Register::Cr(crf, crb), generation);

                analysis.apply_effect(&mut state, idx, instruction);

                let BuildPathResult {
                    stmts: then_stmts,
                    has_return_value: then_has_return_value,
                } = build_path(
                    instructions,
                    true_idx,
                    common_merge_inst,
                    local_generations,
                    analysis,
                    variables,
                    def_use_map,
                    succs,
                );
                let BuildPathResult {
                    stmts: else_stmts,
                    has_return_value: else_has_return_value,
                } = build_path(
                    instructions,
                    false_idx,
                    common_merge_inst,
                    local_generations,
                    analysis,
                    variables,
                    def_use_map,
                    succs,
                );

                has_return_value |= then_has_return_value | else_has_return_value;

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
                        then_stmts: then_stmts,
                        else_stmts: else_stmts,
                    },
                });

                if let Some(common_merge_inst) = common_merge_inst {
                    let next_path = build_path(
                        instructions,
                        common_merge_inst,
                        end_index,
                        local_generations,
                        analysis,
                        variables,
                        def_use_map,
                        succs,
                    );
                    stmts.extend(next_path.stmts);
                    has_return_value |= next_path.has_return_value;
                }
                break;
            }
            Instruction::Lwz { dest, source, imm } => {
                assert!(source != Gpr(0)); // TODO: source == 0 means no register and it loads from imm alone. handle this.

                if source == Gpr::STACK_POINTER {
                    // Stack-relative load

                    let source = variables.id_by_stack_mem(imm.0);

                    analysis.apply_effect(&mut state, idx, instruction);

                    let dest = variables.id_by_gpr(dest, &state);
                    let vis = variables.get(dest).vis();
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
                    todo!();
                }
            }
            Instruction::Mtspr { source: _, spr } => {
                if let Spr::Lr = spr {
                    analysis.apply_effect(&mut state, idx, instruction);
                } else {
                    todo!("{instruction:?}"); // Make sure to add apply_effect here too
                }
            }
            Instruction::Bclr { bo, bi, link } => {
                assert!(!link);

                analysis.apply_effect(&mut state, idx, instruction);

                let return_reg = Register::Gpr(Gpr::RETURN);
                let return_generation = state.registers.gprs[Gpr::RETURN.0 as usize].generation;
                // We assume there is a return value if
                // 1.) The r3 generation is greater than 0 at this point, meaning that r3 has been assigned a value
                // 2.) That generation's value is not used anywhere, so the only logical reason for it to be assigned a value is to return it.
                let cur_has_return_value = return_generation > Generation::INITIAL
                    && !def_use_map.has_uses(return_reg, return_generation);
                has_return_value |= cur_has_return_value;

                let return_stmt = Stmt {
                    kind: StmtKind::Return(if cur_has_return_value {
                        Some(Expr {
                            kind: ExprKind::Var(variables.id_by_reg(return_reg, return_generation)),
                        })
                    } else {
                        None
                    }),
                };

                if bo == BranchOptions::BranchAlways {
                    stmts.push(return_stmt);
                } else {
                    let (crf, crb) = crb_from_index(bi);
                    let var_id = variables.id_by_reg(
                        Register::Cr(crf, crb),
                        state.registers.sprs.cr(crf, crb).generation,
                    );
                    let mut condition = Expr {
                        kind: ExprKind::Var(var_id),
                    };
                    if let BranchOptions::BranchIfFalse = bo {
                        condition = Expr {
                            kind: ExprKind::Unary(UnaryExpr {
                                op: UnaryOp::Not,
                                operand: Box::new(condition),
                            }),
                        };
                    }

                    stmts.push(Stmt {
                        kind: StmtKind::If {
                            condition,
                            then_stmts: vec![return_stmt],
                            else_stmts: Vec::new(),
                        },
                    });

                    let next_path = build_path(
                        instructions,
                        InstId(absolute_index.0 + 1),
                        end_index,
                        local_generations,
                        analysis,
                        variables,
                        def_use_map,
                        succs,
                    );
                    stmts.extend(next_path.stmts);
                    has_return_value |= next_path.has_return_value;
                }
                break;
            }
            _ => todo!("{instruction:?}"),
        }
    }

    BuildPathResult {
        stmts,
        has_return_value,
    }
}

pub fn build(
    AstBuildParams {
        instructions,
        local_generations,
        analysis,
        def_use_map,
        fn_address,
        variables,
        succs,
    }: AstBuildParams,
) -> Ast {
    // Infer parameters

    let mut params = Vec::new();
    let mut end_of_params = false;
    for reg in 3..=8 {
        let register = Register::Gpr(Gpr(reg));
        let uses = def_use_map.uses_of(register, Generation::INITIAL);
        if uses.is_empty() {
            // No uses of this register with generation=0. No parameter.
            end_of_params = true;
        } else {
            // TODO: this might actually be reachable if the function just doesn't use the second parameter but uses the third one.
            assert!(!end_of_params);

            let var_id = variables.id_by_reg(register, Generation::INITIAL);

            params.push(Parameter {
                var_id,
                ty: Ty { kind: TyKind::U32 }, // TODO: figure out the type based on its uses?
            });
        }
    }

    let BuildPathResult {
        stmts,
        has_return_value,
    } = build_path(
        instructions,
        InstId(0),
        None,
        local_generations,
        analysis,
        variables,
        def_use_map,
        succs,
    );

    let function = Function {
        name: format!("{fn_address:#X}"),
        return_ty: if has_return_value {
            Ty { kind: TyKind::U32 } // TODO: figure out the type based on its uses?
        } else {
            Ty { kind: TyKind::Void }
        },
        params,
        stmts,
    };
    let items = vec![Item {
        kind: ItemKind::Function(function),
    }];

    Ast { items }
}
