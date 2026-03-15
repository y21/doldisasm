use crate::ast::stmt::VarId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expr {
    pub kind: ExprKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprKind {
    Var(VarId),
    AddrOf(VarId),
    Unary(UnaryExpr),
    Binary(BinaryExpr),
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
    Sub,
    Xor,
    Rhs,
    Lt,
    Gt,
    Ge,
    Eq,
    Ne,
    BitAnd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnaryExpr {
    pub op: UnaryOp,
    pub operand: Box<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
}
