//! A text form of expressions, nodes and trees, for traces, errors and
//! tooling: the [behavior text language](crate::text), so what a display
//! writes, [`text::parse`](crate::text::parse) reads.
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
//! | a literal | `1.5f64`, `"text"`, `true`, `()`, `f64[1.0, 2.0]`, `str["a"]`, `[1u8, "a"]`, `some(1u8)`, `none` |
//! | a uuid literal | `uuid(…)` |
//! | a structure, an enumeration | `json("…")`: the value's serde JSON |
//! | a variable | `{name}`, or `{<uuid>}` when its name is not known |
//! | an anonymous cell | `{= value}`: the value it holds now |
//! | another node's return value | `@node._ret` |
//! | another node's argument | `@node.parameter` |
//! | a selection | `source[path]` |
//! | a call | `function(parameter: value, …)` |
//!
//! An anonymous cell, and a module, function or parameter computed rather
//! than stated, are shown only: a text does not write them.
//!
//! A call names its function and parameters from the [`Names`] it is shown
//! with, and a native node by its own name (`sequence`, `equal`, …; see
//! [`text::natives`](crate::text::natives)). Without names, or for a function
//! they do not have, it shows the ids: `<module>::<function>(<parameter>:
//! value)`. A module, function or parameter that is computed rather than
//! stated shows its expression in parentheses: `(@<node>.<parameter>)`. A
//! call to another module than the one the index names for its function
//! shows that module: `<module>::name(…)`.
//!
//! `{}` writes one line. `{:#}` breaks a call whose line would be longer than
//! [`WIDTH`] over several lines, one argument each.
//!
//! A node shows as the call it makes: `function(parameter: value, …)`, or
//! `function` alone when it has no argument, with `{target} = ` in front when
//! it binds its return (`_ret`), and its name and a colon in front of that
//! when [`Names::nodes`] names it. Its children are not shown. Attached and
//! detached nodes show alike:
//!
//! ```text
//! cm: clamp(x: @raw._ret, lo: 0f64)
//! level: {face/level} = add(a: @cm._ret, b: 1f64)
//! below(x: @cm._ret, limit: 30f64)
//! ```
//!
//! A [`BehaviorTree`] shows as a text: its root and the nodes under it, each
//! indented two spaces under its parent, then a blank line, then the nodes no
//! node holds: the detached ones in the order the tree lists them, then any
//! other. Each node is on one line. A node that another node reads, or that
//! the tree does not hold, is named: by its name in [`Names::nodes`], else by
//! its id, so the text read back gives every node the id it has. An
//! anonymous cell shows as the value it holds, `()` when it holds none: a
//! call's literal argument is such a cell once the tree is loaded.
use std::collections::{HashMap, HashSet};
use std::fmt::{self, Display, Formatter};
use std::rc::Rc;

use arora_behavior::{FunctionIndex, ModuleFunction};
use arora_types::value::Value;
use uuid::Uuid;

use crate::schema::{CallExpression, Expression, Node, _RET_PARAM_ID};
use crate::text::{is_name, is_parameter_name};
use crate::BehaviorTree;

/// The width past which `{:#}` breaks a call over several lines.
pub const WIDTH: usize = 60;

