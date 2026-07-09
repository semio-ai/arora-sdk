//! The shared behavior **graph model**: one serde-friendly data representation
//! every [`BehaviorInterpreter`](super::BehaviorInterpreter) reads.
//!
//! A behavior — a behavior tree, a node graph — is authored as a [`Graph`]: a
//! set of [`Node`]s, each bound to a **function** (a statically-known id the
//! interpreter handles natively, **or** a module call routed by that same id —
//! exactly how `arora-behavior-tree` already treats its builtins and module
//! actions homogeneously), with typed **inputs/outputs** ([`Io`]) and **links**
//! ([`Link`]) wiring an output/port of one node into the input of another.
//!
//! This module is *only* the data model. It does not tick anything and it does
//! not know how any particular interpreter walks the links — the behavior tree
//! reads them as argument/child edges; a node graph reads them as dataflow. Each
//! interpreter lowers this shared model into its own runtime form (see
//! `arora-behavior-tree`'s `graph` module for the tree lowering).
//!
//! Edition happens through [`GraphDiff`]: a set of node/link additions and
//! removals (plus predetermined-key overrides). "Loading" a behavior is just
//! [`Graph::apply`]ing a diff onto an [`Graph::empty`] graph — which is why
//! [`BehaviorInterpreter::apply`](super::BehaviorInterpreter::apply) is the one
//! edition entry point.

use std::collections::HashMap;

use arora_types::module::high::TypeRef;
use arora_types::value::Value;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Whether an [`Io`] is a node **input** (a sink the interpreter feeds) or an
/// **output** (a source it can link elsewhere).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// A value the node consumes — the target end of a [`Link`].
    Input,
    /// A value the node produces — a source a [`Link`] can read from.
    Output,
}

/// A typed input or output on a [`Node`].
///
/// `id` matches the function's parameter id (for an input) or is the node's
/// output slot id (a return is conventionally the function id itself). `ty` is
/// the arora **`Value` type** of the slot, taken from the frozen `Function`
/// record's signature when known; `None` means "leave it to the interpreter to
/// derive from the function record at build time".
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Io {
    /// The slot id: a parameter id for an input, an output slot id for an output.
    pub id: Uuid,
    /// Whether this slot is consumed (input) or produced (output).
    pub direction: Direction,
    /// The slot's arora `Value` type, when known from the function signature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<TypeRef>,
    /// An optional **predetermined key**: the slot's default store binding (an
    /// animation track's authored key, a sink node's path). The interpreter
    /// binds the slot to this key unless a [`Link`] overrides it. Carried here
    /// per proposal §3.6; the full predetermined-I/O semantics land in a later
    /// pass, but the field travels with the model now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predetermined_key: Option<String>,
}

impl Io {
    /// A bare input slot with no type or predetermined key.
    pub fn input(id: Uuid) -> Self {
        Self {
            id,
            direction: Direction::Input,
            ty: None,
            predetermined_key: None,
        }
    }

    /// A bare output slot with no type or predetermined key.
    pub fn output(id: Uuid) -> Self {
        Self {
            id,
            direction: Direction::Output,
            ty: None,
            predetermined_key: None,
        }
    }
}

/// A reference to one slot on one node: `(node, port)`. Generalizes the behavior
/// tree's `NodeParameterId`.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Port {
    /// The node the slot belongs to.
    pub node: Uuid,
    /// The slot id on that node (an [`Io::id`]).
    pub port: Uuid,
}

impl Port {
    /// A `(node, port)` reference.
    pub fn new(node: Uuid, port: Uuid) -> Self {
        Self { node, port }
    }
}

/// Where the value feeding a [`Link`]'s target comes from.
///
/// Generalizes the behavior tree's `Expression` link kinds (a literal value, a
/// shared blackboard variable, or another node's slot). Full expression links
/// (arithmetic over sources) are deferred — proposal Q-D.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LinkSource {
    /// A constant value.
    Literal(Value),
    /// A shared blackboard variable, by id — the interpreter resolves it to a
    /// store slot (or a tree-local cell) at build time.
    Variable(Uuid),
    /// Another node's slot: the output/port the link reads from. Generalizes
    /// `Expression::NodeArgument`.
    Port(Port),
}

/// A directed wire feeding one node input from a [`LinkSource`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Link {
    /// The input slot being fed. At most one link targets a given port.
    pub target: Port,
    /// What feeds it.
    pub source: LinkSource,
}

impl Link {
    /// A link feeding `target` from `source`.
    pub fn new(target: Port, source: LinkSource) -> Self {
        Self { target, source }
    }
}

/// A node bound to a function, with its typed inputs/outputs and (optionally)
/// ordered children.
///
/// One node kind, routed by `function`: an interpreter dispatches natively for
/// the ids it knows and calls a module for the rest — the same homogeneous split
/// the behavior tree already makes.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Node {
    /// This node's id.
    pub id: Uuid,
    /// The function bound to this node: a statically-known id **or** a module
    /// function id. The interpreter routes on it.
    pub function: Uuid,
    /// Declared inputs.
    #[serde(default)]
    pub inputs: Vec<Io>,
    /// Declared outputs.
    #[serde(default)]
    pub outputs: Vec<Io>,
    /// Ordered children, for interpreters (like the tree) whose structure is a
    /// child relation. `None` for a leaf.
    #[serde(default)]
    pub children: Option<Vec<Uuid>>,
}

