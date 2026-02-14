use crate::ast::{stmt::Stmt, ty::Ty};

pub struct Item {
    pub kind: ItemKind,
}
pub enum ItemKind {
    Function(Function),
}

pub struct Function {
    pub return_ty: Ty,
    pub params: Vec<Parameter>,
    pub stmts: Vec<Stmt>,
}

pub struct Parameter {
    pub ty: Ty,
}
