//! Lowering the shared [`Graph`] model onto the behavior tree's runtime form.
//!
//! The behavior tree is an [interpreter](crate::behavior::BehaviorTreeInterpreter)
//! over `arora-behavior`'s shared [`Graph`]: its tree is a graph whose links are
//! the argument and child edges. This module is the bridge — it lowers a
//! [`Graph`] into the crate's [`schema::Node`] list, which
//! [`load_behavior_tree_nodes_with`](crate::load_behavior_tree_nodes_with) then
//! instantiates into a runnable [`BehaviorTree`]. The execution machinery
//! (native builtins vs. `arora_call` module dispatch) is unchanged; only the
//! authored representation is now the shared model.
//!
//! Link sources map onto the tree's argument [`Expression`]s:
//! - [`LinkSource::Literal`] → [`Expression::Value`]
//! - [`LinkSource::Variable`] → [`Expression::VariableId`] (a shared `{var}`)
//! - [`LinkSource::Port`] → [`Expression::NodeArgument`] (another node's slot)
//!
//! A slot with a **predetermined key** and no link binds to the store variable
//! of that name (the Direct convention: variable name == store key) — an input
//! reads it, an output writes it. A link on the slot overrides the
//! predetermination; nothing else does.

use std::collections::{HashMap, HashSet};

use arora_behavior::graph::{Graph, LinkSource, Port};
use arora_types::gen_uuid_from_str;
use uuid::Uuid;

use crate::error::BehaviorTreeError;
use crate::schema::{Expression, Node as SchemaNode, NodeParameterId};
use crate::variable::VariableResolver;
use crate::{load_behavior_tree_nodes_with, BehaviorTree};

/// Lower a shared [`Graph`] into the behavior tree's [`schema::Node`] list, root
/// first (the loader takes the first node as the tree root).
///
/// Each graph node becomes one schema node; the links targeting that node's
/// inputs become its argument [`Expression`]s. Graph node order is otherwise
/// preserved, so a [`LinkSource::Port`] referencing another node's slot follows
/// the same ordering constraints the raw node list always had.
pub fn graph_to_bt_nodes(graph: &Graph) -> Result<Vec<SchemaNode>, BehaviorTreeError> {
    Ok(lower_graph(graph)?.0)
}

/// The full lowering: the schema nodes plus the variable-name map the loader
/// binds — [`Graph::variables`] extended with one synthesized variable per
/// **predetermined, unlinked slot** (deterministic id from the key, so every
/// slot predetermined to one key shares one cell).
fn lower_graph(
    graph: &Graph,
) -> Result<(Vec<SchemaNode>, HashMap<Uuid, String>), BehaviorTreeError> {
    // Group links by the node they feed, so each node collects its arguments.
    let mut args_by_node: HashMap<Uuid, HashMap<Uuid, Expression>> = HashMap::new();
    for link in &graph.links {
        let expr = link_source_to_expression(&link.source)?;
        args_by_node
            .entry(link.target.node)
            .or_default()
            .insert(link.target.port, expr);
    }

    // Output ports a link reads from: their predetermination is overridden.
    let linked_sources: HashSet<Port> = graph
        .links
        .iter()
        .filter_map(|link| arora_behavior::graph::source_port(&link.source))
        .collect();

    // Predetermined, unlinked slots bind to the store variable named by their
    // key — an input argument reads the cell, an output argument is the cell
    // the node's mutation lands in. A link on the slot took precedence above.
    let mut variables = graph.variables.clone();
    for (id, node) in &graph.nodes {
        let mut bind = |io: &arora_behavior::graph::Io, overridden: bool| {
            let Some(key) = &io.predetermined_key else {
                return;
            };
            if overridden {
                return;
            }
            let args = args_by_node.entry(*id).or_default();
            if args.contains_key(&io.id) {
                return;
            }
            let variable = gen_uuid_from_str(key);
            variables.entry(variable).or_insert_with(|| key.clone());
            args.insert(io.id, Expression::VariableId(variable));
        };
        for io in &node.inputs {
            bind(io, false);
        }
        for io in &node.outputs {
            bind(
                io,
                linked_sources.contains(&Port {
                    node: *id,
                    port: io.id,
                }),
            );
        }
    }

    let mut ordered: Vec<&Uuid> = graph.nodes.keys().collect();
    // Root first; the loader's "first node is the root" contract depends on it.
    if let Some(root) = &graph.root {
        if !graph.nodes.contains_key(root) {
            return Err(BehaviorTreeError::InconsistentTreeError {
                message: format!("graph root {root} is not a node"),
            });
        }
        ordered.sort_by_key(|id| *id != root);
    }

    let mut nodes = Vec::with_capacity(graph.nodes.len());
    for id in ordered {
        let node = &graph.nodes[id];
        nodes.push(SchemaNode {
            id: node.id,
            function: node.function,
            arguments: args_by_node.remove(&node.id).unwrap_or_default(),
            children: node.children.clone(),
        });
    }
    Ok((nodes, variables))
}