/// The shared behavior graph: nodes, the links between their slots, the named
/// blackboard variables, and (for tree-shaped interpreters) the root node.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Graph {
    /// The entry node, for interpreters that need one (a behavior tree's root).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<Uuid>,
    /// All nodes, indexed by id.
    #[serde(default)]
    pub nodes: HashMap<Uuid, Node>,
    /// The links wiring node slots together.
    #[serde(default)]
    pub links: Vec<Link>,
    /// Named blackboard variables (`id -> name`); a [`LinkSource::Variable`]
    /// refers to one of these, and the name is what an interpreter resolves
    /// against the data store.
    #[serde(default)]
    pub variables: HashMap<Uuid, String>,
}

/// A graph edition failed to apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphError {
    /// Human-readable description.
    pub message: String,
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for GraphError {}

/// An edit to a [`Graph`]: what to add and remove.
///
/// Applied in a fixed order (removals of links, then nodes; additions of nodes,
/// then links; then predetermined-key and root/variable settings) so a single
/// diff can both delete and rebuild a region. Loading a fresh behavior is
/// [`Graph::apply`]ing a diff whose `add_nodes`/`add_links` describe the whole
/// graph onto an [`Graph::empty`] graph.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct GraphDiff {
    /// Nodes to insert (replacing any node with the same id).
    #[serde(default)]
    pub add_nodes: Vec<Node>,
    /// Node ids to remove. Links touching a removed node are dropped too.
    #[serde(default)]
    pub remove_nodes: Vec<Uuid>,
    /// Links to insert. Adding a link whose `target` already has one replaces it
    /// (at most one link per input).
    #[serde(default)]
    pub add_links: Vec<Link>,
    /// Link targets to unwire.
    #[serde(default)]
    pub remove_links: Vec<Port>,
    /// Predetermined-key overrides: set (`Some`) or clear (`None`) the
    /// [`Io::predetermined_key`] of the slot at each [`Port`].
    #[serde(default)]
    pub set_predetermined: Vec<(Port, Option<String>)>,
    /// If set, becomes the graph's [`root`](Graph::root).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_root: Option<Uuid>,
    /// Named variables to declare/rename (`id -> name`), merged in.
    #[serde(default)]
    pub variables: HashMap<Uuid, String>,
}

impl GraphDiff {
    /// A diff that loads `graph` wholesale: every node and link as additions,
    /// carrying its root and variables. `apply`ing this onto [`Graph::empty`]
    /// reproduces `graph`.
    pub fn load(graph: Graph) -> Self {
        let mut add_nodes: Vec<Node> = graph.nodes.into_values().collect();
        // Root first keeps interpreters that treat node order as significant
        // (the behavior tree takes the first node as the root) well-defined.
        if let Some(root) = graph.root {
            add_nodes.sort_by_key(|n| n.id != root);
        }
        Self {
            add_nodes,
            add_links: graph.links,
            set_root: graph.root,
            variables: graph.variables,
            ..Self::default()
        }
    }
}

impl Graph {
    /// An empty graph — the starting point a load diff is applied onto.
    pub fn empty() -> Self {
        Self::default()
    }

    /// The node with id `id`, if present.
    pub fn node(&self, id: &Uuid) -> Option<&Node> {
        self.nodes.get(id)
    }

    /// The link feeding `target`, if any.
    pub fn link_to(&self, target: &Port) -> Option<&Link> {
        self.links.iter().find(|l| &l.target == target)
    }

    /// Apply `diff` in place.
    pub fn apply(&mut self, diff: GraphDiff) -> Result<(), GraphError> {
        // 1. Remove links, then nodes (and any links still touching them).
        for target in &diff.remove_links {
            self.links.retain(|l| &l.target != target);
        }
        for id in &diff.remove_nodes {
            self.nodes.remove(id);
            self.links
                .retain(|l| &l.target.node != id && !links_source_is_node(&l.source, id));
            if self.root == Some(*id) {
                self.root = None;
            }
        }

        // 2. Add nodes, then links (replacing any link on the same target).
        for node in diff.add_nodes {
            self.nodes.insert(node.id, node);
        }
        for link in diff.add_links {
            self.links.retain(|l| l.target != link.target);
            self.links.push(link);
        }

        // 3. Predetermined-key overrides.
        for (port, key) in diff.set_predetermined {
            let node = self.nodes.get_mut(&port.node).ok_or_else(|| GraphError {
                message: format!("predetermined key targets unknown node {}", port.node),
            })?;
            let io = node
                .inputs
                .iter_mut()
                .chain(node.outputs.iter_mut())
                .find(|io| io.id == port.port)
                .ok_or_else(|| GraphError {
                    message: format!(
                        "predetermined key targets unknown slot {} on node {}",
                        port.port, port.node
                    ),
                })?;
            io.predetermined_key = key;
        }

        // 4. Root and variables.
        if let Some(root) = diff.set_root {
            self.root = Some(root);
        }
        self.variables.extend(diff.variables);
        Ok(())
    }
}

