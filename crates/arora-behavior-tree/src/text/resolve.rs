//! Resolution: a [`Document`] to the shared [`Graph`] an interpreter runs.
//!
//! - A node's id derives from its name, or from its path when it has none
//!   (`root`, `root.0.2`, `detached.3`, `<holder's path>.<parameter>` for a
//!   call in an argument), so an edit that keeps a node's name or place
//!   keeps its id, and the interpreter keeps its state.
//! - A function is found by name among the natives, then in the function
//!   index; `module::name` picks one of several modules' functions of one
//!   name.
//! - A variable `{name}` binds the key `name`: a store key, or a tree-local
//!   variable when it starts with `.`. `{<uuid>}` binds the tree-local key
//!   `.<uuid>`.
//! - A call in an argument is a node of its own that no node holds, read
//!   through its return value.
//! - A literal takes its type from the parameter it is given to.
//! - What lowering refuses is an error here, where it is written: a read of
//!   a detached node's port that is no output, detached nodes reading one
//!   another in a cycle. Two nodes writing one store key are warned about,
//!   with the one that wins.

use std::collections::{BTreeMap, HashMap, HashSet};

use arora_behavior::graph::{source_port, Graph, Node as GraphNode, Port, Source};
use arora_behavior::{FunctionIndex, ModuleFunction};
use arora_types::data::Key;
use arora_types::record::ty::{FrozenTy, PrimitiveKind};
use arora_types::value::Value;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::ast::{Call, Document, Expr, ExprKind, FunctionName, Node, Span};
use super::natives::{native, native_named, seed};
use super::{Diagnostic, MAX_DEPTH};
use crate::schema::_RET_PARAM_ID;
use crate::variable::{is_tree_local, TREE_LOCAL_PREFIX};

