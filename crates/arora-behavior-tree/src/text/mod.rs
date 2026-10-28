//! The behavior text language: a behavior tree and its detached nodes as
//! text, the form [`display`](crate::display) writes, read back.
//!
//! ```text
//! fallback                                  # the first node is the root
//!   sequence
//!     equal(a: {person/visible}, b: true)
//!     below(x: @cm, limit: 30)
//!   succeed
//!
//! raw: mul(a: {sensor/distance}, b: 100)    # a node no node holds: detached
//! cm: clamp(x: @raw, lo: 0)
//! level: {face/level} = add(a: @cm, b: 1)   # its return value written to a key
//! ```
//!
//! - [`parse`] reads a text into a [`Document`], the syntax tree of what was
//!   written, every part with its [`Span`]. A line that does not parse is
//!   reported and left out; every other line is in the document.
//! - [`format()`] writes a document back as text, in its canonical layout.
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

pub(crate) use natives::native;

use serde::{Deserialize, Serialize};

/// A problem found in a text, and where.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub span: Span,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
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
