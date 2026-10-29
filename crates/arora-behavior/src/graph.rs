//! The shared behavior **graph model**: one serde-friendly data representation
//! every [`BehaviorInterpreter`](super::BehaviorInterpreter) reads.
//!
//! A behavior — a behavior tree, a node graph — is authored as a [`Graph`]: a
//! list of [`Node`]s, each bound to a **function** (a statically-known id the
//! interpreter handles natively, **or** a module call routed by that same id),
//! each holding its own **bindings** ([`Node::bind`]): for each of its
//! function's ports, the [`Source`] that feeds an input, or where an output is
//! written. Which ports are inputs and which are outputs, and their types, is
//! the function signature's to say: the graph carries neither.
//!
//! This module is *only* the data model. It does not tick anything and it does
//! not know how any particular interpreter walks the bindings — the behavior
//! tree reads them as arguments and child edges; a node graph reads them as
//! dataflow. Each interpreter lowers this shared model into its own runtime
//! form (see `arora-behavior-tree`'s `graph` module for the tree lowering).
//!
//! Edition happens through [`GraphDiff`]: whole nodes set or removed, and
//! single bindings set or unset. "Loading" a behavior is [`Graph::apply`]ing
//! the diff from an [`Graph::empty`] graph to it, which is why
//! [`BehaviorInterpreter::apply`](super::BehaviorInterpreter::apply) is the one
//! edition entry point. [`Graph::diff`] gives the diff between two graphs.
//!
//! # Serialized forms
//!
//! A graph and a diff serialize leanly: an empty field is left out, and a
//! [`Source`] is a single-entry map naming its kind, so a graph reads the same
//! in JSON and in YAML:
//!
//! ```yaml
//! root: d2591ab9-…
//! nodes:
//! - id: d2591ab9-…
//!   function: 32246df6-…              # sequence
//!   children:
//!   - baa9bab8-…
//! - id: baa9bab8-…
//!   function: 31814ea5-…              # lessthan
//!   bind:
//!     5f726574-0000-4000-8000-000000000000:   # its return value…
//!       key: face/near                         # …written to a key
//!     784d2762-…:
//!       port:                                  # read from another node
//!         node: 46228f99-…
//!         port: 5f726574-0000-4000-8000-000000000000
//!     b7b9a190-…:
//!       value:
//!         f32: 30.0
//! ```

use std::collections::{BTreeMap, HashMap, HashSet};

use arora_types::data::Key;
use arora_types::value::Value;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A reference to one port on one node: `(node, port)`.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct Port {
    /// The node the port belongs to.
    pub node: Uuid,
    /// The port on that node: a parameter of its function, or its return
    /// value.
    pub port: Uuid,
}

impl Port {
    /// A `(node, port)` reference.
    pub fn new(node: Uuid, port: Uuid) -> Self {
        Self { node, port }
    }
}

/// What a port is bound to: for an input, what feeds it; for an output, where
/// it is written.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// A constant value.
    Value(Value),
    /// A store key, by its path. A key whose path starts with `.` is local:
    /// an interpreter keeps it to itself, and the store never sees it.
    Key(String),
    /// Another node's port.
    Port(Port),
    /// A [`Key`] sub-path of another source's value, read on read — a
    /// structured output's field (by id) or an array element (by index).
    /// Selection is an operation over any source: `Select { source: Key(..),
    /// .. }` selects into a key, and nested `Select`s chain. The path segments
    /// are resolved **ids**, not names, so the runtime resolves nothing. See
    /// [`Key::select`]. Heavier "compute a value" operations (calls,
    /// arithmetic) are nodes, not sources.
    Select {
        /// The source whose value is projected.
        source: Box<Source>,
        /// The sub-path (`Key` attributes = ids) read out of that value.
        path: Key,
    },
}

/// The port `source` ultimately reads, through any [`Source::Select`];
/// `None` for a value or a key.
pub fn source_port(source: &Source) -> Option<Port> {
    match source {
        Source::Port(port) => Some(*port),
        Source::Select { source, .. } => source_port(source),
        Source::Value(_) | Source::Key(_) => None,
    }
}

