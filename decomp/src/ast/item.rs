use crate::ast::{
    stmt::{Stmt, VarId},
    ty::Ty,
};

pub struct Item {
    pub kind: ItemKind,
}
pub enum ItemKind {
    Function(Function),
}

pub struct Function {
    pub name: String,
    pub return_ty: Ty,
    pub params: Vec<Parameter>,
    pub stmts: Vec<Stmt>,
}

pub struct Parameter {
    pub var_id: VarId,
    pub ty: Ty,
}
