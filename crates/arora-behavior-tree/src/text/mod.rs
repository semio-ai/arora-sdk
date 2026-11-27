//! The behavior text language: a behavior tree and its detached nodes as
//! text, the form [`display`](crate::display) writes, read back.
//!
//! ```text
//! fallback                                     # the first node is the root
//!   sequence
//!     equal(a: {person/visible}, b: true)
//!     below(x: @cm._ret, limit: 30)
//!   succeed
//!
//! raw: mul(a: {sensor/distance}, b: 100)       # a node no node holds: detached
//! cm: clamp(x: @raw._ret, lo: 0)
//! level: {face/level} = add(a: @cm._ret, b: 1)  # its return value written to a key
//! ```
//!
//! - [`parse`] reads a text into a [`Document`], the syntax tree of what was
//!   written, every part with its [`Span`]. A line that does not parse is
//!   reported and left out; every other line is in the document.
//! - [`format()`] writes a parsed text back in its canonical layout. It
//!   refuses a text with errors: the lines left out of its document would be
//!   lost.
//! - [`resolve`] turns a document into the shared
//!   [`Graph`](arora_behavior::graph::Graph) an interpreter runs, against the
//!   device's function index and the [`natives`].
//! - [`load`] does both, refusing a text with an error.
//!
//! The language is described in `docs/text.md`.

mod ast;
mod format;
mod lexer;
mod natives;
mod parser;
mod resolve;
#[cfg(test)]
mod tests;

pub use ast::{Argument, Call, Comment, Document, Expr, ExprKind, FunctionName, Name, Node, Span};
pub use format::format;
pub use natives::natives;
pub use parser::{parse, Parsed};
pub use resolve::{resolve, Loaded, LoadedNode};

pub(crate) use format::variable;
pub(crate) use lexer::{is_name, is_parameter_name};
pub(crate) use natives::{native, native_named, seed};
pub(crate) use resolve::{node_id, number, Functions, RETURN};

use arora_behavior::graph::Graph;
use arora_behavior::FunctionIndex;
use serde::{Deserialize, Serialize};

use crate::error::BehaviorTreeError;

/// The [`Graph`] a text says, its functions found in `functions`: [`parse`]
/// then [`resolve`]. A text with an error is refused with every error, each
/// at its `line:column`; warnings do not stop it. A column counts
/// characters.
pub fn load(source: &str, functions: &FunctionIndex) -> Result<Graph, BehaviorTreeError> {
    let parsed = parse(source);
    let loaded = resolve(&parsed.document, functions);
    let mut errors = parsed
        .diagnostics
        .iter()
        .chain(&loaded.diagnostics)
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .peekable();
    if errors.peek().is_none() {
        return Ok(loaded.graph);
    }
    // Where each line starts, found once: a line ends at `\n`, `\r\n` or
    // `\r`.
    let starts: Vec<usize> = std::iter::once(0)
        .chain(source.char_indices().filter_map(|(at, char)| match char {
            '\n' => Some(at + 1),
            '\r' if !source[at + 1..].starts_with('\n') => Some(at + 1),
            _ => None,
        }))
        .collect();
    let errors: Vec<String> = errors
        .map(|diagnostic| {
            let offset = diagnostic.span.start.min(source.len());
            let line = starts.partition_point(|start| *start <= offset);
            let column = source[starts[line - 1]..offset].chars().count() + 1;
            format!("{line}:{column}: {}", diagnostic.message)
        })
        .collect();
    Err(BehaviorTreeError::ParsingError {
        message: errors.join("; "),
    })
}

/// How deep expressions nest, and blocks: a call in an argument, a value in
/// `some(…)` or in an array, and a selection out of a value are each a
/// level, and so is each block of children. A text nested deeper is
/// reported, and no reader goes deeper.
pub const MAX_DEPTH: usize = 128;

/// A problem found in a text, and where.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub span: Span,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Severity {
    /// The text does not say what to run.
    Error,
    /// The text runs, though likely not as meant.
    Warning,
}

impl Diagnostic {
    pub(crate) fn error(message: String, span: Span) -> Self {
        Self {
            severity: Severity::Error,
            message,
            span,
        }
    }

    pub(crate) fn warning(message: String, span: Span) -> Self {
        Self {
            severity: Severity::Warning,
            message,
            span,
        }
    }
}