/// A node bound to a function, with its bindings and (optionally) ordered
/// children.
///
/// One node kind, routed by `function`: an interpreter dispatches natively for
/// the ids it knows and calls a module for the rest.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Node {
    /// This node's id, unique in its graph.
    pub id: Uuid,
    /// The function bound to this node: a statically-known id **or** a module
    /// function id. The interpreter routes on it.
    pub function: Uuid,
    /// Each bound port and its [`Source`]. A port the function reads is fed
    /// by its source; a port it writes is written to its source.
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        with = "serde_yaml::with::singleton_map_recursive"
    )]
    pub bind: BTreeMap<Uuid, Source>,
    /// Ordered children, for interpreters (like the tree) whose structure is a
    /// child relation. `None` for a leaf.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<Uuid>>,
}

/// The shared behavior graph: its nodes, each with its bindings, and (for
/// tree-shaped interpreters) the root node.
///
/// The nodes are a list with unique ids. Their order means nothing to an
/// interpreter: it is the order they serialize in, so a graph serializes the
/// same every time.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    /// The entry node, for interpreters that need one (a behavior tree's root).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<Uuid>,
    /// The nodes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<Node>,
}

/// The language a [`Graph`] is written in, and the version of that language's
/// format it is written for.
///
/// Every language uses the one [`Graph`] model; the type says how its nodes,
/// bindings and structure are read. The version is the format's, not a
/// crate's: a minor version adds to the format (a new node kind), a major
/// changes what an existing graph means. A graph states the oldest version
/// that has everything it uses, so an interpreter reading version `1.3` of a
/// type runs any `1.x` graph with `x ≤ 3` ([`GraphType::reads`]).
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

/// One port's binding, as a [`GraphDiff`] sets it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    /// The node holding the binding.
    pub node: Uuid,
    /// The port bound.
    pub port: Uuid,
    /// What the port is bound to.
    #[serde(with = "serde_yaml::with::singleton_map_recursive")]
    pub source: Source,
}

/// An edit to a [`Graph`]: whole nodes set or removed, single bindings set or
/// unset, and the root.
///
/// Applied in a fixed order, coarse to fine — removals, then whole nodes,
/// then single bindings unset and set, then the root — so one diff can both
/// delete and rebuild a region. Loading a behavior is applying
/// `Graph::empty().diff(&graph)`, which sets every node and the root.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GraphDiff {
    /// Nodes to remove. A binding reading a removed node goes with it, and so
    /// does the root when it is removed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove: Vec<Uuid>,
    /// Nodes to set, whole: each replaces the node with its id, in its place,
    /// or is added after the others.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub set: Vec<Node>,
    /// Ports to unbind. A port already unbound, or on no node, stays so.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unbind: Vec<Port>,
    /// Ports to bind, each replacing what its port was bound to. Of several
    /// for one port, the last stands.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bind: Vec<Binding>,
    /// If set, becomes the graph's [`root`](Graph::root).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<Uuid>,
}

impl Graph {
    /// An empty graph — the starting point a load is applied onto.
    pub fn empty() -> Self {
        Self::default()
    }

