use std::{collections::HashSet, convert::Infallible, ops::ControlFlow};

use ppc32::{
    Instruction,
    instruction::{
        BranchOptions, Crb, Crf, Gpr, Register, Spr, XerRegister, compute_branch_target,
        crb_from_index,
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
        loops::{LoopId, LoopMap},
        ssa::{BlockState, DefUseMap, Generation, LocalGenerationAnalysis},
        variables::{Variables, cr_bits_variables, xer_variables},
    },
    ti_utils::ti_iter,
};

pub struct AstBuildParams<'a, 'b> {
    pub fn_address: u32,
    pub instructions: &'a InstructionsDeref,
    pub local_generations: &'a Results<LocalGenerationAnalysis<'b>>,
    pub analysis: &'a LocalGenerationAnalysis<'b>,
    pub def_use_map: &'a DefUseMap,
    pub variables: &'a Variables,
    pub succs: &'a Successors<LocalGenerationAnalysis<'b>>,
    pub loops: &'a LoopMap,
}

struct BuildPathResult {
    stmts: Vec<Stmt>,
    has_return_value: bool,
    state: BlockState,
}

fn build_crf_assignments(
    state: &BlockState,
    def_use_map: &DefUseMap,
    variables: &Variables,
    stmts: &mut Vec<Stmt>,
    crf: Crf,
    left: Expr,
    right: Expr,
) {
    for (crb, vis) in cr_bits_variables(&state, def_use_map, crf) {
        let var = variables.id_by_reg(
            Register::Cr(crf, crb),
            state.registers.sprs.cr(crf, crb).generation,
        );

        if vis == VariableVisibility::Visible {
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
                            left: Box::new(left.clone()),
                            right: Box::new(right.clone()),
                        }),
                    },
                },
            });
        }
    }
}

fn build_xer_assignments(
    state: &BlockState,
    def_use_map: &DefUseMap,
    variables: &Variables,
    stmts: &mut Vec<Stmt>,
    source_a: Expr,
    source_b: Expr,
    dest: Expr,
) {
    for (xer, generation, _) in xer_variables(&state, def_use_map)
        .into_iter()
        .filter(|(.., vis)| *vis == VariableVisibility::Visible)
    {
        match xer {
            XerRegister::Ov => {
                // XER[OV]: ((source_a ^ source_b) & (source_a ^ result)) >> 31
                let ov = variables.id_by_reg(Register::Spr(Spr::Xer(XerRegister::Ov)), generation);
                stmts.push(Stmt {
                    kind: StmtKind::Assign {
                        dest: Expr::var(ov),
                        value: Expr {
                            kind: ExprKind::Binary(BinaryExpr {
                                op: BinaryOp::Rhs,
                                left: Box::new(Expr {
                                    kind: ExprKind::Binary(BinaryExpr {
                                        op: BinaryOp::BitAnd,
                                        left: Box::new(Expr {
                                            kind: ExprKind::Binary(BinaryExpr {
                                                op: BinaryOp::Xor,
                                                left: Box::new(source_a.clone()),
                                                right: Box::new(source_b.clone()),
                                            }),
                                        }),
                                        right: Box::new(Expr {
                                            kind: ExprKind::Binary(BinaryExpr {
                                                op: BinaryOp::Xor,
                                                left: Box::new(source_a.clone()),
                                                right: Box::new(dest.clone()),
                                            }),
                                        }),
                                    }),
                                }),
                                right: Box::new(Expr {
                                    kind: ExprKind::Immediate16(31),
                                }),
                            }),
                        },
                    },
                })
            }
            XerRegister::So => {
                // TODO: combine previous So + Ov
            }
            XerRegister::Ca => {
                // Unconditionally computed above.
            }
        }
    }
}

