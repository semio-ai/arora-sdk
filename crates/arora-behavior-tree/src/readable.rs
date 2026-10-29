//! Graphs and diffs whose references may be names: the form of the files
//! people write, read and review.
//!
//! A [`Graph`] and a [`GraphDiff`] here are the shared [`graph::Graph`] and
//! [`graph::GraphDiff`] field by field, with every reference a string: a
//! string that parses as a UUID is an id, any other a name. A node's name is
//! the id a `.bt` text gives the node of that name, a function's resolves as
//! in a text, and a port's is its function's parameter of that name, `_ret`
//! its return value:
//!
//! ```yaml
//! root: watch
//! nodes:
//! - id: watch
//!   function: sequence
//!   children: [near]
//! - id: near
//!   function: below
//!   bind:
//!     x: {port: {node: cm, port: _ret}}
//!     limit: {value: {f64: 30.0}}
//! - id: cm
//!   function: clamp
//!   bind:
//!     x: {key: sensor/distance}
//! ```
//!
//! [`Graph::resolve`] and [`GraphDiff::resolve`] read a file, by names, by
//! ids or both, into the shared model; [`Graph::from_graph`] and
//! [`GraphDiff::from_diff`] write the shared model with a name wherever one
//! resolves back to the same id. An interpreter takes the shared model,
//! whose references are ids. `docs/text.md` describes the resolution, and
//! says which form to use where.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, HashMap};

use arora_behavior::graph;
use arora_behavior::{FunctionIndex, ModuleFunction};
use arora_types::data::Key;
use arora_types::value::Value;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::display::Names;
use crate::schema::_RET_PARAM_ID;
use crate::text::{native, node_id, FunctionName, Functions, Name, RETURN};

/// A port on a node, by references: see [`graph::Port`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Port {
    pub node: String,
    pub port: String,
}

/// What a port is bound to, by references: see [`graph::Source`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Value(Value),
    Key(String),
    Port(Port),
    Select { source: Box<Source>, path: Key },
}

/// A node, by references: see [`graph::Node`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub function: String,
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        with = "serde_yaml::with::singleton_map_recursive"
    )]
    pub bind: BTreeMap<String, Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<String>>,
}

/// A graph, by references: see [`graph::Graph`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<Node>,
}

/// One binding of a diff, by references: see [`graph::Binding`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub node: String,
    pub port: String,
    #[serde(with = "serde_yaml::with::singleton_map_recursive")]
    pub source: Source,
}

/// A diff, by references: see [`graph::GraphDiff`].
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GraphDiff {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub set: Vec<Node>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unbind: Vec<Port>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bind: Vec<Binding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
}

/// A reference that resolves to no id, and where the file writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceError {
    /// Its place in the file, as `nodes[1].bind.lo` or
    /// `bind[0].source.port.node`.
    pub at: String,
    /// Why it resolves to none.
    pub message: String,
}

impl std::fmt::Display for ReferenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.at, self.message)
    }
}

impl std::error::Error for ReferenceError {}

impl Graph {
    /// The graph this file describes, its names resolved against
    /// `functions` and the natives; or every reference that does not
    /// resolve.
    pub fn resolve(&self, functions: &FunctionIndex) -> Result<graph::Graph, Vec<ReferenceError>> {
        let mut resolver = Resolver::new(functions);
        let ids = resolver.heads(&self.nodes, "nodes");
        let nodes = self
            .nodes
            .iter()
            .zip(ids)
            .enumerate()
            .map(|(at, (node, head))| resolver.node(node, head, &format!("nodes[{at}]")))
            .collect();
        let root = self
            .root
            .as_ref()
            .and_then(|root| resolver.node_reference(root, "root"));
        resolver.done(graph::Graph { root, nodes })
    }

    /// `graph` written with a name for every reference that has one
    /// resolving back to the same id, and its id otherwise: nodes by
    /// `names.nodes`, functions and ports by `names.functions` and the
    /// natives.
    pub fn from_graph(graph: &graph::Graph, names: &Names) -> Self {
        let empty = FunctionIndex::new();
        let index = names.functions.unwrap_or(&empty);
        let writer = Writer::new(index, names, scope(&graph.nodes));
        Self {
            root: graph.root.map(|root| writer.node_reference(root)),
            nodes: graph.nodes.iter().map(|node| writer.node(node)).collect(),
        }
    }
}

