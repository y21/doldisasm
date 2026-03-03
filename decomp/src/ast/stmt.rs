use bitflags::bitflags;

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

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum VariableVisibility {
    Visible,
    Hidden,
}

bitflags! {
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub struct VariableFlags: u8 {
        const VISIBLE = 1 << 0;
        const RSP = 1 << 1;
    }
}

impl VariableFlags {
    pub fn from_vis(vis: VariableVisibility) -> Self {
        match vis {
            VariableVisibility::Visible => Self::VISIBLE,
            VariableVisibility::Hidden => Self::empty(),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Variable {
    flags: VariableFlags,
}
impl Variable {
    pub fn new(flags: VariableFlags) -> Self {
        Self { flags }
    }

    pub fn vis(&self) -> VariableVisibility {
        if self.flags.contains(VariableFlags::VISIBLE) {
            VariableVisibility::Visible
        } else {
            VariableVisibility::Hidden
        }
    }

    pub fn is_rsp(&self) -> bool {
        self.flags.contains(VariableFlags::RSP)
    }
}