    /// The node with id `id`, if present.
    pub fn node(&self, id: &Uuid) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == *id)
    }

    /// Apply `diff` in place. A diff that binds a port on no node fails, and
    /// leaves the graph as it was.
    pub fn apply(&mut self, diff: GraphDiff) -> Result<(), GraphError> {
        let GraphDiff {
            remove,
            set,
            unbind,
            bind,
            root,
        } = diff;
        let removed: HashSet<Uuid> = remove.into_iter().collect();
        // A binding onto no node is refused before anything changes.
        if !bind.is_empty() {
            let present: HashSet<Uuid> = self
                .nodes
                .iter()
                .map(|node| node.id)
                .filter(|id| !removed.contains(id))
                .chain(set.iter().map(|node| node.id))
                .collect();
            if let Some(binding) = bind.iter().find(|binding| !present.contains(&binding.node)) {
                return Err(GraphError {
                    message: format!(
                        "a binding targets node {}, which is not in the graph",
                        binding.node
                    ),
                });
            }
        }

        // 1. Removals: the nodes, the bindings reading them, the root. One
        // pass over the nodes for the whole diff.
        if !removed.is_empty() {
            self.nodes.retain(|node| !removed.contains(&node.id));
            for node in &mut self.nodes {
                node.bind.retain(|_, source| {
                    !source_port(source).is_some_and(|port| removed.contains(&port.node))
                });
            }
            if self.root.is_some_and(|root| removed.contains(&root)) {
                self.root = None;
            }
        }

        // 2. Whole nodes, then single bindings, through one index of the
        // nodes by id.
        let mut at: HashMap<Uuid, usize> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.id, index))
            .collect();
        for node in set {
            match at.get(&node.id) {
                Some(index) => self.nodes[*index] = node,
                None => {
                    at.insert(node.id, self.nodes.len());
                    self.nodes.push(node);
                }
            }
        }
        for port in unbind {
            if let Some(index) = at.get(&port.node) {
                self.nodes[*index].bind.remove(&port.port);
            }
        }
        for binding in bind {
            let index = at[&binding.node];
            self.nodes[index].bind.insert(binding.port, binding.source);
        }

        // 3. The root.
        if let Some(root) = root {
            self.root = Some(root);
        }
        Ok(())
    }

    /// The diff that takes this graph to `target`: [`apply`](Self::apply)ing
    /// it to this graph gives `target`'s root and nodes, the nodes this graph
    /// has in its order and the others after them, in `target`'s order.
    ///
    /// It is minimal: a node only `target` has, or whose function or
    /// children change, is set whole; a node whose bindings alone change gets
    /// one entry per binding that changes, but for the bindings reading a
    /// removed node, which go with it. A diff has no entry that clears the
    /// root, and a removal is what unroots: a node this graph roots and
    /// `target` keeps unrooted is removed and set again, and the nodes reading
    /// it are bound again, so they restart with it.
    pub fn diff(&self, target: &Graph) -> GraphDiff {
        let theirs: HashMap<Uuid, &Node> =
            target.nodes.iter().map(|node| (node.id, node)).collect();
        let ours: HashMap<Uuid, &Node> = self.nodes.iter().map(|node| (node.id, node)).collect();
        // The root `target` drops: a node it keeps is removed and set again;
        // a root no node holds is removed, which unroots and removes nothing.
        let unrooted = self.root.filter(|root| {
            target.root.is_none() && ours.get(root).is_none_or(|_| theirs.contains_key(root))
        });
        let mut remove: Vec<Uuid> = self
            .nodes
            .iter()
            .map(|node| node.id)
            .filter(|id| !theirs.contains_key(id) || Some(*id) == unrooted)
            .collect();
        remove.extend(unrooted.filter(|root| !ours.contains_key(root)));
        let removed: HashSet<Uuid> = remove.iter().copied().collect();
        let mut diff = GraphDiff {
            remove,
            ..GraphDiff::default()
        };
        for node in &target.nodes {
            let Some(old) = ours.get(&node.id).filter(|_| !removed.contains(&node.id)) else {
                diff.set.push(node.clone());
                continue;
            };
            if old.function != node.function || old.children != node.children {
                diff.set.push(node.clone());
                continue;
            }
            // A binding reading a removed node goes with it: one `target`
            // keeps, to a node set again, is bound again.
            let reads_removed = |source: &Source| {
                source_port(source).is_some_and(|read| removed.contains(&read.node))
            };
            for (port, source) in &node.bind {
                if old.bind.get(port) != Some(source) || reads_removed(source) {
                    diff.bind.push(Binding {
                        node: node.id,
                        port: *port,
                        source: source.clone(),
                    });
                }
            }
            for (port, source) in &old.bind {
                if !node.bind.contains_key(port) && !reads_removed(source) {
                    diff.unbind.push(Port::new(node.id, *port));
                }
            }
        }
        // The root the removals leave, set again where `target` has another,
        // or has this one and a removal took it.
        let left = self.root.filter(|root| !removed.contains(root));
        if target.root.is_some() && target.root != left {
            diff.root = target.root;
        }
        diff
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

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn node(n: u128, function: u128) -> Node {
        Node {
            id: id(n),
            function: id(function),
            ..Node::default()
        }
    }

    fn bound(mut node: Node, port: u128, source: Source) -> Node {
        node.bind.insert(id(port), source);
        node
    }

    fn from(n: u128, port: u128) -> Source {
        Source::Port(Port::new(id(n), id(port)))
    }

    fn graph(root: Option<u128>, nodes: Vec<Node>) -> Graph {
        Graph {
            root: root.map(id),
            nodes,
        }
    }

    #[test]
    fn the_diff_from_empty_loads_the_graph() {
        let target = graph(
            Some(0xA),
            vec![
                Node {
                    children: Some(vec![id(0xB)]),
                    ..node(0xA, 0xF1)
                },
                bound(node(0xB, 0xF2), 0x1, from(0xA, 0x2)),
            ],
        );
        let diff = Graph::empty().diff(&target);
        assert_eq!(diff.set, target.nodes, "every node, whole, in order");
        assert_eq!(diff.root, target.root);
        let mut loaded = Graph::empty();
        loaded.apply(diff).unwrap();
        assert_eq!(loaded, target);
    }

    #[test]
    fn removing_a_node_drops_the_bindings_reading_it() {
        let mut g = graph(
            None,
            vec![
                bound(
                    bound(node(0xA, 0xF1), 0x1, from(0xB, 0x2)),
                    0x2,
                    Source::Select {
                        source: Box::new(from(0xB, 0x3)),
                        path: Key::new(".field"),
                    },
                ),
                node(0xB, 0xF2),
            ],
        );
        g.apply(GraphDiff {
            remove: vec![id(0xB)],
            ..GraphDiff::default()
        })
        .unwrap();
        assert_eq!(g, graph(None, vec![node(0xA, 0xF1)]));
    }

    #[test]
    fn removing_the_root_unroots_the_graph() {
        let mut g = graph(Some(0xA), vec![node(0xA, 0xF1)]);
        g.apply(GraphDiff {
            remove: vec![id(0xA)],
            set: vec![node(0xA, 0xF1)],
            ..GraphDiff::default()
        })
        .unwrap();
        assert_eq!(g, graph(None, vec![node(0xA, 0xF1)]), "set again, unrooted");
    }

    #[test]
    fn a_binding_replaces_what_its_port_was_bound_to() {
        let mut g = graph(
            None,
            vec![bound(node(0xA, 0xF1), 0x1, Source::Value(Value::U8(1)))],
        );
        let bind = |value| Binding {
            node: id(0xA),
            port: id(0x1),
            source: Source::Value(Value::U8(value)),
        };
        g.apply(GraphDiff {
            bind: vec![bind(2), bind(3)],
            ..GraphDiff::default()
        })
        .unwrap();
        assert_eq!(
            g.nodes[0].bind[&id(0x1)],
            Source::Value(Value::U8(3)),
            "the last stands"
        );
    }

    #[test]
    fn unbinding_wins_over_nothing_and_loses_to_a_binding() {
        let mut g = graph(
            None,
            vec![bound(node(0xA, 0xF1), 0x1, Source::Key("k".into()))],
        );
        g.apply(GraphDiff {
            unbind: vec![
                Port::new(id(0xA), id(0x1)),
                Port::new(id(0xA), id(0x2)),
                Port::new(id(0xC), id(0x1)),
            ],
            ..GraphDiff::default()
        })
        .unwrap();
        assert!(
            g.nodes[0].bind.is_empty(),
            "unbound; the others stay unbound"
        );
        g.apply(GraphDiff {
            unbind: vec![Port::new(id(0xA), id(0x1))],
            bind: vec![Binding {
                node: id(0xA),
                port: id(0x1),
                source: Source::Key("j".into()),
            }],
            ..GraphDiff::default()
        })
        .unwrap();
        assert_eq!(g.nodes[0].bind[&id(0x1)], Source::Key("j".into()));
    }

    #[test]
    fn a_binding_onto_no_node_is_refused_whole() {
        let mut g = graph(None, vec![node(0xA, 0xF1)]);
        let before = g.clone();
        let refused = g.apply(GraphDiff {
            remove: vec![id(0xA)],
            set: vec![node(0xB, 0xF2)],
            bind: vec![Binding {
                node: id(0xA),
                port: id(0x1),
                source: Source::Key("k".into()),
            }],
            ..GraphDiff::default()
        });
        assert!(refused.is_err(), "0xA is removed by the same diff");
        assert_eq!(g, before, "nothing applied");
    }

    #[test]
    fn a_set_node_replaces_in_place_and_a_new_one_goes_last() {
        let mut g = graph(None, vec![node(0xA, 0xF1), node(0xB, 0xF1)]);
        g.apply(GraphDiff {
            set: vec![node(0xC, 0xF3), node(0xA, 0xF2)],
            ..GraphDiff::default()
        })
        .unwrap();
        assert_eq!(
            g,
            graph(
                None,
                vec![node(0xA, 0xF2), node(0xB, 0xF1), node(0xC, 0xF3)]
            )
        );
    }

    /// Two graphs differing in every way a diff tells: the diff sets the
    /// nodes it must, binds and unbinds the rest, and takes the one to the
    /// other.
    #[test]
    fn a_diff_takes_a_graph_to_its_target() {
        let key = |k: &str| Source::Key(k.to_string());
        let value = |v| Source::Value(Value::F64(v));
        let a = graph(
            Some(0x1),
            vec![
                Node {
                    children: Some(vec![id(0x2), id(0x3)]),
                    ..node(0x1, 0xF0)
                },
                bound(bound(node(0x2, 0xF1), 0xA, value(1.0)), 0xB, key("in")),
                bound(node(0x3, 0xF1), 0xA, from(0x4, 0xC)),
                node(0x4, 0xF2),
                bound(node(0x5, 0xF3), 0xA, key("out")),
            ],
        );
        let b = graph(
            Some(0x1),
            vec![
                // Children reordered: set whole.
                Node {
                    children: Some(vec![id(0x3), id(0x2)]),
                    ..node(0x1, 0xF0)
                },
                // One binding changed, one removed, one added.
                bound(bound(node(0x2, 0xF1), 0xA, value(2.0)), 0xC, key("new")),
                // Its source removed: the binding goes with it.
                node(0x3, 0xF1),
                // Function changed: set whole.
                bound(node(0x5, 0xF4), 0xA, key("out")),
                // Added.
                node(0x6, 0xF5),
            ],
        );
        let diff = a.diff(&b);
        assert_eq!(diff.remove, [id(0x4)]);
        assert_eq!(
            diff.set.iter().map(|node| node.id).collect::<Vec<_>>(),
            [id(0x1), id(0x5), id(0x6)]
        );
        assert_eq!(
            diff.bind,
            [
                Binding {
                    node: id(0x2),
                    port: id(0xA),
                    source: value(2.0)
                },
                Binding {
                    node: id(0x2),
                    port: id(0xC),
                    source: key("new")
                },
            ]
        );
        assert_eq!(diff.unbind, [Port::new(id(0x2), id(0xB))]);
        assert_eq!(diff.root, None);
        let mut applied = a.clone();
        applied.apply(diff).unwrap();
        assert_eq!(applied, b);
        assert_eq!(
            b.diff(&b),
            GraphDiff::default(),
            "a graph is its own target"
        );
    }

    /// A root the target keeps without rooting it is removed and set again,
    /// which places it after the others, and what reads it is bound again.
    #[test]
    fn a_diff_unroots_a_root_the_target_keeps() {
        let reader = bound(node(0x2, 0xF1), 0xA, from(0x1, 0xB));
        let a = graph(Some(0x1), vec![node(0x1, 0xF0), reader.clone()]);
        let b = graph(None, vec![reader, node(0x1, 0xF0)]);
        let diff = a.diff(&b);
        assert_eq!(diff.remove, [id(0x1)]);
        assert_eq!(diff.set, [node(0x1, 0xF0)]);
        assert_eq!(
            diff.bind,
            [Binding {
                node: id(0x2),
                port: id(0xA),
                source: from(0x1, 0xB)
            }]
        );
        let mut applied = a.clone();
        applied.apply(diff).unwrap();
        assert_eq!(applied, b);

        let mut rerooted = b.clone();
        rerooted.apply(b.diff(&a)).unwrap();
        assert_eq!(rerooted, graph(Some(0x1), b.nodes), "rooted, in b's order");
    }

    /// A root no node holds is dropped by removing it, and a root the diff's
    /// removals take is set again: whatever its root, a graph's diff gives
    /// the target's.
    #[test]
    fn a_diff_gives_the_target_s_root_whatever_it_names() {
        let cases = [
            // A root that is no node, dropped.
            (
                graph(Some(0x7), vec![node(0x1, 0xF0)]),
                graph(None, vec![node(0x1, 0xF0)]),
            ),
            // A root the target keeps while dropping its node.
            (
                graph(Some(0x1), vec![node(0x1, 0xF0)]),
                graph(Some(0x1), vec![]),
            ),
            // A root that is no node, kept.
            (
                graph(Some(0x7), vec![]),
                graph(Some(0x7), vec![node(0x1, 0xF0)]),
            ),
        ];
        for (a, b) in cases {
            let mut applied = a.clone();
            applied.apply(a.diff(&b)).unwrap();
            assert_eq!(applied, b, "{a:?} to {b:?}");
        }
    }

    #[test]
    fn a_graph_serializes_leanly() {
        let g = graph(
            Some(0x1),
            vec![
                Node {
                    children: Some(vec![id(0x2)]),
                    ..node(0x1, 0xF0)
                },
                bound(
                    bound(node(0x2, 0xF1), 0xA, Source::Value(Value::F64(1.5))),
                    0xB,
                    Source::Key("face/x".into()),
                ),
            ],
        );
        let json = serde_json::to_string(&g).unwrap();
        assert_eq!(
            json,
            r#"{"root":"00000000-0000-0000-0000-000000000001","nodes":[{"id":"00000000-0000-0000-0000-000000000001","function":"00000000-0000-0000-0000-0000000000f0","children":["00000000-0000-0000-0000-000000000002"]},{"id":"00000000-0000-0000-0000-000000000002","function":"00000000-0000-0000-0000-0000000000f1","bind":{"00000000-0000-0000-0000-00000000000a":{"value":{"f64":1.5}},"00000000-0000-0000-0000-00000000000b":{"key":"face/x"}}}]}"#
        );
        assert_eq!(serde_json::from_str::<Graph>(&json).unwrap(), g);
        assert_eq!(
            serde_json::to_string(&GraphDiff::default()).unwrap(),
            "{}",
            "an empty diff is empty"
        );
    }

    /// A value bound to a port is an enum in an enum, which YAML writes as a
    /// single-entry map: every binding, a graph's and a diff's, reads back.
    #[test]
    fn a_graph_and_a_diff_round_trip_through_yaml() {
        let values = [
            Value::F64(1.5),
            Value::String("text".into()),
            Value::Unit,
            Value::Option(Some(Box::new(Value::F64(1.0)))),
            Value::Option(None),
            Value::ArrayF64(vec![1.0, 2.0]),
        ];
        let mut g = graph(Some(0x1), vec![node(0x1, 0xF0)]);
        for (port, value) in values.iter().enumerate() {
            g.nodes[0]
                .bind
                .insert(id(port as u128 + 0x10), Source::Value(value.clone()));
        }
        g.nodes[0]
            .bind
            .insert(id(0x20), Source::Key("face/x".into()));
        g.nodes[0].bind.insert(
            id(0x21),
            Source::Select {
                source: Box::new(from(0x1, 0x10)),
                path: Key::new(".field"),
            },
        );
        let yaml = serde_yaml::to_string(&g).unwrap();
        assert_eq!(serde_yaml::from_str::<Graph>(&yaml).unwrap(), g, "{yaml}");

        let diff = Graph::empty().diff(&g);
        let diff = GraphDiff {
            bind: vec![Binding {
                node: id(0x1),
                port: id(0x30),
                source: Source::Value(Value::Option(Some(Box::new(Value::U8(1))))),
            }],
            ..diff
        };
        let yaml = serde_yaml::to_string(&diff).unwrap();
        assert_eq!(
            serde_yaml::from_str::<GraphDiff>(&yaml).unwrap(),
            diff,
            "{yaml}"
        );
    }
}
