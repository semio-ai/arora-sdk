//! The formatter: a parsed text, in its canonical layout.
//!
//! - One line per node, its children indented two spaces under it; a node
//!   with no argument is written without parentheses.
//! - The root's tree, a blank line, then the other top-level nodes. A blank
//!   line written between two nodes is kept, one however many were written.
//! - A function's arguments in the order of its parameters, when the
//!   function is known; the others after them, as written.
//! - Literals as written: `1.5` stays `1.5`, `1.5f64` stays `1.5f64`.
//!   Strings are quoted and escaped as Rust's `Debug` does. A variable is
//!   written `{key}`, or quoted, `{"key"}`, when its key holds a character
//!   `{key}` does not.
//! - Comments where they were: above a node, or at the end of its line. A
//!   node whose arguments are followed by comments is written one argument
//!   per line, each argument's comments after it.
//!
//! A text with errors is refused: what does not parse is not in its
//! document, and formatting it would lose it.

use std::fmt::Write;

use arora_behavior::FunctionIndex;

use super::ast::{Argument, Call, Expr, ExprKind, Node, Span};
use super::parser::Parsed;
use super::resolve::{Found, Functions};
use super::{Diagnostic, Severity, MAX_DEPTH};

/// `parsed` as text, in its canonical layout. The arguments of the
/// functions `functions` and the natives know are written in their
/// parameters' order.
///
/// A text whose parse has errors is refused, with its errors: its document
/// does not hold what does not parse. So is a document nested deeper than
/// [`MAX_DEPTH`].
pub fn format(parsed: &Parsed, functions: &FunctionIndex) -> Result<String, Vec<Diagnostic>> {
    let errors: Vec<Diagnostic> = parsed
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .cloned()
        .collect();
    if !errors.is_empty() {
        return Err(errors);
    }
    let document = &parsed.document;
    let mut writer = Writer {
        text: String::new(),
        functions: Functions::new(functions),
    };
    for (index, node) in document.nodes.iter().enumerate() {
        if index == 1 || (index > 1 && node.blank_before) {
            writer.text.push('\n');
        }
        writer.node(node, 0).map_err(|span| vec![too_deep(span)])?;
    }
    if !document.comments.is_empty() && !document.nodes.is_empty() {
        writer.text.push('\n');
    }
    for comment in &document.comments {
        let _ = writeln!(writer.text, "#{}", comment.text);
    }
    Ok(writer.text)
}

fn too_deep(span: Span) -> Diagnostic {
    Diagnostic::error(
        format!("expressions and blocks nest at most {MAX_DEPTH} levels deep"),
        span,
    )
}

/// `{key}`, or `{"key"}` when `{key}` would not read back as `key`: a key
/// that is empty, holds a space, a brace, a quote or a line break, starts
/// with `=` or is an id.
pub(crate) fn variable(key: &str) -> String {
    let plain = !key.is_empty()
        && !key.starts_with('=')
        && !key
            .chars()
            .any(|char| char.is_whitespace() || matches!(char, '{' | '}' | '"'))
        && uuid::Uuid::parse_str(key).is_err();
    if plain {
        format!("{{{key}}}")
    } else {
        format!("{{{key:?}}}")
    }
}

/// The formatter's state: the text written so far.
struct Writer<'a> {
    text: String,
    functions: Functions<'a>,
}

/// Where a document nests too deep to write.
type Written = Result<(), Span>;

