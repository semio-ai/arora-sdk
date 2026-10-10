//! The shared behavior **graph model**: one serde-friendly data representation
//! every [`BehaviorInterpreter`](super::BehaviorInterpreter) reads.
//!
//! A behavior — a behavior tree, a node graph — is authored as a [`Graph`]: a
//! set of [`Node`]s, each bound to a **function** (a statically-known id the
//! interpreter handles natively, **or** a module call routed by that same id —
//! exactly how `arora-behavior-tree` already treats its builtins and module
//! actions homogeneously), with typed slots ([`Io`] — inputs and outputs, told apart by
//! which [`Node`] list holds them) and **links**
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

use std::collections::{HashMap, HashSet};

use arora_types::data::Key;
use arora_types::module::high::TypeRef;
use arora_types::value::Value;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A typed slot on a [`Node`] — one entry of [`Node::inputs`] or
/// [`Node::outputs`]. **Which of the two lists holds it is what makes it an
/// input or an output**; the slot itself carries no direction, so the two can
/// never disagree.
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
    /// A bare slot with no type or predetermined key.
    pub fn new(id: Uuid) -> Self {
        Self {
            id,
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
    /// Read a [`Key`] sub-path of another source's value on read — a structured
    /// output's field (by id) or an array element (by index). Selection is an
    /// *operation over any source*, not a property of one: `Select { source:
    /// Variable(id), .. }` selects into a variable, and nested `Select`s chain.
    /// The path segments are resolved **ids**, not names (the type registry
    /// maps authored field names to ids when a graph is built), so the runtime
    /// resolves nothing. See [`Key::select`]. Heavier "compute a value"
    /// operations (calls, arithmetic) are nodes, not link sources.
    Select {
        /// The source whose value is projected.
        source: Box<LinkSource>,
        /// The sub-path (`Key` attributes = ids) read out of that value.
        path: Key,
    },
}

/// The source `Port` a link ultimately reads from, through any [`LinkSource::Select`]
/// wrappers; `None` for a literal or variable source.
pub fn source_port(source: &LinkSource) -> Option<Port> {
    match source {
        LinkSource::Port(port) => Some(*port),
        LinkSource::Select { source, .. } => source_port(source),
        _ => None,
    }
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

/// The language a [`Graph`] is written in, and the version of that language's
/// format it is written for.
///
/// Every language uses the one [`Graph`] model; the type says how its nodes,
/// links and structure are read. The version is the format's, not a crate's:
/// a minor version adds to the format (a new node kind), a major changes what
/// an existing graph means. A graph states the oldest version that has
/// everything it uses, so an interpreter reading version `1.3` of a type runs
/// any `1.x` graph with `x ≤ 3` ([`GraphType::reads`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GraphType {
    /// The language's name, e.g. `behavior-tree`.
    pub name: String,
    /// The version of the language's format.
    pub version: semver::Version,
}

impl GraphType {
    /// Whether an interpreter reading `self` runs a graph written for
    /// `graph`: the same name and major version, and a minor version no newer
    /// than `self`'s.
    pub fn reads(&self, graph: &GraphType) -> bool {
        self.name == graph.name
            && self.version.major == graph.version.major
            && graph.version.minor <= self.version.minor
    }
}

impl std::fmt::Display for GraphType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.name, self.version)
    }
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
        // One pass over the links for the whole diff, not one per removal:
        // the cost of an edit follows the edit, not the size of the graph.
        let removed_targets: HashSet<Port> = diff.remove_links.iter().copied().collect();
        let removed_nodes: HashSet<Uuid> = diff.remove_nodes.iter().copied().collect();
        for id in &removed_nodes {
            self.nodes.remove(id);
        }
        if self.root.is_some_and(|root| removed_nodes.contains(&root)) {
            self.root = None;
        }
        if !removed_targets.is_empty() || !removed_nodes.is_empty() {
            self.links.retain(|l| {
                !removed_targets.contains(&l.target)
                    && !removed_nodes.contains(&l.target.node)
                    && !source_port(&l.source)
                        .is_some_and(|port| removed_nodes.contains(&port.node))
            });
        }

        // 2. Add nodes, then links: a link replaces any on the same target,
        // the diff's own included, so of several added for one target the
        // last stands.
        for node in diff.add_nodes {
            self.nodes.insert(node.id, node);
        }
        let last_for_target: HashMap<Port, usize> = diff
            .add_links
            .iter()
            .enumerate()
            .map(|(idx, link)| (link.target, idx))
            .collect();
        if !last_for_target.is_empty() {
            self.links
                .retain(|l| !last_for_target.contains_key(&l.target));
            self.links.extend(
                diff.add_links
                    .into_iter()
                    .enumerate()
                    .filter(|(idx, link)| last_for_target[&link.target] == *idx)
                    .map(|(_, link)| link),
            );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_graph_type_reads_older_minors_of_its_major() {
        let ty = |name: &str, major, minor| GraphType {
            name: name.to_string(),
            version: semver::Version::new(major, minor, 0),
        };
        let reader = ty("behavior-tree", 1, 1);
        assert!(reader.reads(&ty("behavior-tree", 1, 0)));
        assert!(reader.reads(&ty("behavior-tree", 1, 1)));
        assert!(!reader.reads(&ty("behavior-tree", 1, 2)), "a newer minor");
        assert!(!reader.reads(&ty("behavior-tree", 2, 0)), "another major");
        assert!(!reader.reads(&ty("node-graph", 1, 0)), "another language");
        assert_eq!(reader.to_string(), "behavior-tree 1.1.0");
    }

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

    /// The one-removal-at-a-time application the single passes replace:
    /// what `apply` must keep doing to links, root and nodes.
    fn apply_one_at_a_time(graph: &mut Graph, diff: GraphDiff) {
        for target in &diff.remove_links {
            graph.links.retain(|l| &l.target != target);
        }
        for id in &diff.remove_nodes {
            graph.nodes.remove(id);
            graph.links.retain(|l| {
                &l.target.node != id && !source_port(&l.source).is_some_and(|p| &p.node == id)
            });
            if graph.root == Some(*id) {
                graph.root = None;
            }
        }
        for node in diff.add_nodes {
            graph.nodes.insert(node.id, node);
        }
        for link in diff.add_links {
            graph.links.retain(|l| l.target != link.target);
            graph.links.push(link);
        }
    }

    /// One diff removing links and nodes — the root among them, a node some
    /// links read from — and adding several links, two on one target and
    /// one replacing an existing link: the result, link order included, is
    /// what applying it one change at a time gives.
    #[test]
    fn a_diff_applies_as_its_changes_one_at_a_time() {
        let id = |n: u128| Uuid::from_u128(n);
        let port = |n: u128, p: u128| Port::new(id(n), id(p));
        let from = |n: u128| LinkSource::Port(port(n, 0xF));
        let mut graph = Graph::empty();
        for n in 1..=5 {
            graph.nodes.insert(id(n), node(id(n), id(0xF1)));
        }
        graph.root = Some(id(2));
        graph.links = vec![
            Link::new(port(1, 1), from(2)),
            Link::new(port(3, 1), from(1)),
            Link::new(port(3, 2), from(4)),
            Link::new(port(4, 1), LinkSource::Literal(Value::U8(1))),
            Link::new(port(5, 1), from(2)),
            Link::new(port(5, 2), from(3)),
        ];
        let diff = GraphDiff {
            remove_links: vec![port(3, 2)],
            remove_nodes: vec![id(2)],
            add_nodes: vec![node(id(6), id(0xF2))],
            add_links: vec![
                Link::new(port(6, 1), LinkSource::Literal(Value::U8(2))),
                Link::new(port(4, 1), from(6)),
                Link::new(port(6, 1), LinkSource::Literal(Value::U8(3))),
            ],
            ..GraphDiff::default()
        };
        let mut expected = graph.clone();
        apply_one_at_a_time(&mut expected, diff.clone());
        graph.apply(diff).unwrap();
        assert_eq!(graph, expected);
        assert_eq!(graph.root, None);
        assert_eq!(
            graph.link_to(&port(6, 1)).unwrap().source,
            LinkSource::Literal(Value::U8(3))
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
                inputs: vec![Io::new(slot)],
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
