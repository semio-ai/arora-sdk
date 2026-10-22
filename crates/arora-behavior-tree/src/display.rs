//! A text form of expressions and nodes, for traces, errors and tooling.
//!
//! The form reads like a Rust call, so a nested [`Expression::Call`] shows as
//! the dataflow it computes:
//!
//! ```text
//! {proximity} = below(
//!     x: clamp(x: mul(a: distance(), b: 100f64), lo: 0f64),
//!     limit: 30f64,
//! )
//! ```
//!
//! | Expression | Text |
//! |---|---|
//! | a literal | `1.5f64`, `"text"`, `true`, `()`, `f64[1.0, 2.0]`, `some(1u8)`, `none` |
//! | a uuid literal | `uuid(…)` |
//! | a variable | `{name}`, or `{<uuid>}` when its name is not known |
//! | an anonymous cell | `{= value}`: the value it holds now |
//! | another node's return value | `@node` |
//! | another node's argument | `@node.parameter` |
//! | a selection | `source[path]` |
//! | a call | `function(parameter: value, …)` |
//!
//! A call names its function and parameters from the [`Names`] it is shown
//! with. Without them, or for a function they do not have, it shows the ids:
//! `<module>::<function>(<parameter>: value)`. A module, function or
//! parameter that is computed rather than stated shows its expression in
//! parentheses: `(@<node>.<parameter>)`. A call to another module than the one
//! the index names for its function shows that module: `<module>::name(…)`.
//!
//! `{}` writes one line. `{:#}` breaks a call whose line would be longer than
//! [`WIDTH`] over several lines, one argument each.
//!
//! A node shows as the call it makes: `function(parameter: value, …)`, with
//! `{target} = ` in front when it binds its return (`_ret`), and its name and
//! a colon in front of that when [`Names::nodes`] names it. Its children are
//! not shown. Attached and detached nodes show alike:
//!
//! ```text
//! cm: clamp(x: @raw, lo: 0f64)
//! level: {face/level} = add(a: @cm, b: 1f64)
//! below(x: @cm, limit: 30f64)
//! ```
use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};

use arora_behavior::{FunctionIndex, ModuleFunction};
use arora_types::value::Value;
use uuid::Uuid;

use crate::schema::{CallExpression, Expression, Node, _ON_CHANGE_PARAM_ID, _RET_PARAM_ID};

/// The width past which `{:#}` breaks a call over several lines.
pub const WIDTH: usize = 60;

/// What a display shows in place of ids. Every field is optional: an id with
/// no name shows as itself.
#[derive(Default, Clone, Copy)]
pub struct Names<'a> {
    /// The functions, by id: their names and their parameters' names.
    pub functions: Option<&'a FunctionIndex>,
    /// The variables' names, by id.
    pub variables: Option<&'a HashMap<Uuid, String>>,
    /// The nodes' names, by id, for the nodes a reference or a listing names.
    pub nodes: Option<&'a HashMap<Uuid, String>>,
}

impl<'a> Names<'a> {
    /// Names from a function index alone.
    pub fn functions(functions: &'a FunctionIndex) -> Self {
        Self {
            functions: Some(functions),
            ..Self::default()
        }
    }

    fn node(&self, id: Uuid) -> String {
        self.nodes
            .and_then(|nodes| nodes.get(&id))
            .cloned()
            .unwrap_or_else(|| id.to_string())
    }

    /// A parameter's name, from whichever function has it: parameter ids
    /// are unique.
    fn any_parameter(&self, id: Uuid) -> String {
        self.functions
            .and_then(|functions| {
                functions
                    .values()
                    .find_map(|function| function.function.parameters.get(&id))
            })
            .map(|parameter| parameter.name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    fn function(&self, id: Uuid) -> Option<&'a ModuleFunction> {
        self.functions.and_then(|functions| functions.get(&id))
    }

    fn variable(&self, id: Uuid) -> String {
        self.variables
            .and_then(|variables| variables.get(&id))
            .cloned()
            .unwrap_or_else(|| id.to_string())
    }
}

/// An [`Expression`] shown with [`Names`]; see the [module docs](self).
pub struct ExpressionDisplay<'a> {
    expression: &'a Expression,
    names: Names<'a>,
}