/// The namespace node ids derive in: a node's id is the UUID v5 of its name,
/// or of its path, under it.
/// The name of a node's return value as a port: `@node._ret`.
pub(crate) const RETURN: &str = "_ret";

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
        declared: HashMap::new(),
        signatures: HashMap::new(),
        positions: HashMap::new(),
    };
    let mut planned = Vec::new();
    for (index, node) in document.nodes.iter().enumerate() {
        let (path, attached) = match index {
            0 => ("root".to_string(), true),
            index => (format!("detached.{}", index - 1), false),
        };
        resolver.plan(node, path, None, attached, 0, &mut planned);
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
    resolver.cycles();
    resolver.shared_keys();
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
    pub(crate) fn id(&self) -> Uuid {
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
pub(crate) fn node_id(name_or_path: &str) -> Uuid {
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

/// A node of the text, as a reference to it reads it.
#[derive(Clone, Copy)]
struct Declared<'a> {
    function: Option<Found<'a>>,
    attached: bool,
}

struct Resolver<'a> {
    functions: Functions<'a>,
    graph: Graph,
    nodes: Vec<LoadedNode>,
    diagnostics: Vec<Diagnostic>,
    /// The named nodes' ids, and their names' spans, by name.
    named: HashMap<String, (Uuid, Span)>,
    /// Every node of the text, by id: the nodes a reference can name.
    declared: HashMap<Uuid, Declared<'a>>,
    /// The signature of every graph node, by id, when known.
    signatures: HashMap<Uuid, &'a ModuleFunction>,
    /// Where each graph node is in the graph's list.
    positions: HashMap<Uuid, usize>,
}

impl<'a> Resolver<'a> {
    fn error(&mut self, message: String, span: Span) {
        self.diagnostics.push(Diagnostic::error(message, span));
    }

    fn warning(&mut self, message: String, span: Span) {
        self.diagnostics.push(Diagnostic::warning(message, span));
    }

    /// Give `node` and the nodes under it their ids, and find their
    /// functions. Children nested deeper than [`MAX_DEPTH`] are reported and
    /// left out.
    fn plan<'d>(
        &mut self,
        node: &'d Node,
        path: String,
        parent: Option<Uuid>,
        attached: bool,
        depth: usize,
        planned: &mut Vec<Planned<'d, 'a>>,
    ) -> Uuid {
        let name = node.name.as_ref();
        let id = node_id(name.map_or(path.as_str(), |name| &name.text));
        let function = match self.functions.find(&node.call.function) {
            Ok(function) => Some(function),
            Err(message) => {
                self.error(message, node.call.function.span);
                None
            }
        };
        match name.map(|name| (name, self.named.get(&name.text).copied())) {
            Some((name, Some((_, first)))) => {
                self.error(format!("another node is named `{}`", name.text), name.span);
                self.error(format!("`{}` names another node too", name.text), first);
            }
            _ => {
                if self.declared.contains_key(&id) {
                    self.error(
                        format!(
                            "this node has the id of another node: `{}` is the path or the name \
                             of both",
                            name.map_or(path.as_str(), |name| &name.text)
                        ),
                        node.span,
                    );
                }
                if let Some(name) = name {
                    self.named.insert(name.text.clone(), (id, name.span));
                }
                self.declared.insert(id, Declared { function, attached });
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
        if depth + 1 >= MAX_DEPTH && !node.children.is_empty() {
            self.error(
                format!("blocks nest at most {MAX_DEPTH} levels deep"),
                node.children[0].span,
            );
            return id;
        }
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
                    depth + 1,
                    planned,
                )
            })
            .collect();
        id
    }

    /// Add `plan`'s node to the graph, with its bindings and the calls in
    /// its arguments.
    fn build(&mut self, plan: &Planned<'_, 'a>, top: bool) {
        let node = plan.node;
        let Some(function) = plan.function else {
            return;
        };
        let signature = self.signature(function, node.call.function.span);
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
        self.node(plan.id, function, children, &node.call, &plan.path, 0);
        if let Some(target) = &node.target {
            if let Some(source) = self.target(target) {
                self.bind(plan.id, _RET_PARAM_ID, source);
            }
        }
    }

    /// Bind `port` of the graph node `id` to `source`.
    fn bind(&mut self, id: Uuid, port: Uuid, source: Source) {
        let at = self.positions[&id];
        self.graph.nodes[at].bind.insert(port, source);
    }

    /// The signature of `function`, warning at `span` when it is not known.
    fn signature(&mut self, function: Found<'a>, span: Span) -> Option<&'a ModuleFunction> {
        match function {
            Found::Known(signature) => Some(signature),
            Found::Unknown(id) => {
                self.warning(
                    format!(
                        "function {id} is not in the function index: its arguments are not \
                         typed, and the node fails when it runs"
                    ),
                    span,
                );
                None
            }
        }
    }

    /// Add the graph node `id` calling `function`, its arguments bound. A
    /// node of an id the graph has replaces that node, in its place.
    fn node(
        &mut self,
        id: Uuid,
        function: Found<'a>,
        children: Option<Vec<Uuid>>,
        call: &Call,
        path: &str,
        depth: usize,
    ) {
        let signature = match function {
            Found::Known(signature) => Some(signature),
            Found::Unknown(_) => None,
        };
        if let Some(signature) = signature {
            self.signatures.insert(id, signature);
        }
        let node = GraphNode {
            id,
            function: function.id(),
            bind: Default::default(),
            children,
        };
        match self.positions.get(&id) {
            Some(at) => self.graph.nodes[*at] = node,
            None => {
                self.positions.insert(id, self.graph.nodes.len());
                self.graph.nodes.push(node);
            }
        }
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
            let call_path = format!("{path}.{name}");
            let read = Read {
                path: &call_path,
                holder: id,
                depth,
            };
            if let Some(source) = self.source(&argument.value, ty, read) {
                self.bind(id, parameter, source);
            }
        }
        if let Some((parameter, value)) =
            seed(function.id()).filter(|(parameter, _)| !given.contains(parameter))
        {
            self.bind(id, parameter, Source::Value(value));
            given.insert(parameter);
        }
        // A native's parameters a node must bind are known; a module
        // function declares no default, and a parameter a node leaves out is
        // not passed at all: the function decides what its absence means.
        if let Some(native) = native(function.id()) {
            let parameters = &native.function.function.parameters;
            let missing: Vec<String> = native
                .required
                .iter()
                .filter(|parameter| !given.contains(parameter))
                .map(|parameter| format!("`{}`", parameters[parameter].name))
                .collect();
            if !missing.is_empty() {
                self.warning(
                    format!("`{label}` is not given {}", missing.join(", ")),
                    call.span,
                );
            }
        }
    }

    /// Where a node's return value is written: a variable, or another node's
    /// input, which then reads the node. `@n.p` names an input of `n`,
    /// whatever `n` outputs.
    fn target(&mut self, target: &Expr) -> Option<Source> {
        match &target.kind {
            ExprKind::Variable { .. } | ExprKind::VariableId { .. } => {
                self.source(target, None, Read::TARGET)
            }
            ExprKind::Port { node, parameter } => {
                let (id, declared) = self.referenced(node)?;
                let port = self.parameter(node, parameter, declared)?;
                Some(Source::Port(Port::new(id, port)))
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

    /// The node `name` names, by its name or its id.
    fn referenced(&mut self, name: &super::ast::Name) -> Option<(Uuid, Declared<'a>)> {
        let id = match self.named.get(&name.text) {
            Some((id, _)) => *id,
            None => match Uuid::parse_str(&name.text) {
                Ok(id) => id,
                Err(_) => {
                    self.error(format!("no node is named `{}`", name.text), name.span);
                    return None;
                }
            },
        };
        match self.declared.get(&id) {
            Some(declared) => Some((id, *declared)),
            None => {
                self.error(format!("no node has the id {id}"), name.span);
                None
            }
        }
    }

    /// The port of the node `node` names that `parameter` names: `_ret` its
    /// return value, else a parameter of its function.
    fn parameter(
        &mut self,
        node: &super::ast::Name,
        parameter: &super::ast::Name,
        declared: Declared<'a>,
    ) -> Option<Uuid> {
        if parameter.text == RETURN {
            return Some(_RET_PARAM_ID);
        }
        let found = match declared.function {
            Some(Found::Known(function)) => parameter_named(function, &parameter.text),
            _ => Uuid::parse_str(&parameter.text).ok(),
        };
        if found.is_none() {
            self.error(
                format!("`{}` has no parameter `{}`", node.text, parameter.text),
                parameter.span,
            );
        }
        found
    }

    /// The source `expression` is, read as `read` says, given to a parameter
    /// of type `ty`.
    fn source(&mut self, expression: &Expr, ty: Option<&FrozenTy>, read: Read) -> Option<Source> {
        if read.depth >= MAX_DEPTH {
            self.error(
                format!("expressions nest at most {MAX_DEPTH} levels deep"),
                expression.span,
            );
            return None;
        }
        match &expression.kind {
            ExprKind::Variable { name } => Some(Source::Key(name.clone())),
            // A variable known by its id alone names no key: it is a cell of
            // the tree's own.
            ExprKind::VariableId { value } => {
                Some(Source::Key(format!("{TREE_LOCAL_PREFIX}{value}")))
            }
            ExprKind::Port { node, parameter } => {
                let (id, declared) = self.referenced(node)?;
                let port = match self.parameter(node, parameter, declared)? {
                    _RET_PARAM_ID => _RET_PARAM_ID,
                    port => {
                        // A detached node outputs its return value and the
                        // parameters it writes; a tree node's argument is a
                        // cell any node reads.
                        let output = match declared.function {
                            Some(Found::Known(function)) => function
                                .function
                                .parameters
                                .get(&port)
                                .is_some_and(|parameter| parameter.mutable),
                            _ => true,
                        };
                        if !declared.attached && !output {
                            self.error(
                                format!(
                                    "`{}` is held by no node: what it outputs is its return \
                                     value `@{}._ret` and the parameters it writes, and it does not \
                                     write `{}`",
                                    node.text, node.text, parameter.text
                                ),
                                parameter.span,
                            );
                            return None;
                        }
                        port
                    }
                };
                Some(Source::Port(Port::new(id, port)))
            }
            ExprKind::Select {
                source,
                path: selected,
            } => {
                let source = self.source(source, None, read.deeper())?;
                Some(Source::Select {
                    source: Box::new(source),
                    path: Key::new(selected.as_str()),
                })
            }
            ExprKind::Call { call } => self.call(call, read),
            _ => match literal(expression, &expected(ty), read.depth) {
                Ok(value) => Some(Source::Value(value)),
                Err((message, span)) => {
                    self.error(message, span);
                    None
                }
            },
        }
    }

    /// A call in an argument: a node no node holds, read through its return
    /// value.
    fn call(&mut self, call: &Call, read: Read) -> Option<Source> {
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
        // Warns when the function is not known.
        self.signature(function, call.function.span);
        let id = node_id(read.path);
        if self.declared.contains_key(&id) || self.positions.contains_key(&id) {
            self.error(
                format!(
                    "this call has the id of another node: `{}` is the path or the name of both",
                    read.path
                ),
                call.span,
            );
        }
        self.nodes.push(LoadedNode {
            id,
            name: None,
            span: call.span,
            attached: false,
            function: function.id(),
            parent: Some(read.holder),
            path: read.path.to_string(),
        });
        self.node(id, function, None, call, read.path, read.depth + 1);
        Some(Source::Port(Port::new(id, _RET_PARAM_ID)))
    }

    /// A node's name for a message: its name, or its path.
    fn label(&self, id: Uuid) -> String {
        self.nodes
            .iter()
            .find(|node| node.id == id)
            .map(|node| format!("`{}`", node.name.as_deref().unwrap_or(&node.path)))
            .unwrap_or_else(|| id.to_string())
    }

    /// Report the detached nodes that read one another in a cycle, which
    /// lowering refuses, a node reading an output of its own included.
    fn cycles(&mut self) {
        let loaded: HashMap<Uuid, (bool, Span)> = self
            .nodes
            .iter()
            .map(|node| (node.id, (node.attached, node.span)))
            .collect();
        let detached = |id: &Uuid| loaded.get(id).is_some_and(|(attached, _)| !attached);
        // Who reads whom, among detached nodes.
        let mut reads: BTreeMap<Uuid, Vec<Uuid>> = BTreeMap::new();
        let bindings = self.graph.nodes.iter().flat_map(|node| {
            node.bind
                .iter()
                .map(move |(port, source)| (Port::new(node.id, *port), source))
        });
        for (bound, source) in bindings {
            let (reader, read) = match (source, bound.port == _RET_PARAM_ID) {
                // A return value written to an input: that input's node reads
                // this one.
                (Source::Port(port), true) => (port.node, bound.node),
                (source, _) => match source_port(source) {
                    Some(port) if port == bound => continue,
                    Some(port) => (bound.node, port.node),
                    None => continue,
                },
            };
            if detached(&reader) && detached(&read) {
                reads.entry(reader).or_default().push(read);
            }
        }
        let mut state: HashMap<Uuid, u8> = HashMap::new(); // 1: on the path, 2: done
        let mut reported = HashSet::new();
        let starts: Vec<Uuid> = reads.keys().copied().collect();
        for start in starts {
            if state.contains_key(&start) {
                continue;
            }
            let mut path = vec![start];
            let mut next = vec![0usize];
            state.insert(start, 1);
            while let Some(&node) = path.last() {
                let index = *next.last().expect("one per node");
                match reads.get(&node).and_then(|read| read.get(index)).copied() {
                    Some(read) => {
                        *next.last_mut().expect("one per node") += 1;
                        match state.get(&read) {
                            None => {
                                state.insert(read, 1);
                                path.push(read);
                                next.push(0);
                            }
                            Some(1) => {
                                let from = path.iter().position(|id| *id == read).expect("on path");
                                let cycle: Vec<Uuid> = path[from..].to_vec();
                                let names = cycle
                                    .iter()
                                    .map(|id| self.label(*id))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                for id in cycle {
                                    if reported.insert(id) {
                                        let message = if from + 1 == path.len() {
                                            format!(
                                                "{names} reads an output of its own: feed a \
                                                 value back through a store key"
                                            )
                                        } else {
                                            format!(
                                                "{names} read one another in a cycle: feed a \
                                                 value back through a store key"
                                            )
                                        };
                                        self.error(message, loaded[&id].1);
                                    }
                                }
                            }
                            Some(_) => {}
                        }
                    }
                    None => {
                        state.insert(node, 2);
                        path.pop();
                        next.pop();
                    }
                }
            }
        }
    }

    /// Warn about the store keys several nodes write, one of them held by no
    /// node, and say which wins. A node no node holds publishes its keys
    /// after the tree ticks, so it wins over a tree node; among such nodes,
    /// the one with the greatest id publishes last, and wins: lowering lists
    /// them by id.
    fn shared_keys(&mut self) {
        let attached: HashMap<Uuid, (bool, Span)> = self
            .nodes
            .iter()
            .map(|node| (node.id, (node.attached, node.span)))
            .collect();
        let mut writers: BTreeMap<&str, Vec<Uuid>> = BTreeMap::new();
        for node in &self.graph.nodes {
            for (port, source) in &node.bind {
                let Source::Key(key) = source else {
                    continue;
                };
                let written = *port == _RET_PARAM_ID
                    || self
                        .signatures
                        .get(&node.id)
                        .and_then(|function| function.function.parameters.get(port))
                        .is_some_and(|parameter| parameter.mutable);
                if written && !is_tree_local(key) {
                    let nodes = writers.entry(key.as_str()).or_default();
                    if !nodes.contains(&node.id) {
                        nodes.push(node.id);
                    }
                }
            }
        }
        let mut warnings = Vec::new();
        for (key, nodes) in writers {
            let Some(winner) = nodes
                .iter()
                .filter(|id| attached.get(id).is_some_and(|(attached, _)| !attached))
                .max()
                .copied()
            else {
                continue;
            };
            if nodes.len() < 2 {
                continue;
            }
            let others = nodes
                .iter()
                .filter(|id| **id != winner)
                .map(|id| self.label(*id))
                .collect::<Vec<_>>()
                .join(", ");
            for id in &nodes {
                let message = if *id == winner {
                    format!(
                        "`{{{key}}}` is written by this node and by {others}: this node \
                         publishes it last, and wins"
                    )
                } else {
                    format!(
                        "`{{{key}}}` is written by this node and by others: {} publishes it \
                         last, and wins",
                        self.label(winner)
                    )
                };
                warnings.push(Diagnostic::warning(message, attached[id].1));
            }
        }
        self.diagnostics.extend(warnings);
    }
}

/// How an argument's expression is read: where a call in it is placed, the
/// node holding it, and how deep it is nested.
#[derive(Clone, Copy)]
struct Read<'p> {
    /// The path a call takes.
    path: &'p str,
    /// The node holding a call.
    holder: Uuid,
    /// How many expressions hold this one.
    depth: usize,
}

impl Read<'_> {
    /// A target: no call, nested in nothing.
    const TARGET: Read<'static> = Read {
        path: "",
        holder: Uuid::nil(),
        depth: 0,
    };

    fn deeper(self) -> Self {
        Self {
            depth: self.depth + 1,
            ..self
        }
    }
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