impl GraphDiff {
    /// The diff this file describes, its names resolved against `functions`
    /// and the natives, and against `base`, the graph it is applied to, for
    /// the nodes it names and does not set; or every reference that does not
    /// resolve.
    pub fn resolve(
        &self,
        base: &graph::Graph,
        functions: &FunctionIndex,
    ) -> Result<graph::GraphDiff, Vec<ReferenceError>> {
        let mut resolver = Resolver::new(functions);
        resolver.nodes = scope(&base.nodes);
        let remove: Vec<Uuid> = self
            .remove
            .iter()
            .enumerate()
            .filter_map(|(at, node)| resolver.node_reference(node, &format!("remove[{at}]")))
            .collect();
        for id in &remove {
            resolver.nodes.remove(id);
        }
        let heads = resolver.heads(&self.set, "set");
        let set = self
            .set
            .iter()
            .zip(heads)
            .enumerate()
            .map(|(at, (node, head))| resolver.node(node, head, &format!("set[{at}]")))
            .collect();
        let unbind = self
            .unbind
            .iter()
            .enumerate()
            .filter_map(|(at, port)| resolver.port(port, &format!("unbind[{at}]")))
            .collect();
        let bind = self
            .bind
            .iter()
            .enumerate()
            .filter_map(|(at, binding)| {
                let at = format!("bind[{at}]");
                let port = resolver.port(
                    &Port {
                        node: binding.node.clone(),
                        port: binding.port.clone(),
                    },
                    &at,
                );
                let source = resolver.source(&binding.source, &format!("{at}.source"));
                Some(graph::Binding {
                    node: port?.node,
                    port: port?.port,
                    source: source?,
                })
            })
            .collect();
        let root = self
            .root
            .as_ref()
            .and_then(|root| resolver.node_reference(root, "root"));
        resolver.done(graph::GraphDiff {
            remove,
            set,
            unbind,
            bind,
            root,
        })
    }

    /// `diff` written with a name for every reference that has one
    /// resolving back to the same id, and its id otherwise, `base` being
    /// the graph it is applied to: see [`Graph::from_graph`].
    pub fn from_diff(diff: &graph::GraphDiff, base: &graph::Graph, names: &Names) -> Self {
        let empty = FunctionIndex::new();
        let index = names.functions.unwrap_or(&empty);
        let mut writer = Writer::new(index, names, scope(&base.nodes));
        let remove = diff
            .remove
            .iter()
            .map(|id| writer.node_reference(*id))
            .collect();
        for id in &diff.remove {
            writer.nodes.remove(id);
        }
        writer.nodes.extend(scope(&diff.set));
        Self {
            remove,
            set: diff.set.iter().map(|node| writer.node(node)).collect(),
            unbind: diff.unbind.iter().map(|port| writer.port(port)).collect(),
            bind: diff
                .bind
                .iter()
                .map(|binding| {
                    let port = writer.port(&graph::Port::new(binding.node, binding.port));
                    Binding {
                        node: port.node,
                        port: port.port,
                        source: writer.source(&binding.source),
                    }
                })
                .collect(),
            root: diff.root.map(|root| writer.node_reference(root)),
        }
    }
}

/// The nodes a reference may name, with the function each calls: `None`
/// for a node whose function did not resolve.
type Scope = HashMap<Uuid, Option<Uuid>>;

fn scope(nodes: &[graph::Node]) -> Scope {
    nodes
        .iter()
        .map(|node| (node.id, Some(node.function)))
        .collect()
}

/// The function of id `id`: a native, or a function of the index.
fn known(functions: &FunctionIndex, id: Uuid) -> Option<&ModuleFunction> {
    native(id)
        .map(|native| &native.function)
        .or_else(|| functions.get(&id))
}

/// The node `text` names in `scope`: an id as it is, or the node of that
/// name.
fn node_named(scope: &Scope, text: &str) -> Result<Uuid, String> {
    if let Ok(id) = Uuid::parse_str(text) {
        return Ok(id);
    }
    let id = node_id(text);
    if scope.contains_key(&id) {
        Ok(id)
    } else {
        Err(format!("no node is named `{text}`"))
    }
}