/// Whether a [`LinkSource`] reads from node `id` (so the link dangles once that
/// node is removed).
fn links_source_is_node(source: &LinkSource, id: &Uuid) -> bool {
    matches!(source, LinkSource::Port(p) if &p.node == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: Uuid, function: Uuid) -> Node {
        Node {
            id,
            function,
            ..Node::default()
        }
    }

    #[test]
    fn load_onto_empty_reproduces_the_graph() {
        let a = Uuid::from_u128(0xA);
        let b = Uuid::from_u128(0xB);
        let mut graph = Graph::empty();
        graph.nodes.insert(a, node(a, Uuid::from_u128(0xF1)));
        graph.nodes.insert(b, node(b, Uuid::from_u128(0xF2)));
        graph.links.push(Link::new(
            Port::new(b, Uuid::from_u128(0x1)),
            LinkSource::Port(Port::new(a, Uuid::from_u128(0x2))),
        ));
        graph.root = Some(a);
        graph
            .variables
            .insert(Uuid::from_u128(0x9), "battery".into());

        let mut rebuilt = Graph::empty();
        rebuilt.apply(GraphDiff::load(graph.clone())).unwrap();
        assert_eq!(rebuilt, graph);
    }

    #[test]
    fn load_diff_lists_the_root_node_first() {
        let a = Uuid::from_u128(0xA);
        let b = Uuid::from_u128(0xB);
        let c = Uuid::from_u128(0xC);
        let mut graph = Graph::empty();
        for id in [a, b, c] {
            graph.nodes.insert(id, node(id, Uuid::from_u128(0xF0)));
        }
        graph.root = Some(c);
        let diff = GraphDiff::load(graph);
        assert_eq!(diff.add_nodes.first().unwrap().id, c);
    }

    #[test]
    fn removing_a_node_drops_links_touching_it() {
        let a = Uuid::from_u128(0xA);
        let b = Uuid::from_u128(0xB);
        let mut graph = Graph::empty();
        graph.nodes.insert(a, node(a, Uuid::from_u128(0xF1)));
        graph.nodes.insert(b, node(b, Uuid::from_u128(0xF2)));
        // a's input is fed from b's output; removing b must drop the link.
        graph.links.push(Link::new(
            Port::new(a, Uuid::from_u128(0x1)),
            LinkSource::Port(Port::new(b, Uuid::from_u128(0x2))),
        ));

        graph
            .apply(GraphDiff {
                remove_nodes: vec![b],
                ..GraphDiff::default()
            })
            .unwrap();
        assert!(!graph.nodes.contains_key(&b));
        assert!(graph.links.is_empty(), "dangling link dropped");
    }

    #[test]
    fn adding_a_link_replaces_the_one_on_the_same_target() {
        let a = Uuid::from_u128(0xA);
        let target = Port::new(a, Uuid::from_u128(0x1));
        let mut graph = Graph::empty();
        graph.nodes.insert(a, node(a, Uuid::from_u128(0xF1)));
        graph
            .apply(GraphDiff {
                add_links: vec![Link::new(target, LinkSource::Literal(Value::U8(1)))],
                ..GraphDiff::default()
            })
            .unwrap();
        graph
            .apply(GraphDiff {
                add_links: vec![Link::new(target, LinkSource::Literal(Value::U8(2)))],
                ..GraphDiff::default()
            })
            .unwrap();
        assert_eq!(graph.links.len(), 1);
        assert_eq!(
            graph.link_to(&target).unwrap().source,
            LinkSource::Literal(Value::U8(2))
        );
    }

    #[test]
    fn set_predetermined_key_overrides_the_slot() {
        let a = Uuid::from_u128(0xA);
        let slot = Uuid::from_u128(0x1);
        let mut graph = Graph::empty();
        graph.nodes.insert(
            a,
            Node {
                id: a,
                function: Uuid::from_u128(0xF1),
                inputs: vec![Io::input(slot)],
                ..Node::default()
            },
        );
        graph
            .apply(GraphDiff {
                set_predetermined: vec![(Port::new(a, slot), Some("head/pitch".into()))],
                ..GraphDiff::default()
            })
            .unwrap();
        assert_eq!(
            graph.nodes[&a].inputs[0].predetermined_key.as_deref(),
            Some("head/pitch")
        );
    }

    #[test]
    fn predetermined_key_on_unknown_slot_is_an_error() {
        let a = Uuid::from_u128(0xA);
        let mut graph = Graph::empty();
        graph.nodes.insert(a, node(a, Uuid::from_u128(0xF1)));
        let err = graph.apply(GraphDiff {
            set_predetermined: vec![(Port::new(a, Uuid::from_u128(0x2)), Some("k".into()))],
            ..GraphDiff::default()
        });
        assert!(err.is_err());
    }
}
