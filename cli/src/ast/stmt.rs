use crate::ast::expr::Expr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stmt {
    pub kind: StmtKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StmtKind {
    Assign {
        dest: Expr,
        value: Expr,
    },
    Return(Option<Expr>),
    If {
        condition: Expr,
        then_stmts: Vec<Stmt>,
        else_stmts: Vec<Stmt>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VarId(pub u32);

impl From<usize> for VarId {
    fn from(value: usize) -> Self {
        Self(value.try_into().unwrap())
    }
}

impl Into<usize> for VarId {
    fn into(self) -> usize {
        self.0 as usize
    }
}

pub struct Variable {}
