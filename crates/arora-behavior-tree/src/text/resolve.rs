//! Resolution: a [`Document`] to the shared [`Graph`] an interpreter runs.
//!
//! - A node's id derives from its name, or from its path when it has none
//!   (`root`, `root.0.2`, `detached.3`, `<holder's path>.<parameter>` for a
//!   call in an argument), so an edit that keeps a node's name or place
//!   keeps its id, and the interpreter keeps its state.
//! - A function is found by name among the natives, then in the function
//!   index; `module::name` picks one of several modules' functions of one
//!   name.
//! - A variable `{name}` is the graph variable `name`: a store key, or a
//!   tree-local variable when it starts with `.`.
//! - A call in an argument is a node of its own that no node holds, read
//!   through its return value.
//! - A literal takes its type from the parameter it is given to.

use std::collections::{HashMap, HashSet};

use arora_behavior::graph::{Graph, Io, Link, LinkSource, Node as GraphNode, Port};
use arora_behavior::{FunctionIndex, ModuleFunction};
use arora_types::data::Key;
use arora_types::gen_uuid_from_str;
use arora_types::module::high::TypeRef;
use arora_types::record::ty::{FrozenTy, PrimitiveKind};
use arora_types::value::Value;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ast::{Call, Document, Expr, ExprKind, FunctionName, Node, Span};
use super::natives::{native, native_named, seed};
use super::Diagnostic;
use crate::schema::_RET_PARAM_ID;

/// The namespace node ids derive in: a node's id is the UUID v5 of its name,
/// or of its path, under it.
const NODE_NAMESPACE: Uuid = uuid::uuid!("cd5d3316-594e-459a-b1a5-6061fadc477e");

/// A resolved text: the graph to run, where each of its nodes is written,
/// and what is wrong with the text.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Loaded {
    pub graph: Graph,
    /// One per graph node, in the order the text writes them, a call in an
    /// argument right after the node that holds it.
    pub nodes: Vec<LoadedNode>,
    pub diagnostics: Vec<Diagnostic>,
}

/// A graph node and the text it comes from.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LoadedNode {
    pub id: Uuid,
    /// Its name, if the text names it.
    pub name: Option<String>,
    /// Its line, or its call for a call in an argument.
    pub span: Span,
    /// Whether the tree holds it: the root, and the nodes under it.
    pub attached: bool,
    /// The function it calls.
    pub function: Uuid,
    /// The node holding it: its parent in the tree, or the node in whose
    /// argument its call is written.
    pub parent: Option<Uuid>,
    /// Its place in the text: `root`, `root.0.2`, `detached.3`, or
    /// `<holder's path>.<parameter>` for a call in an argument.
    pub path: String,
}

/// Resolve `document` against `functions`, the device's function index, and
/// the [`natives`](super::natives). Every node and link that resolves is in
/// the graph, in spite of those that do not, which the diagnostics report.
pub fn resolve(document: &Document, functions: &FunctionIndex) -> Loaded {
    let mut resolver = Resolver {
        functions: Functions::new(functions),
        graph: Graph::empty(),
        nodes: Vec::new(),
        diagnostics: Vec::new(),
        named: HashMap::new(),
        ids: HashSet::new(),
        slots: HashMap::new(),
    };
    let mut planned = Vec::new();
    for (index, node) in document.nodes.iter().enumerate() {
        let (path, attached) = match index {
            0 => ("root".to_string(), true),
            index => (format!("detached.{}", index - 1), false),
        };
        resolver.plan(node, path, None, attached, &mut planned);
    }
    if document.nodes.is_empty() {
        resolver.diagnostics.push(Diagnostic::warning(
            "the text holds no node: nothing runs".to_string(),
            Span::default(),
        ));
    }
    for (index, plan) in planned.iter().enumerate() {
        let top = plan.parent.is_none() && index > 0;
        resolver.build(plan, top);
    }
    if let Some(root) = planned.first().filter(|root| root.function.is_some()) {
        resolver.graph.root = Some(root.id);
    }
    resolver
        .diagnostics
        .sort_by_key(|diagnostic| diagnostic.span.start);
    Loaded {
        graph: resolver.graph,
        nodes: resolver.nodes,
        diagnostics: resolver.diagnostics,
    }
}