/// The function `text` names: an id as it is, or as a `.bt` text names it.
fn function_named(functions: &Functions, text: &str) -> Result<Uuid, String> {
    let name = |text: &str| Name {
        span: Default::default(),
        text: text.to_string(),
    };
    let (module, function) = match text.split_once("::") {
        Some((module, function)) => (Some(name(module)), function),
        None => (None, text),
    };
    functions
        .find(&FunctionName {
            span: Default::default(),
            module,
            name: name(function),
        })
        .map(|found| found.id())
}

/// The port `text` names on a node calling `function`: an id as it is,
/// `_ret` the return value, or the parameter of that name.
fn port_named(function: Option<&ModuleFunction>, text: &str) -> Result<Uuid, String> {
    if let Ok(id) = Uuid::parse_str(text) {
        return Ok(id);
    }
    let Some(function) = function else {
        return Err(format!(
            "`{text}` names no port: the node's function is not known, \
             its ports are named by their ids"
        ));
    };
    let mut named = function
        .function
        .parameters
        .iter()
        .filter(|(_, parameter)| parameter.name == text)
        .map(|(id, _)| *id)
        .chain((text == RETURN).then_some(_RET_PARAM_ID));
    match (named.next(), named.next()) {
        (Some(id), None) => Ok(id),
        (Some(_), Some(_)) => Err(format!(
            "`{text}` names several ports of `{}`: the port is named by its id",
            function.function_name
        )),
        (None, _) => Err(format!(
            "`{}` has no parameter `{text}`",
            function.function_name
        )),
    }
}

/// Resolves a file's references, collecting those that do not resolve.
struct Resolver<'a> {
    index: &'a FunctionIndex,
    functions: Functions<'a>,
    nodes: Scope,
    errors: Vec<ReferenceError>,
}

impl<'a> Resolver<'a> {
    fn new(index: &'a FunctionIndex) -> Self {
        Self {
            index,
            functions: Functions::new(index),
            nodes: Scope::new(),
            errors: Vec::new(),
        }
    }

    fn check<T>(&mut self, result: Result<T, String>, at: &str) -> Option<T> {
        result
            .map_err(|message| {
                self.errors.push(ReferenceError {
                    at: at.to_string(),
                    message,
                })
            })
            .ok()
    }

    /// Each node's id and function, added to the nodes references may name.
    fn heads(&mut self, nodes: &[Node], field: &str) -> Vec<(Uuid, Option<Uuid>)> {
        let heads: Vec<_> = nodes
            .iter()
            .enumerate()
            .map(|(at, node)| {
                let function = function_named(&self.functions, &node.function);
                let function = self.check(function, &format!("{field}[{at}].function"));
                (node_id(&node.id), function)
            })
            .collect();
        self.nodes.extend(heads.iter().copied());
        heads
    }

    fn node(&mut self, node: &Node, (id, function): (Uuid, Option<Uuid>), at: &str) -> graph::Node {
        let signature = function.and_then(|function| known(self.index, function));
        let mut bind = BTreeMap::new();
        for (port, source) in &node.bind {
            let at = format!("{at}.bind.{port}");
            let source = self.source(source, &at);
            let Some(port) = function.and_then(|_| self.check(port_named(signature, port), &at))
            else {
                continue;
            };
            match bind.entry(port) {
                Entry::Occupied(_) => self.errors.push(ReferenceError {
                    at,
                    message: format!("port {port} is bound twice"),
                }),
                Entry::Vacant(vacant) => {
                    if let Some(source) = source {
                        vacant.insert(source);
                    }
                }
            }
        }
        let children = node.children.as_ref().map(|children| {
            children
                .iter()
                .enumerate()
                .filter_map(|(child, name)| {
                    self.node_reference(name, &format!("{at}.children[{child}]"))
                })
                .collect()
        });
        graph::Node {
            id,
            function: function.unwrap_or_default(),
            bind,
            children,
        }
    }

    fn node_reference(&mut self, text: &str, at: &str) -> Option<Uuid> {
        let found = node_named(&self.nodes, text);
        self.check(found, at)
    }

