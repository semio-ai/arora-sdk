//! The formatter: a [`Document`] as text, in its canonical layout.
//!
//! - One line per node, its children indented two spaces under it; a node
//!   with no argument is written without parentheses.
//! - The root's tree, a blank line, then the other top-level nodes. A blank
//!   line written between two nodes is kept, one however many were written.
//! - A function's arguments in the order of its parameters, when the
//!   function is known; the others after them, as written.
//! - Literals as written: `1.5` stays `1.5`, `1.5f64` stays `1.5f64`.
//!   Strings are quoted and escaped as Rust's `Debug` does.
//! - Comments where they were: above a node, or at the end of its line.

use std::fmt::Write;

use arora_behavior::FunctionIndex;

use super::ast::{Argument, Call, Document, Expr, ExprKind, Node};
use super::resolve::{Found, Functions};

/// `document` as text. The arguments of the functions `functions` and the
/// natives know are written in their parameters' order.
pub fn format(document: &Document, functions: &FunctionIndex) -> String {
    let functions = Functions::new(functions);
    let mut text = String::new();
    for (index, node) in document.nodes.iter().enumerate() {
        if index == 1 || (index > 1 && node.blank_before) {
            text.push('\n');
        }
        write_node(&mut text, node, 0, &functions);
    }
    if !document.comments.is_empty() && !document.nodes.is_empty() {
        text.push('\n');
    }
    for comment in &document.comments {
        let _ = writeln!(text, "#{}", comment.text);
    }
    text
}

fn write_node(text: &mut String, node: &Node, depth: usize, functions: &Functions) {
    let indent = "  ".repeat(depth);
    for comment in &node.comments {
        let _ = writeln!(text, "{indent}#{}", comment.text);
    }
    text.push_str(&indent);
    if let Some(name) = &node.name {
        let _ = write!(text, "{}: ", name.text);
    }
    if let Some(target) = &node.target {
        write_expression(text, target, functions);
        text.push_str(" = ");
    }
    write_call(text, &node.call, functions, true);
    if let Some(comment) = &node.comment {
        let _ = write!(text, "  #{}", comment.text);
    }
    text.push('\n');
    for (index, child) in node.children.iter().enumerate() {
        if index > 0 && child.blank_before {
            text.push('\n');
        }
        write_node(text, child, depth + 1, functions);
    }
}

/// `function(arguments)`; `function` alone when `bare` and there is no
/// argument.
fn write_call(text: &mut String, call: &Call, functions: &Functions, bare: bool) {
    if let Some(module) = &call.function.module {
        let _ = write!(text, "{}::", module.text);
    }
    text.push_str(&call.function.name.text);
    if bare && call.arguments.is_empty() {
        return;
    }
    text.push('(');
    for (index, argument) in ordered(call, functions).into_iter().enumerate() {
        if index > 0 {
            text.push_str(", ");
        }
        let _ = write!(text, "{}: ", argument.name.text);
        write_expression(text, &argument.value, functions);
    }
    text.push(')');
}

/// `call`'s arguments in its function's parameter order, when the function
/// is known; the others after them, in the order written.
fn ordered<'c>(call: &'c Call, functions: &Functions) -> Vec<&'c Argument> {
    let mut arguments: Vec<&Argument> = call.arguments.iter().collect();
    let Ok(Found::Known(function)) = functions.find(&call.function) else {
        return arguments;
    };
    let signature = &function.function;
    arguments.sort_by_cached_key(|argument| {
        signature
            .parameter_ordering
            .iter()
            .position(|id| {
                signature
                    .parameters
                    .get(id)
                    .is_some_and(|parameter| parameter.name == argument.name.text)
                    || id.to_string() == argument.name.text
            })
            .unwrap_or(usize::MAX)
    });
    arguments
}

fn write_expression(text: &mut String, expression: &Expr, functions: &Functions) {
    match &expression.kind {
        ExprKind::Number {
            text: digits,
            suffix,
        } => {
            text.push_str(digits);
            text.push_str(suffix.as_deref().unwrap_or_default());
        }
        ExprKind::String { value } => {
            let _ = write!(text, "{value:?}");
        }
        ExprKind::Bool { value } => {
            let _ = write!(text, "{value}");
        }
        ExprKind::Unit => text.push_str("()"),
        ExprKind::None => text.push_str("none"),
        ExprKind::Some { value } => {
            text.push_str("some(");
            write_expression(text, value, functions);
            text.push(')');
        }
        ExprKind::Array { element, elements } => {
            text.push_str(element.as_deref().unwrap_or_default());
            text.push('[');
            for (index, element) in elements.iter().enumerate() {
                if index > 0 {
                    text.push_str(", ");
                }
                write_expression(text, element, functions);
            }
            text.push(']');
        }
        ExprKind::Uuid { value } => {
            let _ = write!(text, "uuid({value})");
        }
        ExprKind::Json { text: json } => {
            let _ = write!(text, "json({json:?})");
        }
        ExprKind::Variable { name } => {
            let _ = write!(text, "{{{name}}}");
        }
        ExprKind::Port { node, parameter } => {
            let _ = write!(text, "@{}", node.text);
            if let Some(parameter) = parameter {
                let _ = write!(text, ".{}", parameter.text);
            }
        }
        ExprKind::Select { source, path } => {
            write_expression(text, source, functions);
            let _ = write!(text, "[{path}]");
        }
        ExprKind::Call { call } => write_call(text, call, functions, false),
    }
}
