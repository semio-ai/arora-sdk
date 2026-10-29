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
pub(crate) use natives::{native, native_named};
pub(crate) use resolve::{node_id, number, Functions, RETURN};

use serde::{Deserialize, Serialize};

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