/// Build a runnable [`BehaviorTree`] from a shared [`Graph`], binding each
/// variable named in [`Graph::variables`] — plus one synthesized variable per
/// predetermined, unlinked slot — to a store slot via `resolver` (the Direct
/// convention). Variables the resolver declines stay tree-local.
pub fn build_behavior_tree(
    graph: &Graph,
    resolver: &VariableResolver,
) -> Result<BehaviorTree, BehaviorTreeError> {
    let (nodes, variables) = lower_graph(graph)?;
    load_behavior_tree_nodes_with(nodes, resolver, &variables)
}

/// A graph link source, as the tree's argument [`Expression`]. A
/// [`LinkSource::Select`] becomes an [`Expression::Select`] wrapping the lowered
/// source, so the `Key` path is applied on read (see the eval below).
fn link_source_to_expression(source: &LinkSource) -> Result<Expression, BehaviorTreeError> {
    Ok(match source {
        LinkSource::Literal(value) => Expression::Value(value.clone()),
        LinkSource::Variable(id) => Expression::VariableId(*id),
        LinkSource::Port(port) => Expression::NodeArgument(NodeParameterId {
            node: port.node,
            parameter: port.port,
        }),
        LinkSource::Select { source, path } => Expression::Select {
            source: Box::new(link_source_to_expression(source)?),
            path: path.clone(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::{FAIL_FUNCTION_ID, SEQ_FUNCTION_ID, SUCCEED_FUNCTION_ID};
    use crate::run_behavior_tree;
    use arora_behavior::graph::{GraphDiff, Node as GraphNode};
    use arora_types::call::{Call, CallBridge, CallError, CallResult, Callable, CallableId};
    use arora_types::value::Value;
    use std::rc::Rc;

    /// A `CallBridge` that only registers/invokes callables natively — enough to
    /// run a tree of builtins (no module leaves).
    #[derive(Default)]
    struct NativeBridge {
        registered: HashMap<u64, Rc<dyn Callable>>,
        next_id: u64,
    }

    impl CallBridge for NativeBridge {
        fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
            Err(CallError::FunctionNotFound { id: call.id })
        }
        fn arora_register_callable(&mut self, callable: Rc<dyn Callable>) -> CallableId {
            let id = self.next_id;
            self.next_id += 1;
            self.registered.insert(id, callable);
            CallableId { id }
        }
        fn arora_unregister_callable(&mut self, callable_id: &CallableId) {
            self.registered.remove(&callable_id.id);
        }
        fn arora_call_indirect(&mut self, callable_id: &CallableId) -> Result<Value, CallError> {
            let callable =
                self.registered
                    .get(&callable_id.id)
                    .cloned()
                    .ok_or(CallError::Generic {
                        message: format!("unknown callable {}", callable_id.id),
                    })?;
            callable.call(self)
        }
    }

    fn control(id: Uuid, function: Uuid, children: Vec<Uuid>) -> GraphNode {
        GraphNode {
            id,
            function,
            children: Some(children),
            ..GraphNode::default()
        }
    }

    fn leaf(id: Uuid, function: Uuid) -> GraphNode {
        GraphNode {
            id,
            function,
            ..GraphNode::default()
        }
    }

    fn run(graph: &Graph) -> crate::arora_generated::behavior_tree::status::Status {
        let tree = build_behavior_tree(graph, &|_| None).expect("tree builds");
        let mut bridge = NativeBridge::default();
        run_behavior_tree(&tree, Rc::new(HashMap::new()), &mut bridge, false).expect("run")
    }

    /// A `Select` link lowers to a `Select` expression wrapping the lowered
    /// source, carrying the `Key` path — the tree applies it on read (the
    /// path semantics are unit-tested in `arora-behavior`).
    #[test]
    fn a_select_link_lowers_to_a_select_expression() {
        use arora_behavior::graph::{Link, LinkSource, Port};
        use arora_types::data::Key;

        let source = Uuid::from_u128(0xA);
        let target = Uuid::from_u128(0xB);
        let out = Uuid::from_u128(0x1);
        let input = Uuid::from_u128(0x2);

        let mut graph = Graph::empty();
        graph.nodes.insert(source, leaf(source, SUCCEED_FUNCTION_ID));
        graph.nodes.insert(target, leaf(target, SUCCEED_FUNCTION_ID));
        graph.links.push(Link::new(
            Port::new(target, input),
            LinkSource::Select {
                source: Box::new(LinkSource::Port(Port::new(source, out))),
                path: Key::new(".field"),
            },
        ));

        let (nodes, _) = lower_graph(&graph).expect("lower");
        let target_node = nodes.iter().find(|n| n.id == target).expect("target node");
        match target_node.arguments.get(&input).expect("input argument") {
            Expression::Select { source, path } => {
                assert_eq!(path.get_path(), ".field");
                assert!(matches!(**source, Expression::NodeArgument(_)));
            }
            other => panic!("expected a Select expression, got {other:?}"),
        }
    }

    #[test]
    fn seq_of_builtins_lowers_and_runs() {
        use crate::arora_generated::behavior_tree::status::Status;
        let root = Uuid::from_u128(0x100);
        let a = Uuid::from_u128(0x1);
        let b = Uuid::from_u128(0x2);
        let mut graph = Graph::empty();
        graph.root = Some(root);
        graph
            .nodes
            .insert(root, control(root, SEQ_FUNCTION_ID, vec![a, b]));
        graph.nodes.insert(a, leaf(a, SUCCEED_FUNCTION_ID));
        graph.nodes.insert(b, leaf(b, SUCCEED_FUNCTION_ID));
        assert_eq!(run(&graph), Status::Success);

        // Swap the second child to fail via a diff, and the tree now fails.
        graph
            .apply(GraphDiff {
                add_nodes: vec![leaf(b, FAIL_FUNCTION_ID)],
                ..GraphDiff::default()
            })
            .unwrap();
        assert_eq!(run(&graph), Status::Failure);
    }

    #[test]
    fn root_node_is_lowered_first() {
        let root = Uuid::from_u128(0xEEE);
        let child = Uuid::from_u128(0x1);
        let mut graph = Graph::empty();
        graph.root = Some(root);
        // Insert child first so map order can't accidentally put it first.
        graph.nodes.insert(child, leaf(child, SUCCEED_FUNCTION_ID));
        graph
            .nodes
            .insert(root, control(root, SEQ_FUNCTION_ID, vec![child]));
        let nodes = graph_to_bt_nodes(&graph).unwrap();
        assert_eq!(nodes.first().unwrap().id, root);
    }

    #[test]
    fn missing_root_node_is_an_error() {
        let mut graph = Graph::empty();
        graph.root = Some(Uuid::from_u128(0xDEAD));
        assert!(graph_to_bt_nodes(&graph).is_err());
    }

    /// Groot XML lowers to the shared graph and runs through it — the import path
    /// is now Groot → `Graph` → `BehaviorTree`. Builtins need no function index.
    #[test]
    fn groot_lowers_to_graph_and_runs() {
        use crate::arora_generated::behavior_tree::status::Status;
        use crate::schema_groot::BehaviorTree as GrootTree;

        let xml = r#"<root main_tree_to_execute="MainTree">
  <BehaviorTree ID="MainTree">
    <Sequence>
      <Succeed/>
      <Succeed/>
    </Sequence>
  </BehaviorTree>
</root>"#;
        let groot = GrootTree::try_from_groot_xml(xml).expect("parse");
        let graph = groot.into_graph(&HashMap::new()).expect("lower to graph");
        assert!(graph.root.is_some());
        assert_eq!(graph.nodes.len(), 3, "sequence + two leaves");
        assert_eq!(run(&graph), Status::Success);
    }

    /// An unlinked input with a predetermined key lowers to a variable
    /// expression on that key's deterministic id; a linked one keeps the link.
    #[test]
    fn predetermined_slots_bind_unless_linked() {
        use arora_behavior::graph::{Io, Link, Port};

        let node_id = Uuid::from_u128(0x1);
        let bound_port = Uuid::from_u128(0x10);
        let linked_port = Uuid::from_u128(0x11);
        let mut node = leaf(node_id, SUCCEED_FUNCTION_ID);
        node.inputs = vec![
            Io {
                predetermined_key: Some("face/x".to_string()),
                ..Io::new(bound_port)
            },
            Io {
                predetermined_key: Some("face/y".to_string()),
                ..Io::new(linked_port)
            },
        ];
        let mut graph = Graph::empty();
        graph.root = Some(node_id);
        graph.nodes.insert(node_id, node);
        // The link on the second slot overrides its predetermination.
        graph.links.push(Link::new(
            Port {
                node: node_id,
                port: linked_port,
            },
            LinkSource::Literal(Value::Boolean(true)),
        ));

        let nodes = graph_to_bt_nodes(&graph).unwrap();
        let lowered = &nodes[0];
        assert_eq!(
            lowered.arguments[&bound_port],
            Expression::VariableId(gen_uuid_from_str("face/x")),
            "the unlinked slot binds to its predetermined key's variable"
        );
        assert_eq!(
            lowered.arguments[&linked_port],
            Expression::Value(Value::Boolean(true)),
            "the link wins over the predetermined key"
        );
    }

    /// An output slot binds to its predetermined key too — unless a link reads
    /// from it, which overrides the predetermination.
    #[test]
    fn predetermined_outputs_bind_unless_read_by_a_link() {
        use arora_behavior::graph::{Io, Link, Port};

        let producer = Uuid::from_u128(0x1);
        let consumer = Uuid::from_u128(0x2);
        let out_port = Uuid::from_u128(0x20);
        let in_port = Uuid::from_u128(0x21);

        let make = |linked: bool| {
            let mut p = leaf(producer, SUCCEED_FUNCTION_ID);
            p.outputs = vec![Io {
                predetermined_key: Some("motor/left".to_string()),
                ..Io::new(out_port)
            }];
            let mut graph = Graph::empty();
            graph.root = Some(producer);
            graph.nodes.insert(producer, p);
            if linked {
                graph
                    .nodes
                    .insert(consumer, leaf(consumer, SUCCEED_FUNCTION_ID));
                graph.links.push(Link::new(
                    Port {
                        node: consumer,
                        port: in_port,
                    },
                    LinkSource::Port(Port {
                        node: producer,
                        port: out_port,
                    }),
                ));
            }
            graph
        };

        let unlinked = graph_to_bt_nodes(&make(false)).unwrap();
        assert_eq!(
            unlinked[0].arguments[&out_port],
            Expression::VariableId(gen_uuid_from_str("motor/left")),
            "the unread output binds to its predetermined key's variable"
        );

        let linked = graph_to_bt_nodes(&make(true)).unwrap();
        assert!(
            !linked
                .iter()
                .find(|n| n.id == producer)
                .unwrap()
                .arguments
                .contains_key(&out_port),
            "a link reading the output overrides its predetermination"
        );
    }

    /// The synthesized variables reach the loader with the key as their name, so
    /// the Direct resolver is asked for the store slot at exactly that key.
    #[test]
    fn predetermined_keys_resolve_by_name() {
        use arora_behavior::graph::Io;
        use std::cell::RefCell;

        let node_id = Uuid::from_u128(0x1);
        let port = Uuid::from_u128(0x10);
        let mut node = leaf(node_id, SUCCEED_FUNCTION_ID);
        node.inputs = vec![Io {
            predetermined_key: Some("face/x".to_string()),
            ..Io::new(port)
        }];
        let mut graph = Graph::empty();
        graph.root = Some(node_id);
        graph.nodes.insert(node_id, node);

        let asked = RefCell::new(Vec::new());
        build_behavior_tree(&graph, &|name: &str| {
            asked.borrow_mut().push(name.to_string());
            None
        })
        .expect("tree builds");
        assert!(
            asked.borrow().contains(&"face/x".to_string()),
            "the loader asked the resolver for the predetermined key, got {:?}",
            asked.borrow()
        );
    }
}