/// What a display shows in place of ids. Every field is optional: an id with
/// no name shows as itself.
#[derive(Default, Clone, Copy)]
pub struct Names<'a> {
    /// The functions, by id: their names and their parameters' names.
    pub functions: Option<&'a FunctionIndex>,
    /// The variables' names, by id. A tree names a variable they do not name
    /// as it was loaded with it: a tree built from a graph, by the key the
    /// graph binds.
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

    /// A node's name, when it is written as a name as it is; its id
    /// otherwise.
    fn node(&self, id: Uuid) -> String {
        self.label(id).unwrap_or_else(|| id.to_string())
    }

    /// A node's name, when [`Names::nodes`] has one that is written as a
    /// name as it is.
    fn label(&self, id: Uuid) -> Option<String> {
        self.nodes
            .and_then(|nodes| nodes.get(&id))
            .filter(|name| is_name(name))
            .cloned()
    }

    /// The text naming a function: its name, `<module>::name` when the name
    /// alone names a native or another module's function, or its id when its
    /// name is not written as a name as it is, or names another function of
    /// its module too.
    pub(crate) fn head(&self, function: &ModuleFunction) -> String {
        let name = &function.function_name;
        if crate::text::native(function.function_id).is_some() {
            return name.clone();
        }
        let others: Vec<&ModuleFunction> = self
            .functions
            .into_iter()
            .flat_map(|functions| functions.values())
            .filter(|other| {
                other.function_name == *name && other.function_id != function.function_id
            })
            .collect();
        if !is_name(name)
            || others
                .iter()
                .any(|other| other.module_id == function.module_id)
        {
            function.function_id.to_string()
        } else if !others.is_empty() || crate::text::native_named(name).is_some() {
            format!("{}::{name}", function.module_id)
        } else {
            name.clone()
        }
    }

    /// A parameter's name, from whichever function has it: parameter ids
    /// are unique.
    fn any_parameter(&self, id: Uuid) -> String {
        let natives = crate::text::natives().map(|native| -> &'a ModuleFunction { native });
        natives
            .chain(
                self.functions
                    .into_iter()
                    .flat_map(|functions| functions.values()),
            )
            .find_map(|function| function.function.parameters.get(&id))
            .map(|parameter| parameter.name.clone())
            .filter(|name| is_parameter_name(name))
            .unwrap_or_else(|| id.to_string())
    }

    /// A function's signature: a native's, or the index's.
    fn function(&self, id: Uuid) -> Option<&'a ModuleFunction> {
        crate::text::native(id)
            .map(|native| &native.function)
            .or_else(|| self.functions.and_then(|functions| functions.get(&id)))
    }

    /// A variable: `{key}`, quoted when the key needs it, or `{<uuid>}`
    /// when it has no name.
    fn variable(&self, id: Uuid) -> String {
        match self.variables.and_then(|variables| variables.get(&id)) {
            Some(key) => crate::text::variable(key),
            None => format!("{{{id}}}"),
        }
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
        let form = Form {
            pretty: f.alternate(),
            file: false,
        };
        f.write_str(&expression(self.expression, &self.names, form, 0))
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
        let form = Form {
            pretty: f.alternate(),
            file: false,
        };
        f.write_str(&node_text(self.node, &self.names, form))
    }
}

/// How a display writes an expression.
#[derive(Clone, Copy)]
struct Form {
    /// Whether a long call is broken over lines.
    pretty: bool,
    /// Whether the text is one to read back, which writes an anonymous cell
    /// as the value it holds.
    file: bool,
}

impl Form {
    fn line(self) -> Self {
        Self {
            pretty: false,
            ..self
        }
    }
}

/// `node` as the call it makes; see the [module docs](self).
fn node_text(node: &Node, names: &Names, form: Form) -> String {
    let signature = names.function(node.function);
    let label = match names.nodes.and_then(|nodes| nodes.get(&node.id)) {
        Some(_) => format!("{}: ", names.node(node.id)),
        None => String::new(),
    };
    let target = match node.arguments.get(&_RET_PARAM_ID) {
        Some(target) => format!("{label}{} = ", expression(target, names, form.line(), 0)),
        None => label,
    };
    let head = match signature {
        Some(signature) => names.head(signature),
        None => node.function.to_string(),
    };
    let arguments: Vec<(String, &Expression)> = ordered(&node.arguments, signature)
        .into_iter()
        .filter(|(parameter, _)| *parameter != _RET_PARAM_ID)
        .map(|(parameter, argument)| (parameter_name(signature, parameter), argument))
        .collect();
    if arguments.is_empty() {
        return format!("{target}{head}");
    }
    let call = call_text(&head, &arguments, names, form, 0, target.len());
    format!("{target}{call}")
}

/// A [`BehaviorTree`] as the text language writes it; see the
/// [module docs](self).
pub struct TreeDisplay<'a> {
    tree: &'a BehaviorTree,
    names: Names<'a>,
}

impl BehaviorTree {
    /// The tree as text, the form [`text::parse`](crate::text::parse) reads,
    /// with `names` in place of ids.
    pub fn display<'a>(&'a self, names: Names<'a>) -> TreeDisplay<'a> {
        TreeDisplay { tree: self, names }
    }
}