/// A [`Node`] shown as the call it makes; see the [module docs](self).
pub struct NodeDisplay<'a> {
    node: &'a Node,
    names: Names<'a>,
}

impl Expression {
    /// This expression as text, with `names` in place of ids.
    pub fn display<'a>(&'a self, names: Names<'a>) -> ExpressionDisplay<'a> {
        ExpressionDisplay {
            expression: self,
            names,
        }
    }
}

impl Node {
    /// This node as the call it makes, with `names` in place of ids.
    pub fn display<'a>(&'a self, names: Names<'a>) -> NodeDisplay<'a> {
        NodeDisplay { node: self, names }
    }
}

impl Display for ExpressionDisplay<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&expression(self.expression, &self.names, f.alternate(), 0))
    }
}

/// With no names: every id shows as itself.
impl Display for Expression {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.display(Names::default()), f)
    }
}

impl Display for NodeDisplay<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let node = self.node;
        let signature = self.names.function(node.function);
        let label = match self.names.nodes.and_then(|nodes| nodes.get(&node.id)) {
            Some(name) => format!("{name}: "),
            None => String::new(),
        };
        let target = match node.arguments.get(&_RET_PARAM_ID) {
            Some(target) => format!("{label}{} = ", expression(target, &self.names, false, 0)),
            None => label,
        };
        let head = match signature {
            Some(signature) => signature.function_name.clone(),
            None if node.function == crate::nodes::WITH_FUNCTION_ID => "WITH".into(),
            None => node.function.to_string(),
        };
        let arguments: Vec<(String, &Expression)> = ordered(&node.arguments, signature)
            .into_iter()
            .filter(|(parameter, _)| *parameter != _RET_PARAM_ID)
            .map(|(parameter, argument)| (parameter_name(signature, parameter), argument))
            .collect();
        f.write_str(&target)?;
        f.write_str(&call_text(
            &head,
            &arguments,
            &self.names,
            f.alternate(),
            0,
            target.len(),
        ))
    }
}

/// A node's arguments in its function's parameter order, then any others by
/// id: a node holds them in a map.
pub(crate) fn ordered<'e>(
    arguments: &'e HashMap<Uuid, Expression>,
    signature: Option<&ModuleFunction>,
) -> Vec<(Uuid, &'e Expression)> {
    let order: Vec<Uuid> = signature
        .map(|signature| signature.function.parameter_ordering.clone())
        .unwrap_or_default();
    let mut ordered: Vec<(Uuid, &Expression)> = arguments
        .iter()
        .map(|(parameter, argument)| (*parameter, argument))
        .collect();
    ordered.sort_by_key(|(parameter, _)| {
        (
            order
                .iter()
                .position(|id| id == parameter)
                .unwrap_or(usize::MAX),
            *parameter,
        )
    });
    ordered
}

fn parameter_name(signature: Option<&ModuleFunction>, parameter: Uuid) -> String {
    if parameter == _ON_CHANGE_PARAM_ID {
        return "_on_change".to_string();
    }
    signature
        .and_then(|signature| signature.function.parameters.get(&parameter))
        .map(|parameter| parameter.name.clone())
        .unwrap_or_else(|| parameter.to_string())
}

/// The id an expression states, when it states one: a uuid literal, or the
/// bytes of one. A computed id is `None`.
pub(crate) fn stated_id(expression: &Expression) -> Option<Uuid> {
    match expression {
        Expression::Uuid(id) => Some(*id),
        Expression::Value(Value::ArrayU8(bytes)) => Uuid::from_slice(bytes).ok(),
        Expression::Value(Value::Uuid(id)) => Some(*id),
        _ => None,
    }
}

fn expression(expression: &Expression, names: &Names, pretty: bool, indent: usize) -> String {
    match expression {
        Expression::Value(value) => literal(value),
        Expression::Uuid(id) => format!("uuid({id})"),
        Expression::Variable(cell) => match cell.get() {
            Some(value) => format!("{{= {}}}", literal(&value)),
            None => "{=}".to_string(),
        },
        Expression::VariableId(id) => format!("{{{}}}", names.variable(*id)),
        Expression::NodeArgument(port) if port.parameter == _RET_PARAM_ID => {
            format!("@{}", names.node(port.node))
        }
        Expression::NodeArgument(port) => format!(
            "@{}.{}",
            names.node(port.node),
            names.any_parameter(port.parameter)
        ),
        Expression::Select { source, path } => {
            format!(
                "{}[{path}]",
                self::expression(source, names, pretty, indent)
            )
        }
        Expression::Call(call) => call_expression(call, names, pretty, indent),
    }
}