/// The value the literal `expression` writes, read as `expected`, nested
/// in `depth` expressions.
fn literal(expression: &Expr, expected: &Expected, depth: usize) -> Typed {
    let span = expression.span;
    if depth >= MAX_DEPTH {
        return Err((
            format!("expressions nest at most {MAX_DEPTH} levels deep"),
            span,
        ));
    }
    let mismatch = |what: &str, kind: &PrimitiveKind| {
        Err((
            format!("{what} is given to a parameter of type {kind}"),
            span,
        ))
    };
    match (&expression.kind, expected) {
        (ExprKind::None, Expected::Option(_) | Expected::Any) => Ok(Value::Option(None)),
        (ExprKind::Some { value }, Expected::Option(inner)) => Ok(Value::Option(Some(Box::new(
            literal(value, inner, depth + 1)?,
        )))),
        (ExprKind::Some { value }, Expected::Any) => Ok(Value::Option(Some(Box::new(literal(
            value,
            &Expected::Any,
            depth + 1,
        )?)))),
        (ExprKind::None | ExprKind::Some { .. }, Expected::Primitive(kind)) => {
            mismatch("an optional", kind)
        }
        // An optional parameter's value is framed when it is passed.
        (_, Expected::Option(inner)) => literal(expression, inner, depth),
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
            number(text, kind).map_err(|message| (message, span))
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
                (Some(kind), _) => array(kind, elements, depth),
                (None, Expected::Primitive(kind)) if kind.is_array() => {
                    array(*kind, elements, depth)
                }
                (None, Expected::Primitive(kind)) => mismatch("an array", kind),
                (None, _) => Ok(Value::ArrayValue(
                    elements
                        .iter()
                        .map(|element| literal(element, &Expected::Any, depth + 1))
                        .collect::<Result<_, _>>()?,
                )),
            }
        }
        (ExprKind::Uuid { value }, _) => Ok(Value::Uuid(*value)),
        (ExprKind::Json { text }, _) => serde_json::from_str(text)
            .map_err(|error| (format!("not the JSON form of a value: {error}"), span)),
        (
            ExprKind::Variable { .. }
            | ExprKind::VariableId { .. }
            | ExprKind::Port { .. }
            | ExprKind::Select { .. }
            | ExprKind::Call { .. },
            _,
        ) => Err((
            "`some(…)` and an array hold literals: a variable, a port or a call is an argument \
             of its own"
                .to_string(),
            span,
        )),
    }
}