/// The functions a text can name: the natives, and a function index.
pub(crate) struct Functions<'a> {
    index: &'a FunctionIndex,
    by_name: HashMap<&'a str, Vec<&'a ModuleFunction>>,
}

/// A function a text names.
#[derive(Clone, Copy)]
pub(crate) enum Found<'a> {
    /// A native, or a function of the index.
    Known(&'a ModuleFunction),
    /// An id that is neither: its signature is not known.
    Unknown(Uuid),
}

impl Found<'_> {
    fn id(&self) -> Uuid {
        match self {
            Found::Known(function) => function.function_id,
            Found::Unknown(id) => *id,
        }
    }
}

impl<'a> Functions<'a> {
    pub(crate) fn new(index: &'a FunctionIndex) -> Self {
        let mut by_name: HashMap<&str, Vec<&ModuleFunction>> = HashMap::new();
        for function in index.values() {
            by_name
                .entry(function.function_name.as_str())
                .or_default()
                .push(function);
        }
        for functions in by_name.values_mut() {
            functions.sort_by_key(|function| function.module_id);
        }
        Self { index, by_name }
    }

    /// The function `name` names, or why none.
    pub(crate) fn find(&self, name: &FunctionName) -> Result<Found<'a>, String> {
        let text = name.name.text.as_str();
        let id = Uuid::parse_str(text).ok();
        let Some(module) = &name.module else {
            if let Some(native) = native_named(text) {
                return Ok(Found::Known(&native.function));
            }
            if let Some(id) = id {
                return Ok(native(id)
                    .map(|native| &native.function)
                    .or_else(|| self.index.get(&id))
                    .map_or(Found::Unknown(id), Found::Known));
            }
            return match self.by_name.get(text).map(Vec::as_slice) {
                Some([function]) => Ok(Found::Known(function)),
                Some(functions) if !functions.is_empty() => Err(format!(
                    "`{text}` is a function of several modules: write `<module>::{text}`, \
                     the module one of {}",
                    functions
                        .iter()
                        .map(|function| function.module_id.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
                _ => Err(format!("no function is named `{text}`")),
            };
        };
        let Ok(module_id) = Uuid::parse_str(&module.text) else {
            return Err(format!(
                "`{}` is no module id: a module is named by its id, as in `<module>::{text}`",
                module.text
            ));
        };
        let found = match id {
            Some(id) => match self.index.get(&id) {
                Some(function) => Some(function),
                None => return Ok(Found::Unknown(id)),
            },
            None => self.by_name.get(text).and_then(|functions| {
                functions
                    .iter()
                    .copied()
                    .find(|function| function.module_id == module_id)
            }),
        };
        match found {
            Some(function) if function.module_id == module_id => Ok(Found::Known(function)),
            Some(function) => Err(format!(
                "`{text}` is a function of module {}, not of module {module_id}: \
                 the graph calls the module the function index names",
                function.module_id
            )),
            None => Err(format!("module {module_id} has no function `{text}`")),
        }
    }
}

/// The id a node named `name` has, or a node at `path` that has no name.
fn node_id(name_or_path: &str) -> Uuid {
    Uuid::parse_str(name_or_path)
        .unwrap_or_else(|_| Uuid::new_v5(&NODE_NAMESPACE, name_or_path.as_bytes()))
}

/// The parameter of `function` named `name`, by its name or its id.
fn parameter_named(function: &ModuleFunction, name: &str) -> Option<Uuid> {
    let parameters = &function.function.parameters;
    parameters
        .iter()
        .find(|(_, parameter)| parameter.name == name)
        .map(|(id, _)| *id)
        .or_else(|| {
            Uuid::parse_str(name)
                .ok()
                .filter(|id| parameters.contains_key(id))
        })
}

/// A node of the text, its id and function found.
struct Planned<'d, 'a> {
    node: &'d Node,
    id: Uuid,
    path: String,
    parent: Option<Uuid>,
    attached: bool,
    /// `None` when the function is not found, which is reported.
    function: Option<Found<'a>>,
    children: Vec<Uuid>,
}

/// A named node, as a reference to it reads it.
#[derive(Clone, Copy)]
struct Declared<'a> {
    id: Uuid,
    function: Option<Found<'a>>,
}

struct Resolver<'a> {
    functions: Functions<'a>,
    graph: Graph,
    nodes: Vec<LoadedNode>,
    diagnostics: Vec<Diagnostic>,
    /// The named nodes, by name.
    named: HashMap<String, Declared<'a>>,
    /// Every node id given.
    ids: HashSet<Uuid>,
    /// Each function's slots, as [`slots`] gives them.
    slots: HashMap<Uuid, (Vec<Io>, Vec<Io>)>,
}

