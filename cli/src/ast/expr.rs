use crate::{ast::stmt::VarId, flow::ssa::RegisterWithGeneration};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expr {
    pub kind: ExprKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprKind {
    Var(VarId),
    Unary(UnaryExpr),
    Binary(BinaryExpr),
    Immediate32(i32),
    Immediate16(i16),
    FnCall(FnCallTarget, Vec<Expr>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FnCallTarget {
    Addr(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryExpr {
    pub op: BinaryOp,
    pub left: Box<Expr>,
    pub right: Box<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Lt,
    Gt,
    Eq,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnaryExpr {
    pub op: UnaryOp,
    pub operand: Box<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}