impl Display for TreeDisplay<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let tree = self.tree;
        let mut shown = HashSet::new();
        let mut attached = Vec::new();
        subtree(tree, &tree.root, 0, &mut shown, &mut attached);
        let attached_ids = shown.clone();
        let mut others = Vec::new();
        for node in tree.detached.nodes() {
            subtree(tree, node, 0, &mut shown, &mut others);
        }
        let mut loose: Vec<&Rc<Node>> = tree
            .node_index
            .values()
            .filter(|node| !shown.contains(&node.id))
            .collect();
        loose.sort_by_key(|node| node.id);
        for node in loose {
            subtree(tree, node, 0, &mut shown, &mut others);
        }

        // A node another node reads, or one the tree does not hold, is
        // named, by its id when `names` has no name for it, so that a text
        // reading it back gives it the same id.
        let mut referenced = HashSet::new();
        for node in tree.node_index.values() {
            for argument in node.arguments.values() {
                referenced_nodes(argument, &mut referenced);
            }
        }
        let mut labels = self.names.nodes.cloned().unwrap_or_default();
        for id in tree.node_index.keys() {
            if !labels.contains_key(id) && (referenced.contains(id) || !attached_ids.contains(id)) {
                labels.insert(*id, id.to_string());
            }
        }
        // A variable the names do not name keeps the name the tree was
        // loaded with, so a text read back binds the same key.
        let mut variables = tree.variable_names.clone();
        if let Some(named) = self.names.variables {
            variables.extend(named.iter().map(|(id, name)| (*id, name.clone())));
        }
        let names = Names {
            nodes: Some(&labels),
            variables: Some(&variables),
            ..self.names
        };
        for (depth, node) in &attached {
            writeln!(
                f,
                "{}{}",
                "  ".repeat(*depth),
                node_text(node, &names, FILE)
            )?;
        }
        if !others.is_empty() {
            writeln!(f)?;
        }
        for (depth, node) in &others {
            writeln!(
                f,
                "{}{}",
                "  ".repeat(*depth),
                node_text(node, &names, FILE)
            )?;
        }
        Ok(())
    }
}

/// The form of a tree's text: one line per node, each anonymous cell as the
/// value it holds.
const FILE: Form = Form {
    pretty: false,
    file: true,
};

/// `node` and the nodes under it not yet `shown`, each with its depth, in
/// the order the text writes them.
fn subtree<'t>(
    tree: &'t BehaviorTree,
    node: &'t Rc<Node>,
    depth: usize,
    shown: &mut HashSet<Uuid>,
    lines: &mut Vec<(usize, &'t Rc<Node>)>,
) {
    let mut stack = vec![(depth, node)];
    while let Some((depth, node)) = stack.pop() {
        if !shown.insert(node.id) {
            continue;
        }
        lines.push((depth, node));
        for child in node.children.iter().flatten().rev() {
            if let Some(child) = tree.node_index.get(child) {
                stack.push((depth + 1, child));
            }
        }
    }
}

/// The nodes `expression` reads a port of, at any depth.
fn referenced_nodes(expression: &Expression, found: &mut HashSet<Uuid>) {
    match expression {
        Expression::NodeArgument(port) => {
            found.insert(port.node);
        }
        Expression::Select { source, .. } => referenced_nodes(source, found),
        Expression::Call(call) => {
            for (parameter, argument) in &call.arguments {
                referenced_nodes(parameter, found);
                referenced_nodes(argument, found);
            }
            referenced_nodes(&call.module, found);
            referenced_nodes(&call.function, found);
        }
        _ => {}
    }
}

