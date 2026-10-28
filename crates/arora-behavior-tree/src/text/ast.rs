//! The syntax tree of a behavior text: what was written, with where it was
//! written. Nothing here is resolved: names are names, and a literal keeps
//! its text.

use serde::{Deserialize, Serialize};

/// A range of the source text, in bytes: `start` included, `end` excluded.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// The span from the start of `self` to the end of `other`.
    pub(crate) fn to(self, other: Span) -> Span {
        Span::new(self.start, other.end.max(self.start))
    }
}

/// A behavior text: its top-level nodes, the first of which is the root, and
/// the comments after the last node.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Document {
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub comments: Vec<Comment>,
}

/// A node: `[name ':'] [target '='] function ['(' arguments ')']`, its
/// children indented under it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Node {
    /// The node's line, from its first token to its last.
    pub span: Span,
    /// The name it is referenced by, as `@name`.
    pub name: Option<Name>,
    /// Where its return value goes: `{key}` or `@node.parameter`.
    pub target: Option<Expr>,
    /// The function it calls, with its arguments.
    pub call: Call,
    pub children: Vec<Node>,
    /// The comment lines right above the node.
    #[serde(default)]
    pub comments: Vec<Comment>,
    /// The comment at the end of its line.
    #[serde(default)]
    pub comment: Option<Comment>,
    /// Whether a blank line separates it from what comes before it.
    #[serde(default)]
    pub blank_before: bool,
}

/// `function(parameter: value, …)`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Call {
    pub span: Span,
    pub function: FunctionName,
    pub arguments: Vec<Argument>,
}

/// A function: `name`, or `module::name` with the module's id.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct FunctionName {
    pub span: Span,
    pub module: Option<Name>,
    pub name: Name,
}

/// `parameter: value`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Argument {
    pub span: Span,
    pub name: Name,
    pub value: Expr,
    /// The comments after it, in a call broken over lines.
    #[serde(default)]
    pub comments: Vec<Comment>,
}

/// An identifier or an id, as written.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Name {
    pub span: Span,
    pub text: String,
}

/// A comment: `#` and the rest of its line. `text` follows the `#`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Comment {
    pub span: Span,
    pub text: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Expr {
    pub span: Span,
    pub kind: ExprKind,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExprKind {
    /// A number, its digits as written and its type suffix if it has one:
    /// `1.5`, `100u8`, `-3i64`.
    Number {
        text: String,
        suffix: Option<String>,
    },
    /// A string, unescaped.
    String {
        value: String,
    },
    Bool {
        value: bool,
    },
    /// `()`.
    Unit,
    /// `none`.
    None,
    /// `some(value)`.
    Some {
        value: Box<Expr>,
    },
    /// `[a, b]`, or `f64[a, b]` with its element type.
    Array {
        element: Option<String>,
        elements: Vec<Expr>,
    },
    /// `uuid(…)`.
    Uuid {
        value: uuid::Uuid,
    },
    /// `json("…")`: a value in its serde JSON form, for a value no other
    /// literal writes (a structure, an enumeration).
    Json {
        text: String,
    },
    /// `{name}` or `{"name"}`: a store key, or a tree-local variable when
    /// it starts with `.`.
    Variable {
        name: String,
    },
    /// `{<uuid>}`: the variable of that id, which names none.
    VariableId {
        value: uuid::Uuid,
    },
    /// `@node`, or `@node.parameter`.
    Port {
        node: Name,
        parameter: Option<Name>,
    },
    /// `source[path]`.
    Select {
        source: Box<Expr>,
        path: String,
    },
    /// `function(parameter: value, …)`.
    Call {
        call: Box<Call>,
    },
}
