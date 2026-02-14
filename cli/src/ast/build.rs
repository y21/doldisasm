use std::{
    collections::{HashMap, hash_map::Entry},
    ops::Deref,
};

use dataflow::{Dataflow, Results};
use ppc32::{
    Instruction,
    instruction::{BranchOptions, Gpr, Register, Spr, compute_branch_target},
};
use typed_index_collections::TiVec;

use crate::{
    ast::{
        Ast,
        expr::{BinaryExpr, BinaryOp, Expr, ExprKind, FnCallTarget},
        item::{Function, Item, ItemKind},
        stmt::{Stmt, StmtKind, VarId, Variable},
        ty::{Ty, TyKind},
    },
    flow::{
        InstId, InstructionsDeref,
        local_generation::{BlockState, LocalGenerationAnalysis, RegisterWithGeneration},
        ti_iter,
    },
};

pub struct AstBuildParams<'a, 'b> {
    pub instructions: &'a InstructionsDeref,
    pub local_generations: &'a Results<LocalGenerationAnalysis<'b>>,
    pub analysis: &'a LocalGenerationAnalysis<'a>,
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

    fn id_by_gpr(&mut self, reg: Gpr, state: &BlockState) -> VarId {
        let reg = RegisterWithGeneration {
            reg: Register::Gpr(reg),
            generation: state.registers.gprs[reg.0 as usize].generation,
        };

        *self
            .reg_to_var
            .entry(reg)
            .or_insert_with(|| self.list.push_and_get_key(Variable {}))
    }

    fn id_by_stack_mem(&mut self, offset: i16) -> VarId {
        let addr = StackRelativeAddress { offset };

        *self
            .mem_to_var
            .entry(addr)
            .or_insert_with(|| self.list.push_and_get_key(Variable {}))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct StackRelativeAddress {
    offset: i16,
}

struct Memory {
    map: HashMap<StackRelativeAddress, VarId>,
}

impl Memory {}

// fn gpr_to_local(gpr: Gpr, state: &BlockState) -> RegisterWithGeneration {
//     RegisterWithGeneration {
//         reg: Register::Gpr(gpr),
//         generation: state.registers.gprs[gpr.0 as usize].generation,
//     }
// }

// fn spr_to_local(spr: Spr, state: &BlockState) -> RegisterWithGeneration {
//     RegisterWithGeneration {
//         reg: Register::Spr(spr),
//         generation: match spr {
//             Spr::Xer => todo!(),
//             Spr::Lr => state.registers.sprs.lr.generation,
//             Spr::Ctr => state.registers.sprs.ctr.generation,
//             Spr::Msr => state.registers.sprs.msr.generation,
//             Spr::Pc => todo!(),
//             Spr::Other(_) => todo!(),
//         },
//     }
// }

fn build_path(
    instructions: &InstructionsDeref,
    idx: InstId,
    local_generations: &Results<LocalGenerationAnalysis<'_>>,
    analysis: &LocalGenerationAnalysis<'_>,
    variables: &mut Variables,
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
                if dest == Gpr::STACK_POINTER {
                    // Stack pointer update - not something we can meaningfully represent
                }
                analysis.apply_effect(&mut state, idx, instruction);
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

                    stmts.push(Stmt {
                        kind: StmtKind::Assign {
                            dest: Expr {
                                kind: ExprKind::Var(variables.id_by_gpr(dest, &state)),
                            },
                            value: Expr {
                                kind: ExprKind::Var(source),
                            },
                        },
                    });
                } else {
                    todo!()
                }
            }
            Instruction::Mfspr { dest, spr } => {
                if let Spr::Lr = spr {
                    // Probably nothing to do?
                } else {
                    todo!("{instruction:?}");
                }

                analysis.apply_effect(&mut state, idx, instruction);
            }
            Instruction::Addi { dest, source, imm } => {
                let source = variables.id_by_gpr(source, &state);

                analysis.apply_effect(&mut state, idx, instruction);

                stmts.push(Stmt {
                    kind: StmtKind::Assign {
                        dest: Expr {
                            kind: ExprKind::Var(variables.id_by_gpr(dest, &state)),
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
            Instruction::Stw { source, dest, imm } => {
                if dest == Gpr::STACK_POINTER {
                    // Writing to a stack-relative address - probably a write to a variable

                    let source = variables.id_by_gpr(source, &state);

                    analysis.apply_effect(&mut state, idx, instruction);

                    let dest: VarId = variables.id_by_stack_mem(imm.0);

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
                    todo!()
                }
            }
            Instruction::Branch { target, mode, link } => {
                let target = compute_branch_target(inst_addr.0, mode, target);
                if link {
                    // Function call. Probably.
                    analysis.apply_effect(&mut state, idx, instruction);

                    // TODO: check if r3 with this generation is used anywhere to tell if it even returns a value at all.
                    let return_var = variables.id_by_gpr(Gpr::RETURN, &state);
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

                let then_stmts = build_path(
                    instructions,
                    true_idx,
                    local_generations,
                    analysis,
                    variables,
                );
                let else_stmts = build_path(
                    instructions,
                    false_idx,
                    local_generations,
                    analysis,
                    variables,
                );

                analysis.apply_effect(&mut state, idx, instruction);
                stmts.push(Stmt {
                    kind: StmtKind::If {
                        condition: Expr {
                            kind: ExprKind::Immediate16(1), // TODO: figure out how to build conditions based on `bi` here...
                        },
                        then_stmts,
                        else_stmts,
                    },
                });
            }
            Instruction::Lwz { dest, source, imm } => {
                assert!(source != Gpr(0)); // TODO: source == 0 means no register and it loads from imm alone. handle this.

                if source == Gpr::STACK_POINTER {
                    // Stack-relative load

                    let source = variables.id_by_stack_mem(imm.0);

                    analysis.apply_effect(&mut state, idx, instruction);

                    let dest = variables.id_by_gpr(dest, &state);
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
                    // Probably nothing to do?
                } else {
                    todo!("{instruction:?}");
                }

                analysis.apply_effect(&mut state, idx, instruction);
            }
            Instruction::Bclr { bo, bi, link } => {
                assert!(!link);
                assert!(bo == BranchOptions::BranchAlways);

                analysis.apply_effect(&mut state, idx, instruction);

                // TODO: dont hardcode None, there may be a return value
                stmts.push(Stmt {
                    kind: StmtKind::Return(None),
                });
            }
            _ => todo!("{instruction:?}"),
        }
    }
    stmts
}

pub fn build(params: AstBuildParams) -> Ast {
    let mut variables = Variables::new();

    let stmts = build_path(
        params.instructions,
        InstId(0),
        params.local_generations,
        params.analysis,
        &mut variables,
    );

    let function = Function {
        stmts,
        return_ty: Ty { kind: TyKind::Void }, // TODO
        params: Vec::new(),                   // TODO
    };
    let items = vec![Item {
        kind: ItemKind::Function(function),
    }];

    Ast { items }
}
