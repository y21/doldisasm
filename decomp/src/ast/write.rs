use core::fmt;

use crate::{
    ast::{
        Ast,
        expr::{BinaryExpr, BinaryOp, Expr, ExprKind, FnCallTarget, UnaryExpr, UnaryOp},
        item::{Function, Item, ItemKind, Parameter},
        stmt::{Stmt, StmtKind, VarId},
        ty::{self, TyKind},
    },
    dataflow::variables::Variables,
};

pub trait Writer {
    fn write_str(&mut self, s: &str);
    fn write_fmt(&mut self, args: fmt::Arguments);
    fn with_scope(&mut self, f: &mut dyn FnMut(&mut dyn Writer));
    fn next_line(&mut self);
}

pub struct WriteContext<'a> {
    pub variables: &'a Variables,
}

pub struct StringWriter {
    buf: String,
    indentation: u32,
}

impl StringWriter {
    pub fn new() -> Self {
        Self {
            buf: String::new(),
            indentation: 0,
        }
    }

    pub fn into_string(self) -> String {
        self.buf
    }
}

impl Writer for StringWriter {
    fn write_str(&mut self, s: &str) {
        self.buf.push_str(s);
    }

    fn write_fmt(&mut self, args: fmt::Arguments) {
        use std::fmt::Write;
        self.buf.write_fmt(args).unwrap();
    }

    fn with_scope(&mut self, f: &mut dyn FnMut(&mut dyn Writer)) {
        self.indentation += 1;
        f(self);
        self.indentation -= 1;
    }

    fn next_line(&mut self) {
        self.buf.push_str("\n");
        for _ in 0..self.indentation {
            self.buf.push_str("    ");
        }
    }
}

fn write_var_id(var_id: VarId, cx: &WriteContext<'_>, writer: &mut dyn Writer) {
    if cx.variables.get(var_id).is_rsp() {
        writer.write_str("__RSP__");
    } else {
        writer.write_fmt(format_args!("v{}", var_id.0));
    }
}

fn write_expr(expr: &Expr, cx: &WriteContext<'_>, writer: &mut dyn Writer) {
    match expr.kind {
        ExprKind::Var(var_id) => {
            write_var_id(var_id, cx, writer);
        }
        ExprKind::Binary(BinaryExpr {
            ref left,
            op,
            ref right,
        }) => {
            write_expr(left, cx, writer);
            match op {
                BinaryOp::Add => writer.write_str(" + "),
                BinaryOp::Lt => writer.write_str(" < "),
                BinaryOp::Gt => writer.write_str(" > "),
                BinaryOp::Eq => writer.write_str(" == "),
                BinaryOp::Ne => writer.write_str(" != "),
            }
            write_expr(right, cx, writer);
        }
        ExprKind::Unary(UnaryExpr { op, ref operand }) => {
            match op {
                UnaryOp::Not => writer.write_str("!"),
            }
            write_expr(operand, cx, writer);
        }
        ExprKind::Immediate16(value) => writer.write_fmt(format_args!("{}", value)),
        ExprKind::FnCall(FnCallTarget::Addr(addr), ref args) => {
            writer.write_fmt(format_args!("{:#X}", addr));
            writer.write_str("(");
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    writer.write_str(", ");
                }
                write_expr(arg, cx, writer);
            }
            writer.write_str(")");
        }
        ExprKind::AddrOf(var) => {
            writer.write_str("&");
            write_var_id(var, cx, writer);
        }
    }
}

fn write_stmt(stmt: &Stmt, cx: &WriteContext<'_>, writer: &mut dyn Writer) {
    match stmt.kind {
        StmtKind::Assign {
            ref dest,
            ref value,
        } => {
            write_expr(dest, cx, writer);
            writer.write_str(" = ");
            write_expr(value, cx, writer);
            writer.write_str(";");
        }
        StmtKind::Return(ref expr) => {
            writer.write_str("return");
            if let Some(expr) = expr {
                writer.write_str(" ");
                write_expr(expr, cx, writer);
            }
            writer.write_str(";");
        }
        StmtKind::If {
            ref condition,
            ref then_stmts,
            ref else_stmts,
        } => {
            writer.write_str("if (");
            write_expr(&condition, cx, writer);
            writer.write_str(") {");

            writer.with_scope(&mut |writer| {
                for stmt in then_stmts.iter() {
                    writer.next_line();
                    write_stmt(stmt, cx, writer);
                }
            });
            writer.next_line();
            writer.write_str("}");

            if !else_stmts.is_empty() {
                writer.write_str(" else {");
                writer.with_scope(&mut |writer| {
                    for stmt in else_stmts.iter() {
                        writer.next_line();
                        write_stmt(stmt, cx, writer);
                    }
                });
                writer.next_line();
                writer.write_str("}");
            }
        }
    }
}

fn write_ty(ty: &ty::Ty, writer: &mut dyn Writer) {
    match ty.kind {
        TyKind::Void => writer.write_str("void"),
        TyKind::U32 => writer.write_str("u32"),
    }
}

fn write_function(
    Function {
        return_ty,
        params,
        stmts,
        name,
    }: &Function,
    cx: &WriteContext<'_>,
    writer: &mut dyn Writer,
) {
    write_ty(return_ty, writer);
    writer.write_str(" ");
    writer.write_str(name);
    writer.write_str("(");
    for (i, &Parameter { ref ty, var_id }) in params.iter().enumerate() {
        if i > 0 {
            writer.write_str(", ");
        }
        write_ty(ty, writer);
        writer.write_str(" ");
        write_var_id(var_id, cx, writer);
    }
    writer.write_str(") {");
    writer.with_scope(&mut |writer| {
        for stmt in stmts.iter() {
            writer.next_line();
            write_stmt(stmt, cx, writer);
        }
    });
    writer.next_line();
    writer.write_str("}");
}

fn write_item(item: &Item, cx: &WriteContext<'_>, writer: &mut dyn Writer) {
    match item.kind {
        ItemKind::Function(ref function) => write_function(function, cx, writer),
    }
}

pub fn write_ast(Ast { items }: &Ast, cx: &WriteContext<'_>, writer: &mut dyn Writer) {
    for item in items {
        write_item(item, cx, writer);
    }
}