/// A node's arguments in its function's parameter order, then any others by
/// id: a node holds them in a map.
pub(crate) fn ordered<'e>(
    arguments: &'e HashMap<Uuid, Expression>,
    signature: Option<&ModuleFunction>,
) -> Vec<(Uuid, &'e Expression)> {
    let order: &[Uuid] = signature
        .map(|signature| signature.function.parameter_ordering.as_slice())
        .unwrap_or_default();
    let mut ordered: Vec<(Uuid, &Expression)> = arguments
        .iter()
        .map(|(parameter, argument)| (*parameter, argument))
        .collect();
    // Each argument's position is found once.
    ordered.sort_by_cached_key(|(parameter, _)| {
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

/// [`ordered`], by the `positions` of the function's parameters.
pub(crate) fn ordered_by<'e>(
    arguments: &'e HashMap<Uuid, Expression>,
    positions: &crate::id_hash::IdMap<Uuid, usize>,
) -> Vec<(Uuid, &'e Expression)> {
    let mut ordered: Vec<(Uuid, &Expression)> = arguments
        .iter()
        .map(|(parameter, argument)| (*parameter, argument))
        .collect();
    ordered.sort_unstable_by_key(|(parameter, _)| {
        (
            positions.get(parameter).copied().unwrap_or(usize::MAX),
            *parameter,
        )
    });
    ordered
}

/// A parameter's name, or its id when its name is not written as a
/// parameter's name as it is.
fn parameter_name(signature: Option<&ModuleFunction>, parameter: Uuid) -> String {
    signature
        .and_then(|signature| signature.function.parameters.get(&parameter))
        .map(|parameter| parameter.name.clone())
        .filter(|name| is_parameter_name(name))
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

fn expression(expression: &Expression, names: &Names, form: Form, indent: usize) -> String {
    match expression {
        Expression::Value(value) => literal(value),
        Expression::Uuid(id) => format!("uuid({id})"),
        Expression::Variable(cell) => match (cell.get(), form.file) {
            (Some(value), true) => literal(&value),
            (None, true) => "()".to_string(),
            (Some(value), false) => format!("{{= {}}}", literal(&value)),
            (None, false) => "{=}".to_string(),
        },
        Expression::VariableId(id) => names.variable(*id),
        Expression::NodeArgument(port) if port.parameter == _RET_PARAM_ID => {
            format!("@{}.{}", names.node(port.node), crate::text::RETURN)
        }
        Expression::NodeArgument(port) => format!(
            "@{}.{}",
            names.node(port.node),
            names.any_parameter(port.parameter)
        ),
        Expression::Select { source, path } => {
            format!("{}[{path}]", self::expression(source, names, form, indent))
        }
        Expression::Call(call) => call_expression(call, names, form, indent),
    }
}

/// A part of a call that names an id: the id's name when it states one,
/// else the expression that computes it, in parentheses.
fn id_text(
    expression: &Expression,
    names: &Names,
    form: Form,
    name: impl Fn(Uuid) -> String,
) -> String {
    match stated_id(expression) {
        Some(id) => name(id),
        None => format!("({})", self::expression(expression, names, form.line(), 0)),
    }
}

fn call_expression(call: &CallExpression, names: &Names, form: Form, indent: usize) -> String {
    let signature = stated_id(&call.function).and_then(|id| names.function(id));
    let module = stated_id(&call.module);
    let head = match signature {
        Some(signature) if module == Some(signature.module_id) => names.head(signature),
        Some(signature) => format!(
            "{}::{}",
            id_text(&call.module, names, form, |id| id.to_string()),
            if is_name(&signature.function_name) {
                signature.function_name.clone()
            } else {
                signature.function_id.to_string()
            }
        ),
        None => format!(
            "{}::{}",
            id_text(&call.module, names, form, |id| id.to_string()),
            id_text(&call.function, names, form, |id| id.to_string())
        ),
    };
    let arguments: Vec<(String, &Expression)> = call
        .arguments
        .iter()
        .map(|(parameter, argument)| {
            (
                id_text(parameter, names, form, |id| parameter_name(signature, id)),
                argument,
            )
        })
        .collect();
    call_text(&head, &arguments, names, form, indent, 0)
}

/// `head(name: argument, …)` on one line, or, when `pretty` and that line
/// would pass [`WIDTH`] from `indent` and `lead` characters already written,
/// one argument per line.
fn call_text(
    head: &str,
    arguments: &[(String, &Expression)],
    names: &Names,
    form: Form,
    indent: usize,
    lead: usize,
) -> String {
    let line = format!(
        "{head}({})",
        arguments
            .iter()
            .map(|(name, argument)| {
                format!("{name}: {}", expression(argument, names, form.line(), 0))
            })
            .collect::<Vec<_>>()
            .join(", ")
    );
    if !form.pretty || arguments.is_empty() || indent * 4 + lead + line.len() <= WIDTH {
        return line;
    }
    let pad = "    ".repeat(indent + 1);
    let mut text = format!("{head}(\n");
    for (name, argument) in arguments {
        text.push_str(&format!(
            "{pad}{name}: {},\n",
            expression(argument, names, form, indent + 1)
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
    fn floats<T: Display + std::fmt::Debug + Copy>(elements: &[T]) -> String {
        elements
            .iter()
            .map(|element| float(*element))
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
            "str[{}]",
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
        Value::ArrayF32(elements) => format!("f32[{}]", floats(elements)),
        Value::ArrayF64(elements) => format!("f64[{}]", floats(elements)),
        Value::F32(value) => format!("{}f32", float(*value)),
        Value::F64(value) => format!("{}f64", float(*value)),
        Value::Structure(_)
        | Value::Enumeration(_)
        | Value::KeyValue(_)
        | Value::ArrayStructure { .. }
        | Value::ArrayEnumeration { .. } => format!(
            "json({:?})",
            serde_json::to_string(value).expect("a value has a JSON form")
        ),
        other => other.to_string(),
    }
}

/// A float in its shortest form that reads back: with an exponent when that
/// form has one (`1e300`, `1e-7`), as digits otherwise (`100`, `0.2`).
fn float<T: Display + std::fmt::Debug>(value: T) -> String {
    let debug = format!("{value:?}");
    if debug.contains('e') {
        debug
    } else {
        value.to_string()
    }
}