fn append_phi_merge_assignments(
    cur_state: &BlockState,
    next_state: &BlockState,
    variables: &Variables,
    stmts: &mut Vec<Stmt>,
) {
    cur_state
        .registers
        .register_iter()
        .zip(next_state.registers.register_iter())
        .for_each(|((cur_reg, cur_state), (next_reg, next_state))| {
            assert_eq!(cur_reg, next_reg);

            if cur_state.generation != next_state.generation
                // The next state variable may not exist, e.g. imagine a register is at gen 0 when entering the loop
                // (hasn't been assigned a value), and it writes a value to it in the loop body:
                // in that case the variable does not exist, but there also isn't a need
                // to insert an assignment to it anyway since we won't read its value.
                && let Some(next_var) =
                    variables.optional_id_by_reg(next_reg, next_state.generation)
            {
                let next_vis = variables.get_vis(next_var);

                if next_vis == VariableVisibility::Visible
                    && let Some(cur_var) =
                        variables.optional_id_by_reg(cur_reg, cur_state.generation)
                {
                    stmts.push(Stmt {
                        kind: StmtKind::Assign {
                            dest: Expr::var(next_var),
                            value: Expr::var(cur_var),
                        },
                    });
                }
            }
        });
}

fn build_path(
    instructions: &InstructionsDeref,
    start_index: InstId,
    end_index: Option<InstId>,
    local_generations: &Results<LocalGenerationAnalysis<'_>>,
    analysis: &LocalGenerationAnalysis<'_>,
    variables: &Variables,
    def_use_map: &DefUseMap,
    loops: &LoopMap,
    succs: &Successors<LocalGenerationAnalysis<'_>>,
    prev_state: Option<&BlockState>,
    current_loop: Option<LoopId>,
) -> BuildPathResult {
    if let Some(end_index) = end_index
        && start_index == end_index
    {
        return BuildPathResult {
            stmts: Vec::new(),
            has_return_value: false,
            state: prev_state.unwrap().clone(),
        };
    }

    if let Some((id, loop_)) = loops.find(start_index)
        && Some(id) != current_loop
    {
        // This is the start of a loop.
        let loop_result = build_path(
            instructions,
            start_index,
            loop_.common_merge_inst,
            local_generations,
            analysis,
            variables,
            def_use_map,
            loops,
            succs,
            prev_state,
            Some(id),
        );

        if let Some(common_merge_inst) = loop_.common_merge_inst {
            let mut result = build_path(
                instructions,
                common_merge_inst,
                end_index,
                local_generations,
                analysis,
                variables,
                def_use_map,
                loops,
                succs,
                prev_state,
                current_loop,
            );
            result.stmts.insert(
                0,
                Stmt {
                    kind: StmtKind::While {
                        condition: Expr {
                            kind: ExprKind::Immediate16(1),
                        },
                        body: loop_result.stmts,
                    },
                },
            );

            return BuildPathResult {
                stmts: result.stmts,
                has_return_value: loop_result.has_return_value || result.has_return_value,
                state: result.state,
            };
        } else {
            return loop_result;
        }
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

    for (idx, (inst_addr, instruction)) in ti_iter(&instructions[start_index..]) {
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
                loops,
                succs,
                Some(&state),
                current_loop,
            );

            stmts.extend(next_result.stmts);
            has_return_value |= next_result.has_return_value;

            return BuildPathResult {
                stmts,
                has_return_value,
                state,
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
            Instruction::Cmp {
                source_a,
                source_b,
                crf,
                l,
            } => {
                assert!(!l);
                let source_a = variables.id_by_gpr(source_a, &state);
                let source_b = variables.id_by_gpr(source_b, &state);

                analysis.apply_effect(&mut state, idx, instruction);

                build_crf_assignments(
                    &state,
                    def_use_map,
                    variables,
                    &mut stmts,
                    crf,
                    Expr::var(source_a),
                    Expr::var(source_b),
                );
            }
            Instruction::Cmpi { source, imm, crf } => {
                let source = variables.id_by_gpr(source, &state);

                analysis.apply_effect(&mut state, idx, instruction);

                build_crf_assignments(
                    &state,
                    def_use_map,
                    variables,
                    &mut stmts,
                    crf,
                    Expr::var(source),
                    Expr {
                        kind: ExprKind::Immediate16(imm.0 as i16),
                    },
                );
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
                    let visibility = variables.get_vis(dest);

                    if visibility == VariableVisibility::Visible {
                        stmts.push(Stmt {
                            kind: StmtKind::Assign {
                                dest: Expr::var(dest),
                                value: Expr::var(source),
                            },
                        });
                    }

                    if rc {
                        // Make sure code doesn't try to branch on a hidden variable. This could happen, but I'm not sure how to deal with that yet.
                        assert!(visibility == VariableVisibility::Visible);

                        build_crf_assignments(
                            &state,
                            def_use_map,
                            variables,
                            &mut stmts,
                            Crf(0),
                            Expr::var(dest),
                            Expr {
                                kind: ExprKind::Immediate16(0),
                            },
                        );
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
            Instruction::AddicRc { dest, source, simm } => {
                let source = variables.id_by_gpr(source, &state);

                analysis.apply_effect(&mut state, idx, instruction);

                let dest = variables.id_by_gpr(dest, &state);

                if variables.get_vis(dest) == VariableVisibility::Visible {
                    stmts.push(Stmt {
                        kind: StmtKind::Assign {
                            dest: Expr::var(dest),
                            value: Expr {
                                kind: ExprKind::Binary(BinaryExpr {
                                    op: BinaryOp::Add,
                                    left: Box::new(Expr::var(source)),
                                    right: Box::new(Expr {
                                        kind: ExprKind::Immediate16(simm),
                                    }),
                                }),
                            },
                        },
                    });
                }

                build_crf_assignments(
                    &state,
                    def_use_map,
                    variables,
                    &mut stmts,
                    Crf(0),
                    Expr::var(dest),
                    Expr {
                        kind: ExprKind::Immediate16(0),
                    },
                );

                build_xer_assignments(
                    &state,
                    def_use_map,
                    variables,
                    &mut stmts,
                    Expr::var(source),
                    Expr {
                        kind: ExprKind::Immediate16(0),
                    },
                    Expr::var(dest),
                );
            }
            Instruction::Addi { dest, source, imm } => {
                let source = if source == Gpr::ZERO {
                    ExprKind::Immediate16(imm.0)
                } else if source == Gpr::STACK_POINTER {
                    ExprKind::AddrOf(variables.id_by_stack_mem(imm.0))
                } else {
                    ExprKind::Binary(BinaryExpr {
                        op: BinaryOp::Add,
                        left: Box::new(Expr::var(variables.id_by_gpr(source, &state))),
                        right: Box::new(Expr {
                            kind: ExprKind::Immediate16(imm.0),
                        }),
                    })
                };

                analysis.apply_effect(&mut state, idx, instruction);

                let dest = variables.id_by_gpr(dest, &state);
                let visibility = variables.get_vis(dest);

                if visibility == VariableVisibility::Visible {
                    stmts.push(Stmt {
                        kind: StmtKind::Assign {
                            dest: Expr::var(dest),
                            value: Expr { kind: source },
                        },
                    });
                }
            }
            Instruction::Subfic { dest, source, simm } => {
                let source = variables.id_by_gpr(source, &state);
                analysis.apply_effect(&mut state, idx, instruction);
                let dest = variables.id_by_gpr(dest, &state);
                let ca = variables.id_by_reg(
                    Register::Spr(Spr::Xer(XerRegister::Ca)),
                    state.registers.sprs.xer.ca.generation,
                );
                let visibility = variables.get_vis(dest);
                if visibility == VariableVisibility::Visible {
                    stmts.push(Stmt {
                        kind: StmtKind::Assign {
                            dest: Expr::var(dest),
                            value: Expr {
                                kind: ExprKind::Binary(BinaryExpr {
                                    op: BinaryOp::Sub,
                                    left: Box::new(Expr {
                                        kind: ExprKind::Immediate16(simm),
                                    }),
                                    right: Box::new(Expr::var(source)),
                                }),
                            },
                        },
                    });

                    // XER[CA] bit: simm >= source
                    stmts.push(Stmt {
                        kind: StmtKind::Assign {
                            dest: Expr::var(ca),
                            value: Expr {
                                kind: ExprKind::Binary(BinaryExpr {
                                    op: BinaryOp::Ge,
                                    left: Box::new(Expr {
                                        kind: ExprKind::Immediate16(simm),
                                    }),
                                    right: Box::new(Expr::var(source)),
                                }),
                            },
                        },
                    });
                }
            }
            Instruction::Subf {
                dest,
                source_b,
                source_a,
                oe,
                rc,
            } => {
                let source_a = variables.id_by_gpr(source_a, &state);
                let source_b = variables.id_by_gpr(source_b, &state);

                analysis.apply_effect(&mut state, idx, instruction);

                let dest = variables.id_by_gpr(dest, &state);
                if variables.get_vis(dest) == VariableVisibility::Visible {
                    stmts.push(Stmt {
                        kind: StmtKind::Assign {
                            dest: Expr::var(dest),
                            value: Expr {
                                kind: ExprKind::Binary(BinaryExpr {
                                    op: BinaryOp::Sub,
                                    left: Box::new(Expr::var(source_a)),
                                    right: Box::new(Expr::var(source_b)),
                                }),
                            },
                        },
                    });
                }

                if rc {
                    build_crf_assignments(
                        &state,
                        def_use_map,
                        variables,
                        &mut stmts,
                        Crf(0),
                        Expr::var(dest),
                        Expr {
                            kind: ExprKind::Immediate16(0),
                        },
                    );
                }

                if oe {
                    build_xer_assignments(
                        &state,
                        def_use_map,
                        variables,
                        &mut stmts,
                        Expr::var(source_a),
                        Expr::var(source_b),
                        Expr::var(dest),
                    );
                }
            }
            Instruction::Subfe {
                dest,
                source_a,
                source_b,
                oe,
                rc,
            } => {
                // source_a - source_b - (1 - CA)
                let source_a = variables.id_by_gpr(source_a, &state);
                let source_b = variables.id_by_gpr(source_b, &state);
                let ca = variables.id_by_reg(
                    Register::Spr(Spr::Xer(XerRegister::Ca)),
                    state.registers.sprs.xer.ca.generation,
                );
                analysis.apply_effect(&mut state, idx, instruction);
                let dest = variables.id_by_gpr(dest, &state);
                if variables.get_vis(dest) == VariableVisibility::Visible {
                    // source_a - source_b
                    let src_sub = Expr {
                        kind: ExprKind::Binary(BinaryExpr {
                            op: BinaryOp::Sub,
                            left: Box::new(Expr::var(source_a)),
                            right: Box::new(Expr::var(source_b)),
                        }),
                    };
                    // 1 - CA
                    let ca_sub = Expr {
                        kind: ExprKind::Binary(BinaryExpr {
                            op: BinaryOp::Sub,
                            left: Box::new(Expr {
                                kind: ExprKind::Immediate16(1),
                            }),
                            right: Box::new(Expr::var(ca)),
                        }),
                    };
                    let expr = Expr {
                        kind: ExprKind::Binary(BinaryExpr {
                            op: BinaryOp::Sub,
                            left: Box::new(src_sub),
                            right: Box::new(ca_sub),
                        }),
                    };

                    stmts.push(Stmt {
                        kind: StmtKind::Assign {
                            dest: Expr::var(dest),
                            value: expr,
                        },
                    });
                }

                let ca_out = variables.id_by_reg(
                    Register::Spr(Spr::Xer(XerRegister::Ca)),
                    state.registers.sprs.xer.ca.generation,
                );

                if variables.get_vis(ca_out) == VariableVisibility::Visible {
                    // CA_out = lhs + CA_in > rhs
                    stmts.push(Stmt {
                        kind: StmtKind::Assign {
                            dest: Expr {
                                kind: ExprKind::Var(ca_out),
                            },
                            value: Expr {
                                kind: ExprKind::Binary(BinaryExpr {
                                    op: BinaryOp::Gt,
                                    left: Box::new(Expr {
                                        kind: ExprKind::Binary(BinaryExpr {
                                            op: BinaryOp::Add,
                                            left: Box::new(Expr::var(source_a)),
                                            right: Box::new(Expr::var(ca)),
                                        }),
                                    }),
                                    right: Box::new(Expr::var(source_b)),
                                }),
                            },
                        },
                    });
                }

                if oe {
                    build_xer_assignments(
                        &state,
                        def_use_map,
                        variables,
                        &mut stmts,
                        Expr::var(source_a),
                        Expr::var(source_b),
                        Expr::var(dest),
                    );
                }

                if rc {
                    build_crf_assignments(
                        &state,
                        def_use_map,
                        variables,
                        &mut stmts,
                        Crf(0),
                        Expr::var(dest),
                        Expr {
                            kind: ExprKind::Immediate16(0),
                        },
                    );
                }
            }
            Instruction::Andi { source, dest, simm } => {
                let source = variables.id_by_gpr(source, &state);
                analysis.apply_effect(&mut state, idx, instruction);
                let dest = variables.id_by_gpr(dest, &state);
                let vis = variables.get_vis(dest);
                if vis == VariableVisibility::Visible {
                    stmts.push(Stmt {
                        kind: StmtKind::Assign {
                            dest: Expr::var(dest),
                            value: Expr {
                                kind: ExprKind::Binary(BinaryExpr {
                                    op: BinaryOp::BitAnd,
                                    left: Box::new(Expr::var(source)),
                                    right: Box::new(Expr {
                                        kind: ExprKind::Immediate16(simm),
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

                    let dest: VarId = variables.id_by_stack_mem(imm.0);
                    let vis = variables.get_vis(dest);

                    // Don't create an assignment if this is just saving a callee-saved register
                    if vis == VariableVisibility::Visible {
                        stmts.push(Stmt {
                            kind: StmtKind::Assign {
                                dest: Expr::var(dest),
                                value: Expr::var(source),
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
                            arguments.push(Expr::var(var_id));
                        }
                    }

                    analysis.apply_effect(&mut state, idx, instruction);

                    // TODO: check if r3 with this generation is used anywhere to tell if it even returns a value at all.
                    let return_var = variables.id_by_gpr(Gpr::RETURN, &state);
                    stmts.push(Stmt {
                        kind: StmtKind::Assign {
                            dest: Expr::var(return_var),
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
                        loops,
                        succs,
                        Some(&state),
                        current_loop,
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

                let (true_loop_target, false_loop_target, true_is_break, false_is_break) =
                    if let Some(loop_id) = current_loop
                        && let current_loop = loops.get(loop_id)
                        && let true_is_start = current_loop.start == true_idx
                        && let false_is_start = current_loop.start == false_idx
                        && (true_is_start || false_is_start)
                    {
                        (
                            true_is_start.then_some(loop_id),
                            false_is_start.then_some(loop_id),
                            Some(true_idx) == current_loop.common_merge_inst,
                            Some(false_idx) == current_loop.common_merge_inst,
                        )
                    } else {
                        (None, None, false, false)
                    };

                // TODO: maybe compute this one for each entry in successors and then just use the map here?
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
                    state: then_state,
                } = if true_loop_target.is_some() {
                    let mut stmts = Vec::with_capacity(2);
                    append_phi_merge_assignments(
                        &state,
                        local_generations.get(true_idx).unwrap(),
                        variables,
                        &mut stmts,
                    );
                    stmts.push(Stmt {
                        kind: StmtKind::Continue,
                    });

                    BuildPathResult {
                        stmts,
                        has_return_value,
                        state: state.clone(),
                    }
                } else if true_is_break {
                    let mut stmts = Vec::with_capacity(2);
                    append_phi_merge_assignments(
                        &state,
                        local_generations.get(true_idx).unwrap(),
                        variables,
                        &mut stmts,
                    );
                    stmts.push(Stmt {
                        kind: StmtKind::Break,
                    });
                    BuildPathResult {
                        stmts,
                        has_return_value,
                        state: state.clone(),
                    }
                } else {
                    build_path(
                        instructions,
                        true_idx,
                        common_merge_inst,
                        local_generations,
                        analysis,
                        variables,
                        def_use_map,
                        loops,
                        succs,
                        Some(&state),
                        current_loop,
                    )
                };
                let BuildPathResult {
                    stmts: else_stmts,
                    has_return_value: else_has_return_value,
                    state: else_state,
                } = if false_loop_target.is_some() {
                    let mut stmts = Vec::with_capacity(2);
                    append_phi_merge_assignments(
                        &state,
                        local_generations.get(false_idx).unwrap(),
                        variables,
                        &mut stmts,
                    );
                    stmts.push(Stmt {
                        kind: StmtKind::Continue,
                    });
                    BuildPathResult {
                        stmts,
                        has_return_value,
                        state: state.clone(),
                    }
                } else if false_is_break {
                    let mut stmts = Vec::with_capacity(2);
                    append_phi_merge_assignments(
                        &state,
                        local_generations.get(false_idx).unwrap(),
                        variables,
                        &mut stmts,
                    );
                    stmts.push(Stmt {
                        kind: StmtKind::Break,
                    });
                    BuildPathResult {
                        stmts,
                        has_return_value,
                        state: state.clone(),
                    }
                } else {
                    build_path(
                        instructions,
                        false_idx,
                        common_merge_inst,
                        local_generations,
                        analysis,
                        variables,
                        def_use_map,
                        loops,
                        succs,
                        Some(&state),
                        current_loop,
                    )
                };

                has_return_value |= then_has_return_value | else_has_return_value;

                let mut condition = Expr::var(condition);
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

                has_return_value |= then_has_return_value | else_has_return_value;
                stmts.push(Stmt {
                    kind: StmtKind::If {
                        condition,
                        then_stmts: then_stmts,
                        else_stmts: else_stmts,
                    },
                });

                if let Some(common_merge_inst) = common_merge_inst {
                    let StmtKind::If {
                        then_stmts,
                        else_stmts,
                        ..
                    } = &mut stmts.last_mut().unwrap().kind
                    else {
                        unreachable!()
                    };

                    // TODO: currently we merge the phis when we process the next block,
                    // but we should really just do it in the places where we transfer to the next block
                    // (call the append_phi... function)
                    let next_state = local_generations.get(common_merge_inst).unwrap();
                    append_phi_merge_assignments(&then_state, next_state, variables, then_stmts);
                    append_phi_merge_assignments(&else_state, next_state, variables, else_stmts);

                    let next_path = build_path(
                        instructions,
                        common_merge_inst,
                        end_index,
                        local_generations,
                        analysis,
                        variables,
                        def_use_map,
                        loops,
                        succs,
                        Some(&state),
                        current_loop,
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
                    let vis = variables.get_vis(dest);
                    if vis == VariableVisibility::Visible {
                        stmts.push(Stmt {
                            kind: StmtKind::Assign {
                                dest: Expr::var(dest),
                                value: Expr::var(source),
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
                        Some(Expr::var(
                            variables.id_by_reg(return_reg, return_generation),
                        ))
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
                    let mut condition = Expr::var(var_id);
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
                        loops,
                        succs,
                        Some(&state),
                        current_loop,
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
        state,
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
        loops,
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
        state: _,
    } = build_path(
        instructions,
        InstId(0),
        None,
        local_generations,
        analysis,
        variables,
        def_use_map,
        loops,
        succs,
        None,
        None,
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