impl Writer<'_> {
    fn node(&mut self, node: &Node, depth: usize) -> Written {
        if depth >= MAX_DEPTH {
            return Err(node.span);
        }
        let indent = "  ".repeat(depth);
        for comment in &node.comments {
            let _ = writeln!(self.text, "{indent}#{}", comment.text);
        }
        self.text.push_str(&indent);
        if let Some(name) = &node.name {
            let _ = write!(self.text, "{}: ", name.text);
        }
        if let Some(target) = &node.target {
            self.expression(target, 0)?;
            self.text.push_str(" = ");
        }
        if node
            .call
            .arguments
            .iter()
            .any(|argument| !argument.comments.is_empty())
        {
            self.broken_call(&node.call, &indent)?;
        } else {
            self.call(&node.call, true, 0)?;
        }
        if let Some(comment) = &node.comment {
            let _ = write!(self.text, "  #{}", comment.text);
        }
        self.text.push('\n');
        for (index, child) in node.children.iter().enumerate() {
            if index > 0 && child.blank_before {
                self.text.push('\n');
            }
            self.node(child, depth + 1)?;
        }
        Ok(())
    }

    /// `function(` then one argument per line, each followed by its
    /// comments, then `)`.
    fn broken_call(&mut self, call: &Call, indent: &str) -> Written {
        self.function(call);
        self.text.push_str("(\n");
        for argument in ordered(call, &self.functions) {
            let _ = write!(self.text, "{indent}    {}: ", argument.name.text);
            self.expression(&argument.value, 1)?;
            self.text.push(',');
            for (index, comment) in argument.comments.iter().enumerate() {
                if index == 0 {
                    let _ = write!(self.text, "  #{}", comment.text);
                } else {
                    let _ = write!(self.text, "\n{indent}    #{}", comment.text);
                }
            }
            self.text.push('\n');
        }
        self.text.push_str(indent);
        self.text.push(')');
        Ok(())
    }

    fn function(&mut self, call: &Call) {
        if let Some(module) = &call.function.module {
            let _ = write!(self.text, "{}::", module.text);
        }
        self.text.push_str(&call.function.name.text);
    }

    /// `function(arguments)`; `function` alone when `bare` and there is no
    /// argument.
    fn call(&mut self, call: &Call, bare: bool, depth: usize) -> Written {
        self.function(call);
        if bare && call.arguments.is_empty() {
            return Ok(());
        }
        self.text.push('(');
        for (index, argument) in ordered(call, &self.functions).into_iter().enumerate() {
            if index > 0 {
                self.text.push_str(", ");
            }
            let _ = write!(self.text, "{}: ", argument.name.text);
            self.expression(&argument.value, depth + 1)?;
        }
        self.text.push(')');
        Ok(())
    }

    fn expression(&mut self, expression: &Expr, depth: usize) -> Written {
        if depth >= MAX_DEPTH {
            return Err(expression.span);
        }
        match &expression.kind {
            ExprKind::Number { text, suffix } => {
                self.text.push_str(text);
                self.text.push_str(suffix.as_deref().unwrap_or_default());
            }
            ExprKind::String { value } => {
                let _ = write!(self.text, "{value:?}");
            }
            ExprKind::Bool { value } => {
                let _ = write!(self.text, "{value}");
            }
            ExprKind::Unit => self.text.push_str("()"),
            ExprKind::None => self.text.push_str("none"),
            ExprKind::Some { value } => {
                self.text.push_str("some(");
                self.expression(value, depth + 1)?;
                self.text.push(')');
            }
            ExprKind::Array { element, elements } => {
                self.text.push_str(element.as_deref().unwrap_or_default());
                self.text.push('[');
                for (index, element) in elements.iter().enumerate() {
                    if index > 0 {
                        self.text.push_str(", ");
                    }
                    self.expression(element, depth + 1)?;
                }
                self.text.push(']');
            }
            ExprKind::Uuid { value } => {
                let _ = write!(self.text, "uuid({value})");
            }
            ExprKind::Json { text } => {
                let _ = write!(self.text, "json({text:?})");
            }
            ExprKind::Variable { name } => self.text.push_str(&variable(name)),
            ExprKind::VariableId { value } => {
                let _ = write!(self.text, "{{{value}}}");
            }
            ExprKind::Port { node, parameter } => {
                let _ = write!(self.text, "@{}", node.text);
                if let Some(parameter) = parameter {
                    let _ = write!(self.text, ".{}", parameter.text);
                }
            }
            ExprKind::Select { source, path } => {
                self.expression(source, depth + 1)?;
                let _ = write!(self.text, "[{path}]");
            }
            ExprKind::Call { call } => self.call(call, false, depth)?,
        }
        Ok(())
    }
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