/// A part of a call that names an id: the id's name when it states one,
/// else the expression that computes it, in parentheses.
fn id_text(expression: &Expression, names: &Names, name: impl Fn(Uuid) -> String) -> String {
    match stated_id(expression) {
        Some(id) => name(id),
        None => format!("({})", self::expression(expression, names, false, 0)),
    }
}

fn call_expression(call: &CallExpression, names: &Names, pretty: bool, indent: usize) -> String {
    let signature = stated_id(&call.function).and_then(|id| names.function(id));
    let module = stated_id(&call.module);
    let head = match signature {
        Some(signature) if module == Some(signature.module_id) => signature.function_name.clone(),
        Some(signature) => format!(
            "{}::{}",
            id_text(&call.module, names, |id| id.to_string()),
            signature.function_name
        ),
        None => format!(
            "{}::{}",
            id_text(&call.module, names, |id| id.to_string()),
            id_text(&call.function, names, |id| id.to_string())
        ),
    };
    let arguments: Vec<(String, &Expression)> = call
        .arguments
        .iter()
        .map(|(parameter, argument)| {
            (
                id_text(parameter, names, |id| parameter_name(signature, id)),
                argument,
            )
        })
        .collect();
    call_text(&head, &arguments, names, pretty, indent, 0)
}

/// `head(name: argument, …)` on one line, or, when `pretty` and that line
/// would pass [`WIDTH`] from `indent` and `lead` characters already written,
/// one argument per line.
fn call_text(
    head: &str,
    arguments: &[(String, &Expression)],
    names: &Names,
    pretty: bool,
    indent: usize,
    lead: usize,
) -> String {
    let line = format!(
        "{head}({})",
        arguments
            .iter()
            .map(|(name, argument)| format!("{name}: {}", expression(argument, names, false, 0)))
            .collect::<Vec<_>>()
            .join(", ")
    );
    if !pretty || arguments.is_empty() || indent * 4 + lead + line.len() <= WIDTH {
        return line;
    }
    let pad = "    ".repeat(indent + 1);
    let mut text = format!("{head}(\n");
    for (name, argument) in arguments {
        text.push_str(&format!(
            "{pad}{name}: {},\n",
            expression(argument, names, true, indent + 1)
        ));
    }
    text.push_str(&"    ".repeat(indent));
    text.push(')');
    text
}

/// A value as a literal: scalars as [`Value`] shows them (`1.5f64`), strings
/// quoted and escaped, arrays as their element type and elements.
pub(crate) fn literal(value: &Value) -> String {
    fn list<T: Display>(elements: &[T]) -> String {
        elements
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
    match value {
        Value::String(text) => format!("{text:?}"),
        Value::Option(None) => "none".to_string(),
        Value::Option(Some(value)) => format!("some({})", literal(value)),
        Value::ArrayValue(values) => format!(
            "[{}]",
            values.iter().map(literal).collect::<Vec<_>>().join(", ")
        ),
        Value::ArrayString(texts) => format!(
            "[{}]",
            texts
                .iter()
                .map(|text| format!("{text:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::ArrayBoolean(elements) => format!("bool[{}]", list(elements)),
        Value::ArrayU8(elements) => format!("u8[{}]", list(elements)),
        Value::ArrayU16(elements) => format!("u16[{}]", list(elements)),
        Value::ArrayU32(elements) => format!("u32[{}]", list(elements)),
        Value::ArrayU64(elements) => format!("u64[{}]", list(elements)),
        Value::ArrayI8(elements) => format!("i8[{}]", list(elements)),
        Value::ArrayI16(elements) => format!("i16[{}]", list(elements)),
        Value::ArrayI32(elements) => format!("i32[{}]", list(elements)),
        Value::ArrayI64(elements) => format!("i64[{}]", list(elements)),
        Value::ArrayF32(elements) => format!("f32[{}]", list(elements)),
        Value::ArrayF64(elements) => format!("f64[{}]", list(elements)),
        other => other.to_string(),
    }
}