/// `text` as a number of type `kind`. A finite text is a finite number: one
/// past the type's range is refused, not rounded to an infinity.
pub(crate) fn number(text: &str, kind: PrimitiveKind) -> Result<Value, String> {
    use PrimitiveKind::*;
    let not = || format!("`{text}` is no {kind}");
    let finite = !text.contains("inf") && !text.contains("NaN");
    let out_of_range = || format!("`{text}` is past the range of {kind}");
    Ok(match kind {
        U8 => Value::U8(text.parse().map_err(|_| not())?),
        U16 => Value::U16(text.parse().map_err(|_| not())?),
        U32 => Value::U32(text.parse().map_err(|_| not())?),
        U64 => Value::U64(text.parse().map_err(|_| not())?),
        I8 => Value::I8(text.parse().map_err(|_| not())?),
        I16 => Value::I16(text.parse().map_err(|_| not())?),
        I32 => Value::I32(text.parse().map_err(|_| not())?),
        I64 => Value::I64(text.parse().map_err(|_| not())?),
        F32 => {
            let value: f32 = text.parse().map_err(|_| not())?;
            if finite && !value.is_finite() {
                return Err(out_of_range());
            }
            Value::F32(value)
        }
        F64 => {
            let value: f64 = text.parse().map_err(|_| not())?;
            if finite && !value.is_finite() {
                return Err(out_of_range());
            }
            Value::F64(value)
        }
        _ => return Err(not()),
    })
}

/// The array of type `kind` holding `elements`, each read as its element
/// type.
fn array(kind: PrimitiveKind, elements: &[Expr], depth: usize) -> Typed {
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
        .map(|value| literal(value, &Expected::Primitive(element), depth + 1))
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
