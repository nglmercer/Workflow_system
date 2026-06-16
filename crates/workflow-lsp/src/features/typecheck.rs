//! Type-checking: walks the parsed program and produces diagnostics for
//! argument-type mismatches at call sites. Annotation-mismatch
//! detection is a placeholder for now.
//!
//! This module is self-contained — it depends only on the public
//! `inference` API and `workflow-parser` types.

use crate::features::{Diagnostic, DiagnosticSeverity};
use crate::inference;

/// Check for type mismatches in the program and return diagnostics.
pub fn check_type_mismatches(source: &str, inference: &inference::Inference) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Parse the source to get the AST
    let program = match workflow_parser::FlowParser::parse_flow_program(source) {
        Ok(p) => p,
        Err(_) => return diagnostics, // Parse errors are handled elsewhere
    };

    // Check global variables for type mismatches
    for global in &program.globals {
        check_expr(&global.value, source, inference, &mut diagnostics);
    }

    // Check function calls for type mismatches
    for workflow in &program.workflows {
        check_stmts(&workflow.body, source, inference, &mut diagnostics);
    }
    for func in &program.functions {
        check_stmts(&func.body, source, inference, &mut diagnostics);
    }

    // Check annotation vs inferred type mismatches
    check_annotation_mismatches(source, &program, inference, &mut diagnostics);

    diagnostics
}

/// Recursively check statements for type mismatches.
fn check_stmts(
    stmts: &[workflow_parser::ast::Stmt],
    source: &str,
    inference: &inference::Inference,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for stmt in stmts {
        match stmt {
            workflow_parser::ast::Stmt::VarDecl {
                value: Some(expr), ..
            } => {
                check_expr(expr, source, inference, diagnostics);
            }
            workflow_parser::ast::Stmt::If {
                condition,
                then_body,
                else_body,
            } => {
                check_expr(condition, source, inference, diagnostics);
                check_stmts(then_body, source, inference, diagnostics);
                if let Some(else_stmts) = else_body {
                    check_stmts(else_stmts, source, inference, diagnostics);
                }
            }
            workflow_parser::ast::Stmt::Return { value: Some(expr) } => {
                check_expr(expr, source, inference, diagnostics);
            }
            workflow_parser::ast::Stmt::Expr(expr) | workflow_parser::ast::Stmt::Log(expr) => {
                check_expr(expr, source, inference, diagnostics);
            }
            workflow_parser::ast::Stmt::Foreach { iterable, body, .. } => {
                check_expr(iterable, source, inference, diagnostics);
                check_stmts(body, source, inference, diagnostics);
            }
            _ => {}
        }
    }
}

