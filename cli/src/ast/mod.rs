use crate::ast::item::Item;

pub mod build;
pub mod expr;
pub mod item;
pub mod stmt;
pub mod ty;
pub mod write;

pub use build::build;

pub struct Ast {
    pub items: Vec<Item>,
}