    fn port(&mut self, port: &Port, at: &str) -> Option<graph::Port> {
        let node = self.node_reference(&port.node, &format!("{at}.node"))?;
        let function = match self.nodes.get(&node) {
            Some(None) => return None,
            Some(Some(function)) => known(self.index, *function),
            None => None,
        };
        let found = port_named(function, &port.port);
        let port = self.check(found, &format!("{at}.port"))?;
        Some(graph::Port::new(node, port))
    }

    fn source(&mut self, source: &Source, at: &str) -> Option<graph::Source> {
        Some(match source {
            Source::Value(value) => graph::Source::Value(value.clone()),
            Source::Key(key) => graph::Source::Key(key.clone()),
            Source::Port(port) => graph::Source::Port(self.port(port, &format!("{at}.port"))?),
            Source::Select { source, path } => graph::Source::Select {
                source: Box::new(self.source(source, &format!("{at}.select.source"))?),
                path: path.clone(),
            },
        })
    }

    fn done<T>(self, resolved: T) -> Result<T, Vec<ReferenceError>> {
        if self.errors.is_empty() {
            Ok(resolved)
        } else {
            Err(self.errors)
        }
    }
}

/// Writes references by name where the name resolves back to the same id.
struct Writer<'a> {
    index: &'a FunctionIndex,
    functions: Functions<'a>,
    names: &'a Names<'a>,
    nodes: Scope,
}

impl<'a> Writer<'a> {
    fn new(index: &'a FunctionIndex, names: &'a Names<'a>, nodes: Scope) -> Self {
        Self {
            index,
            functions: Functions::new(index),
            names,
            nodes,
        }
    }

    /// The node's name, if it has one naming it and nothing else.
    fn node_name(&self, id: Uuid) -> Option<String> {
        self.names
            .nodes
            .and_then(|nodes| nodes.get(&id))
            .filter(|name| Uuid::parse_str(name).is_err() && node_id(name) == id)
            .cloned()
    }

    fn node_reference(&self, id: Uuid) -> String {
        self.node_name(id)
            .filter(|name| node_named(&self.nodes, name) == Ok(id))
            .unwrap_or_else(|| id.to_string())
    }

    fn function(&self, id: Uuid) -> String {
        known(self.index, id)
            .map(|function| self.names.head(function))
            .filter(|head| function_named(&self.functions, head) == Ok(id))
            .unwrap_or_else(|| id.to_string())
    }

    fn port_name(&self, function: Option<Uuid>, id: Uuid) -> String {
        let function = function.and_then(|function| known(self.index, function));
        let name = match function {
            Some(_) if id == _RET_PARAM_ID => Some(RETURN.to_string()),
            Some(function) => function
                .function
                .parameters
                .get(&id)
                .map(|parameter| parameter.name.clone()),
            None => None,
        };
        name.filter(|name| port_named(function, name) == Ok(id))
            .unwrap_or_else(|| id.to_string())
    }

    fn node(&self, node: &graph::Node) -> Node {
        Node {
            id: self
                .node_name(node.id)
                .unwrap_or_else(|| node.id.to_string()),
            function: self.function(node.function),
            bind: node
                .bind
                .iter()
                .map(|(port, source)| {
                    (
                        self.port_name(Some(node.function), *port),
                        self.source(source),
                    )
                })
                .collect(),
            children: node.children.as_ref().map(|children| {
                children
                    .iter()
                    .map(|child| self.node_reference(*child))
                    .collect()
            }),
        }
    }

    fn port(&self, port: &graph::Port) -> Port {
        let function = self.nodes.get(&port.node).copied().flatten();
        Port {
            node: self.node_reference(port.node),
            port: self.port_name(function, port.port),
        }
    }

    fn source(&self, source: &graph::Source) -> Source {
        match source {
            graph::Source::Value(value) => Source::Value(value.clone()),
            graph::Source::Key(key) => Source::Key(key.clone()),
            graph::Source::Port(port) => Source::Port(self.port(port)),
            graph::Source::Select { source, path } => Source::Select {
                source: Box::new(self.source(source)),
                path: path.clone(),
            },
        }
    }
}

#[cfg(test)]
mod tests;