impl<'a> Resolver<'a> {
    fn error(&mut self, message: String, span: Span) {
        self.diagnostics.push(Diagnostic::error(message, span));
    }

    fn warning(&mut self, message: String, span: Span) {
        self.diagnostics.push(Diagnostic::warning(message, span));
    }

    /// Give `node` and the nodes under it their ids, and find their
    /// functions.
    fn plan<'d>(
        &mut self,
        node: &'d Node,
        path: String,
        parent: Option<Uuid>,
        attached: bool,
        planned: &mut Vec<Planned<'d, 'a>>,
    ) -> Uuid {
        let id = node_id(node.name.as_ref().map_or(path.as_str(), |name| &name.text));
        self.claim(
            id,
            node.span,
            node.name.as_ref().map(|name| name.text.as_str()),
            &path,
        );
        let function = match self.functions.find(&node.call.function) {
            Ok(function) => Some(function),
            Err(message) => {
                self.error(message, node.call.function.span);
                None
            }
        };
        if let Some(name) = &node.name {
            if self.named.contains_key(&name.text) {
                self.error(format!("another node is named `{}`", name.text), name.span);
            } else {
                self.named
                    .insert(name.text.clone(), Declared { id, function });
            }
        }
        let at = planned.len();
        planned.push(Planned {
            node,
            id,
            path: path.clone(),
            parent,
            attached,
            function,
            children: Vec::new(),
        });
        planned[at].children = node
            .children
            .iter()
            .enumerate()
            .map(|(index, child)| {
                self.plan(
                    child,
                    format!("{path}.{index}"),
                    Some(id),
                    attached,
                    planned,
                )
            })
            .collect();
        id
    }

    /// Record that the node at `span` has `id`, reporting a node that has it
    /// already: a name colliding with another node's path.
    fn claim(&mut self, id: Uuid, span: Span, name: Option<&str>, path: &str) {
        if !self.ids.insert(id) && name.is_none_or(|name| !self.named.contains_key(name)) {
            self.error(
                format!(
                    "this node has the id of another node: `{}` is the path or the name of both",
                    name.unwrap_or(path)
                ),
                span,
            );
        }
    }

    /// Add `plan`'s node to the graph, with its links and the calls in its
    /// arguments.
    fn build(&mut self, plan: &Planned<'_, 'a>, top: bool) {
        let node = plan.node;
        let Some(function) = plan.function else {
            return;
        };
        let signature = match function {
            Found::Known(signature) => Some(signature),
            Found::Unknown(id) => {
                self.warning(
                    format!(
                        "function {id} is not in the function index: its arguments are not \
                         typed, and the node fails when it runs"
                    ),
                    node.call.function.span,
                );
                None
            }
        };
        let label = node.call.function.name.text.as_str();
        let native = native(function.id());
        if !node.children.is_empty() && native.is_some_and(|native| !native.children) {
            self.error(
                format!("`{label}` holds no children"),
                node.call.function.span,
            );
        }
        if !node.children.is_empty() && native.is_none() && signature.is_some() {
            self.error(
                format!("`{label}` is a module function: it holds no children"),
                node.call.function.span,
            );
        }
        if top && (native.is_some() || !node.children.is_empty()) {
            self.warning(
                "no node holds this node, and it is not a module call without children: \
                 it never runs"
                    .to_string(),
                node.span,
            );
        }
        let children = (!node.children.is_empty() || native.is_some_and(|native| native.children))
            .then(|| plan.children.clone());
        self.nodes.push(LoadedNode {
            id: plan.id,
            name: node.name.as_ref().map(|name| name.text.clone()),
            span: node.span,
            attached: plan.attached,
            function: function.id(),
            parent: plan.parent,
            path: plan.path.clone(),
        });
        self.node(
            plan.id, function, signature, children, &node.call, &plan.path,
        );
        if let Some(target) = &node.target {
            if let Some(source) = self.target(target) {
                self.graph
                    .links
                    .push(Link::new(Port::new(plan.id, _RET_PARAM_ID), source));
            }
        }
    }

    /// Add the graph node `id` calling `function`, and its arguments' links.
    fn node(
        &mut self,
        id: Uuid,
        function: Found<'a>,
        signature: Option<&'a ModuleFunction>,
        children: Option<Vec<Uuid>>,
        call: &Call,
        path: &str,
    ) {
        let (inputs, outputs) = match signature {
            Some(signature) => self
                .slots
                .entry(signature.function_id)
                .or_insert_with(|| slots(signature))
                .clone(),
            None => (Vec::new(), vec![Io::new(_RET_PARAM_ID)]),
        };
        self.graph.nodes.insert(
            id,
            GraphNode {
                id,
                function: function.id(),
                inputs,
                outputs,
                children,
            },
        );
        let label = call.function.name.text.as_str();
        let mut given = HashSet::new();
        for argument in &call.arguments {
            let name = argument.name.text.as_str();
            let parameter = match signature {
                Some(signature) => parameter_named(signature, name),
                None => Uuid::parse_str(name).ok(),
            };
            let Some(parameter) = parameter else {
                let message = match signature {
                    Some(_) => format!("`{label}` has no parameter `{name}`"),
                    None => format!(
                        "`{name}` is no id: the parameters of a function the index does not \
                         have are named by their ids"
                    ),
                };
                self.error(message, argument.name.span);
                continue;
            };
            if !given.insert(parameter) {
                self.error(format!("`{name}` is given twice"), argument.name.span);
                continue;
            }
            let ty = signature
                .and_then(|signature| signature.function.parameters.get(&parameter))
                .map(|parameter| &parameter.ty);
            if let Some(source) = self.source(&argument.value, ty, &format!("{path}.{name}"), id) {
                self.graph
                    .links
                    .push(Link::new(Port::new(id, parameter), source));
            }
        }
        if let Some((parameter, value)) =
            seed(function.id()).filter(|(parameter, _)| !given.contains(parameter))
        {
            self.graph.links.push(Link::new(
                Port::new(id, parameter),
                LinkSource::Literal(value),
            ));
            given.insert(parameter);
        }
        if let Some(signature) = signature {
            let missing: Vec<&str> = match native(signature.function_id) {
                Some(native) => native
                    .required
                    .iter()
                    .filter(|parameter| !given.contains(parameter))
                    .map(|parameter| signature.function.parameters[parameter].name.as_str())
                    .collect(),
                None => signature
                    .function
                    .parameter_ordering
                    .iter()
                    .filter(|parameter| !given.contains(parameter))
                    .filter_map(|parameter| signature.function.parameters.get(parameter))
                    .filter(|parameter| !parameter.ty.is_option())
                    .map(|parameter| parameter.name.as_str())
                    .collect(),
            };
            if !missing.is_empty() {
                self.warning(
                    format!(
                        "`{label}` is not given {}",
                        missing
                            .iter()
                            .map(|name| format!("`{name}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    call.span,
                );
            }
        }
    }

    /// The source a node's return value is written to: a variable or another
    /// node's port.
    fn target(&mut self, target: &Expr) -> Option<LinkSource> {
        match &target.kind {
            ExprKind::Variable { .. } | ExprKind::Port { .. } => {
                self.source(target, None, "", Uuid::nil())
            }
            _ => {
                self.error(
                    "a node's return value is written to a variable `{key}` or a port \
                     `@node.parameter`"
                        .to_string(),
                    target.span,
                );
                None
            }
        }
    }

    /// The link source `expression` is, given to a parameter of type `ty`; a
    /// call is added as a node of its own at `path`, held by `holder`.
    fn source(
        &mut self,
        expression: &Expr,
        ty: Option<&FrozenTy>,
        path: &str,
        holder: Uuid,
    ) -> Option<LinkSource> {
        match &expression.kind {
            ExprKind::Variable { name } => {
                let id = gen_uuid_from_str(name);
                if Uuid::parse_str(name).is_err() {
                    self.graph.variables.insert(id, name.clone());
                }
                Some(LinkSource::Variable(id))
            }
            ExprKind::Port { node, parameter } => {
                let declared = match self.named.get(&node.text) {
                    Some(declared) => *declared,
                    None => match Uuid::parse_str(&node.text) {
                        Ok(id) => Declared { id, function: None },
                        Err(_) => {
                            self.error(format!("no node is named `{}`", node.text), node.span);
                            return None;
                        }
                    },
                };
                let port = match parameter {
                    None => _RET_PARAM_ID,
                    Some(parameter) => {
                        let found = match declared.function {
                            Some(Found::Known(function)) => {
                                parameter_named(function, &parameter.text)
                            }
                            _ => Uuid::parse_str(&parameter.text).ok(),
                        };
                        match found {
                            Some(port) => port,
                            None => {
                                self.error(
                                    format!(
                                        "`{}` has no parameter `{}`",
                                        node.text, parameter.text
                                    ),
                                    parameter.span,
                                );
                                return None;
                            }
                        }
                    }
                };
                Some(LinkSource::Port(Port::new(declared.id, port)))
            }
            ExprKind::Select {
                source,
                path: selected,
            } => {
                let source = self.source(source, None, path, holder)?;
                Some(LinkSource::Select {
                    source: Box::new(source),
                    path: Key::new(selected.as_str()),
                })
            }
            ExprKind::Call { call } => self.call(call, path, holder),
            _ => match literal(expression, &expected(ty)) {
                Ok(value) => Some(LinkSource::Literal(value)),
                Err((message, span)) => {
                    self.error(message, span);
                    None
                }
            },
        }
    }

    /// A call in an argument: a node no node holds, read through its return
    /// value.
    fn call(&mut self, call: &Call, path: &str, holder: Uuid) -> Option<LinkSource> {
        let function = match self.functions.find(&call.function) {
            Ok(function) => function,
            Err(message) => {
                self.error(message, call.function.span);
                return None;
            }
        };
        if native(function.id()).is_some() {
            self.error(
                format!(
                    "`{}` is a native node: a call in an argument is a module function's",
                    call.function.name.text
                ),
                call.function.span,
            );
            return None;
        }
        let signature = match function {
            Found::Known(signature) => Some(signature),
            Found::Unknown(id) => {
                self.warning(
                    format!(
                        "function {id} is not in the function index: its arguments are not \
                         typed, and the node fails when it runs"
                    ),
                    call.function.span,
                );
                None
            }
        };
        let id = node_id(path);
        self.claim(id, call.span, None, path);
        self.nodes.push(LoadedNode {
            id,
            name: None,
            span: call.span,
            attached: false,
            function: function.id(),
            parent: Some(holder),
            path: path.to_string(),
        });
        self.node(id, function, signature, None, call, path);
        Some(LinkSource::Port(Port::new(id, _RET_PARAM_ID)))
    }
}

/// A function's slots: every parameter an input, and its return value and
/// every parameter it writes an output.
fn slots(function: &ModuleFunction) -> (Vec<Io>, Vec<Io>) {
    let slot = |id: Uuid, ty: &FrozenTy| Io {
        id,
        ty: type_ref(ty),
        predetermined_key: None,
    };
    let signature = &function.function;
    let mut inputs = Vec::with_capacity(signature.parameter_ordering.len());
    let mut outputs = vec![slot(_RET_PARAM_ID, &signature.return_ty)];
    for id in &signature.parameter_ordering {
        let Some(parameter) = signature.parameters.get(id) else {
            continue;
        };
        inputs.push(slot(*id, &parameter.ty));
        if parameter.mutable {
            outputs.push(slot(*id, &parameter.ty));
        }
    }
    (inputs, outputs)
}

/// A frozen type as a graph slot's type: a primitive by its name, a record
/// by its id. `None` for an optional of anything but a scalar, which a slot's
/// type does not write.
fn type_ref(ty: &FrozenTy) -> Option<TypeRef> {
    Some(match ty {
        FrozenTy::Primitive(primitive) if primitive.kind.is_array() => TypeRef::Array {
            id: primitive
                .kind
                .to_string()
                .trim_end_matches("[]")
                .to_string(),
        },
        FrozenTy::Primitive(primitive) => TypeRef::Scalar {
            id: primitive.kind.to_string(),
        },
        FrozenTy::FrozenScalar(scalar) => TypeRef::Scalar {
            id: scalar.reference.id.to_string(),
        },
        FrozenTy::FrozenArray(array) => TypeRef::Array {
            id: array.reference.id.to_string(),
        },
        FrozenTy::FrozenOption(option) => match type_ref(&option.element)? {
            TypeRef::Scalar { id } => TypeRef::Option { id },
            _ => return None,
        },
    })
}

/// The type a literal is read as.
enum Expected {
    Primitive(PrimitiveKind),
    /// An optional: a literal is its content, or `none`, or `some(…)`.
    Option(Box<Expected>),
    /// Any type: a literal states its own.
    Any,
}

fn expected(ty: Option<&FrozenTy>) -> Expected {
    match ty {
        Some(FrozenTy::Primitive(primitive)) => Expected::Primitive(primitive.kind),
        Some(FrozenTy::FrozenOption(option)) => {
            Expected::Option(Box::new(expected(Some(&option.element))))
        }
        _ => Expected::Any,
    }
}

type Typed = Result<Value, (String, Span)>;

/// The value the literal `expression` writes, read as `expected`.
fn literal(expression: &Expr, expected: &Expected) -> Typed {
    let span = expression.span;
    let mismatch = |what: &str, kind: &PrimitiveKind| {
        Err((
            format!("{what} is given to a parameter of type {kind}"),
            span,
        ))
    };
    match (&expression.kind, expected) {
        (ExprKind::None, Expected::Option(_) | Expected::Any) => Ok(Value::Option(None)),
        (ExprKind::Some { value }, Expected::Option(inner)) => {
            Ok(Value::Option(Some(Box::new(literal(value, inner)?))))
        }
        (ExprKind::Some { value }, Expected::Any) => Ok(Value::Option(Some(Box::new(literal(
            value,
            &Expected::Any,
        )?)))),
        (ExprKind::None | ExprKind::Some { .. }, Expected::Primitive(kind)) => {
            mismatch("an optional", kind)
        }
        // An optional parameter's value is framed when it is passed.
        (_, Expected::Option(inner)) => literal(expression, inner),
        (ExprKind::Number { text, suffix }, expected) => {
            let kind = match (suffix, expected) {
                (Some(suffix), Expected::Primitive(kind))
                    if suffix.as_str() != kind.to_string() =>
                {
                    return mismatch(&format!("a {suffix}"), kind)
                }
                (Some(suffix), _) => suffix
                    .parse::<PrimitiveKind>()
                    .map_err(|error| (error, span))?,
                (None, Expected::Primitive(kind)) => *kind,
                (None, _) => {
                    return Err((
                        format!(
                            "`{text}` has no type here: write it with its type, as `{text}f64`"
                        ),
                        span,
                    ))
                }
            };
            number(text, kind).ok_or_else(|| (format!("`{text}` is no {kind}"), span))
        }
        (
            ExprKind::String { value },
            Expected::Primitive(PrimitiveKind::String) | Expected::Any,
        ) => Ok(Value::String(value.clone())),
        (ExprKind::String { .. }, Expected::Primitive(kind)) => mismatch("a string", kind),
        (ExprKind::Bool { value }, Expected::Primitive(PrimitiveKind::Boolean) | Expected::Any) => {
            Ok(Value::Boolean(*value))
        }
        (ExprKind::Bool { .. }, Expected::Primitive(kind)) => mismatch("a boolean", kind),
        (ExprKind::Unit, Expected::Primitive(PrimitiveKind::Unit) | Expected::Any) => {
            Ok(Value::Unit)
        }
        (ExprKind::Unit, Expected::Primitive(kind)) => mismatch("`()`", kind),
        (ExprKind::Array { element, elements }, expected) => {
            let stated = match element {
                Some(element) => Some(
                    format!("{element}[]")
                        .parse::<PrimitiveKind>()
                        .map_err(|error| (error, span))?,
                ),
                None => None,
            };
            match (stated, expected) {
                (Some(stated), Expected::Primitive(kind)) if stated != *kind => {
                    mismatch(&format!("a {stated}"), kind)
                }
                (Some(kind), _) => array(kind, elements),
                (None, Expected::Primitive(kind)) if kind.is_array() => array(*kind, elements),
                (None, Expected::Primitive(kind)) => mismatch("an array", kind),
                (None, _) => Ok(Value::ArrayValue(
                    elements
                        .iter()
                        .map(|element| literal(element, &Expected::Any))
                        .collect::<Result<_, _>>()?,
                )),
            }
        }
        (ExprKind::Uuid { value }, _) => Ok(Value::Uuid(*value)),
        (ExprKind::Json { text }, _) => serde_json::from_str(text)
            .map_err(|error| (format!("not the JSON form of a value: {error}"), span)),
        (
            ExprKind::Variable { .. }
            | ExprKind::Port { .. }
            | ExprKind::Select { .. }
            | ExprKind::Call { .. },
            _,
        ) => Err((
            "an array holds literals: a variable, a port or a call is an argument of its own"
                .to_string(),
            span,
        )),
    }
}

/// `text` as a number of type `kind`.
fn number(text: &str, kind: PrimitiveKind) -> Option<Value> {
    use PrimitiveKind::*;
    Some(match kind {
        U8 => Value::U8(text.parse().ok()?),
        U16 => Value::U16(text.parse().ok()?),
        U32 => Value::U32(text.parse().ok()?),
        U64 => Value::U64(text.parse().ok()?),
        I8 => Value::I8(text.parse().ok()?),
        I16 => Value::I16(text.parse().ok()?),
        I32 => Value::I32(text.parse().ok()?),
        I64 => Value::I64(text.parse().ok()?),
        F32 => Value::F32(text.parse().ok()?),
        F64 => Value::F64(text.parse().ok()?),
        _ => return None,
    })
}

/// The array of type `kind` holding `elements`, each read as its element
/// type.
fn array(kind: PrimitiveKind, elements: &[Expr]) -> Typed {
    use PrimitiveKind::*;
    let element = match kind {
        ArrayBoolean => Boolean,
        ArrayU8 => U8,
        ArrayU16 => U16,
        ArrayU32 => U32,
        ArrayU64 => U64,
        ArrayI8 => I8,
        ArrayI16 => I16,
        ArrayI32 => I32,
        ArrayI64 => I64,
        ArrayF32 => F32,
        ArrayF64 => F64,
        ArrayString => String,
        _ => unreachable!("an array kind"),
    };
    let values = elements
        .iter()
        .map(|value| literal(value, &Expected::Primitive(element)))
        .collect::<Result<Vec<_>, _>>()?;
    macro_rules! collect {
        ($variant:ident, $array:ident) => {
            Value::$array(
                values
                    .into_iter()
                    .map(|value| match value {
                        Value::$variant(value) => value,
                        _ => unreachable!("read as the element type"),
                    })
                    .collect(),
            )
        };
    }
    Ok(match kind {
        ArrayBoolean => collect!(Boolean, ArrayBoolean),
        ArrayU8 => collect!(U8, ArrayU8),
        ArrayU16 => collect!(U16, ArrayU16),
        ArrayU32 => collect!(U32, ArrayU32),
        ArrayU64 => collect!(U64, ArrayU64),
        ArrayI8 => collect!(I8, ArrayI8),
        ArrayI16 => collect!(I16, ArrayI16),
        ArrayI32 => collect!(I32, ArrayI32),
        ArrayI64 => collect!(I64, ArrayI64),
        ArrayF32 => collect!(F32, ArrayF32),
        ArrayF64 => collect!(F64, ArrayF64),
        _ => collect!(String, ArrayString),
    })
}