/// Recursively check expressions for type mismatches.
fn check_expr(
    expr: &workflow_parser::ast::Expr,
    source: &str,
    inference: &inference::Inference,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match expr {
        workflow_parser::ast::Expr::Call { name, args } => {
            // Check each argument
            for arg in args {
                check_expr(arg, source, inference, diagnostics);
            }

            // Check if argument types match function signature
            if let Some(sig) = inference.functions.get(name) {
                for (i, arg) in args.iter().enumerate() {
                    if let Some(param_type) = sig.param_types.get(i) {
                        if let Some(arg_type) = infer_expr_type(arg, inference) {
                            if !types_compatible(param_type, &arg_type) {
                                // Find the argument's position in source
                                if let Some((start_line, start_col, end_line, end_col)) =
                                    find_expr_range(source, arg)
                                {
                                    diagnostics.push(Diagnostic {
                                        start_line,
                                        start_col,
                                        end_line,
                                        end_col,
                                        message: format!(
                                            "Type mismatch: expected `{}`, got `{}`",
                                            param_type.label(),
                                            arg_type.label()
                                        ),
                                        severity: DiagnosticSeverity::Warning,
                                        source: Some("type-checker".to_string()),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        workflow_parser::ast::Expr::BinaryOp { left, right, .. } => {
            check_expr(left, source, inference, diagnostics);
            check_expr(right, source, inference, diagnostics);
        }
        workflow_parser::ast::Expr::UnaryOp { operand, .. } => {
            check_expr(operand, source, inference, diagnostics);
        }
        workflow_parser::ast::Expr::Member { object, .. } => {
            check_expr(object, source, inference, diagnostics);
        }
        workflow_parser::ast::Expr::Array(elements) => {
            for elem in elements {
                check_expr(elem, source, inference, diagnostics);
            }
        }
        workflow_parser::ast::Expr::InterpolatedString(parts) => {
            for part in parts {
                if let workflow_parser::ast::InterpPart::Expr(e) = part {
                    check_expr(e, source, inference, diagnostics);
                }
            }
        }
        _ => {}
    }
}

/// Infer the type of an expression.
fn infer_expr_type(
    expr: &workflow_parser::ast::Expr,
    inference: &inference::Inference,
) -> Option<inference::Type> {
    match expr {
        workflow_parser::ast::Expr::String(_) => Some(inference::Type::String),
        workflow_parser::ast::Expr::Number(_) => Some(inference::Type::Number),
        workflow_parser::ast::Expr::Bool(_) => Some(inference::Type::Bool),
        workflow_parser::ast::Expr::Null => Some(inference::Type::Null),
        workflow_parser::ast::Expr::Var(name) => {
            // Look up in inference scope
            inference
                .scope_at
                .iter()
                .flatten()
                .find(|b| b.name == *name)
                .map(|b| b.ty.clone())
        }
        workflow_parser::ast::Expr::Call { name, .. } => {
            inference.functions.get(name).map(|sig| sig.ret.clone())
        }
        workflow_parser::ast::Expr::Array(elements) => {
            if let Some(first) = elements.first() {
                infer_expr_type(first, inference).map(|t| inference::Type::Array(Box::new(t)))
            } else {
                Some(inference::Type::Array(Box::new(inference::Type::Any)))
            }
        }
        _ => Some(inference::Type::Any),
    }
}

/// Check if two types are compatible (equal or one is Any).
fn types_compatible(expected: &inference::Type, actual: &inference::Type) -> bool {
    if matches!(expected, inference::Type::Any) || matches!(actual, inference::Type::Any) {
        return true;
    }
    match (expected, actual) {
        (inference::Type::String, inference::Type::String) => true,
        (inference::Type::Number, inference::Type::Number) => true,
        (inference::Type::Bool, inference::Type::Bool) => true,
        (inference::Type::Null, inference::Type::Null) => true,
        (inference::Type::Array(a), inference::Type::Array(b)) => types_compatible(a, b),
        _ => false,
    }
}

/// Find the source range of an expression (best-effort using line offsets).
fn find_expr_range(
    source: &str,
    expr: &workflow_parser::ast::Expr,
) -> Option<(u32, u32, u32, u32)> {
    // Simple heuristic: find the expression text in the source
    let expr_text = expr_to_text(expr);
    let lines: Vec<&str> = source.lines().collect();

    for (line_idx, line) in lines.iter().enumerate() {
        if let Some(col) = line.find(&expr_text) {
            let start_col = col as u32;
            let end_col = (col + expr_text.len()) as u32;
            return Some((line_idx as u32, start_col, line_idx as u32, end_col));
        }
    }
    None
}

/// Convert an expression to its approximate text representation.
fn expr_to_text(expr: &workflow_parser::ast::Expr) -> String {
    match expr {
        workflow_parser::ast::Expr::String(s) => format!("\"{}\"", s),
        workflow_parser::ast::Expr::Number(n) => format!("{}", n),
        workflow_parser::ast::Expr::Bool(b) => format!("{}", b),
        workflow_parser::ast::Expr::Null => "null".to_string(),
        workflow_parser::ast::Expr::Var(name) => name.clone(),
        workflow_parser::ast::Expr::Call { name, args } => {
            let arg_strs: Vec<String> = args.iter().map(expr_to_text).collect();
            format!("{}({})", name, arg_strs.join(", "))
        }
        workflow_parser::ast::Expr::Member { object, property } => {
            format!("{}.{}", expr_to_text(object), property)
        }
        workflow_parser::ast::Expr::BinaryOp { op, left, right } => {
            let op_str = match op {
                workflow_parser::ast::BinaryOp::Add => "+",
                workflow_parser::ast::BinaryOp::Sub => "-",
                workflow_parser::ast::BinaryOp::Mul => "*",
                workflow_parser::ast::BinaryOp::Div => "/",
                workflow_parser::ast::BinaryOp::Eq => "==",
                workflow_parser::ast::BinaryOp::Neq => "!=",
                _ => "?",
            };
            format!("{} {} {}", expr_to_text(left), op_str, expr_to_text(right))
        }
        workflow_parser::ast::Expr::Array(elems) => {
            let elem_strs: Vec<String> = elems.iter().map(expr_to_text).collect();
            format!("[{}]", elem_strs.join(", "))
        }
        _ => "...".to_string(),
    }
}

/// Check for annotation vs inferred type mismatches.
fn check_annotation_mismatches(
    _source: &str,
    _program: &workflow_parser::ast::FlowProgram,
    _inference: &inference::Inference,
    _diagnostics: &mut Vec<Diagnostic>,
) {
    // Future: check if annotations match inferred types
}
