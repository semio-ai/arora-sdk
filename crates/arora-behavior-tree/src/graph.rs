//! Lowering the shared [`Graph`] model onto the behavior tree's runtime form.
//!
//! The behavior tree is an [interpreter](crate::behavior::BehaviorTreeInterpreter)
//! over `arora-behavior`'s shared [`Graph`]: its tree is a graph whose
//! bindings are the nodes' arguments and whose children are the tree's edges.
//! This module is the bridge — it lowers a [`Graph`] into the crate's
//! [`schema::Node`] list, which
//! [`load_behavior_tree_nodes_with`](crate::load_behavior_tree_nodes_with) then
//! instantiates into a runnable [`BehaviorTree`].
//!
//! A node's bindings are its arguments, whether the function reads the port
//! or writes it — an output bound to a key is the cell its value lands in:
//! - [`Source::Value`] → [`Expression::Value`]
//! - [`Source::Key`] → [`Expression::VariableId`], the variable named by the
//!   key (the Direct convention: variable name == store key), its id
//!   [`gen_uuid_from_str`] of the key, so every binding to one key shares one
//!   cell. A key starting with `.` is tree-local.
//! - [`Source::Port`] → [`Expression::NodeArgument`] (another node's port)
//! - [`Source::Select`] → [`Expression::Select`]

use std::collections::HashMap;

use arora_behavior::graph::{Graph, Source};
use arora_types::gen_uuid_from_str;
use uuid::Uuid;

use crate::error::BehaviorTreeError;
use crate::schema::{Expression, Node as SchemaNode, NodeParameterId};
use crate::variable::VariableResolver;
use crate::{load_behavior_tree_nodes_with, BehaviorTree};

/// Lower a shared [`Graph`] into the behavior tree's [`schema::Node`] list, root
/// first (the loader takes the first node as the tree root).
///
/// Each graph node becomes one schema node, its bindings its argument
/// [`Expression`]s.
pub fn graph_to_bt_nodes(graph: &Graph) -> Result<Vec<SchemaNode>, BehaviorTreeError> {
    Ok(lower_graph(graph)?.0)
}

/// The full lowering: the schema nodes plus the variable-name map the loader
/// binds, one variable per key the graph binds.
fn lower_graph(
    graph: &Graph,
) -> Result<(Vec<SchemaNode>, HashMap<Uuid, String>), BehaviorTreeError> {
    if let Some(root) = &graph.root {
        if graph.node(root).is_none() {
            return Err(BehaviorTreeError::InconsistentTreeError {
                message: format!("graph root {root} is not a node"),
            });
        }
    }
    // Root first; the loader's "first node is the root" contract depends on
    // it. The others by id: their order means nothing to the tree, and the
    // order of detached sinks decides which one of two writing a key wins.
    let mut ordered: Vec<&arora_behavior::graph::Node> = graph.nodes.iter().collect();
    ordered.sort_by_key(|node| (graph.root != Some(node.id), node.id));

    let mut variables = HashMap::new();
    let nodes = ordered
        .into_iter()
        .map(|node| SchemaNode {
            id: node.id,
            function: node.function,
            arguments: node
                .bind
                .iter()
                .map(|(port, source)| (*port, source_to_expression(source, &mut variables)))
                .collect(),
            children: node.children.clone(),
        })
        .collect();
    Ok((nodes, variables))
}

/// Build a runnable [`BehaviorTree`] from a shared [`Graph`], binding the
/// variable of each key the graph binds to a store slot via `resolver` (the
/// Direct convention). Keys the resolver declines, and tree-local keys, stay
/// tree-local.
pub fn build_behavior_tree(
    graph: &Graph,
    resolver: &VariableResolver,
) -> Result<BehaviorTree, BehaviorTreeError> {
    let (nodes, variables) = lower_graph(graph)?;
    load_behavior_tree_nodes_with(nodes, resolver, &variables)
}

/// A binding's source, as the tree's argument [`Expression`], recording the
/// variable a key names in `variables`. A [`Source::Select`] becomes an
/// [`Expression::Select`] wrapping the lowered source, so the `Key` path is
/// applied on read.
fn source_to_expression(source: &Source, variables: &mut HashMap<Uuid, String>) -> Expression {
    match source {
        Source::Value(value) => Expression::Value(value.clone()),
        Source::Key(key) => {
            let id = gen_uuid_from_str(key);
            variables.entry(id).or_insert_with(|| key.clone());
            Expression::VariableId(id)
        }
        Source::Port(port) => Expression::NodeArgument(NodeParameterId {
            node: port.node,
            parameter: port.port,
        }),
        Source::Select { source, path } => Expression::Select {
            source: Box::new(source_to_expression(source, variables)),
            path: path.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::{FAIL_FUNCTION_ID, SEQ_FUNCTION_ID, SUCCEED_FUNCTION_ID};
    use crate::run_behavior_tree;
    use crate::schema::_RET_PARAM_ID;
    use arora_behavior::graph::{GraphDiff, Node as GraphNode, Port};
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

    fn run(graph: &Graph) -> arora_behavior::Status {
        let tree = build_behavior_tree(graph, &|_| None).expect("tree builds");
        let mut bridge = NativeBridge::default();
        run_behavior_tree(&tree, &HashMap::new(), &mut bridge, false).expect("run")
    }

    /// The order of a graph's nodes means nothing to the tree: they lower
    /// root first, then by id, whatever order the graph lists them in.
    #[test]
    fn nodes_lower_root_first_then_by_id() {
        let ids = [0x30, 0x10, 0x40, 0x20].map(Uuid::from_u128);
        let mut graph = Graph::empty();
        for id in ids {
            graph.nodes.push(GraphNode {
                id,
                function: SUCCEED_FUNCTION_ID,
                ..GraphNode::default()
            });
        }
        graph.root = Some(ids[2]);
        let order: Vec<Uuid> = graph_to_bt_nodes(&graph)
            .unwrap()
            .iter()
            .map(|node| node.id)
            .collect();
        assert_eq!(order, [0x40, 0x10, 0x20, 0x30].map(Uuid::from_u128));
    }

    /// A `Select` binding lowers to a `Select` expression wrapping the lowered
    /// source, carrying the `Key` path — the tree applies it on read (the
    /// path semantics are unit-tested in `arora-behavior`).
    #[test]
    fn a_select_binding_lowers_to_a_select_expression() {
        use arora_types::data::Key;

        let source = Uuid::from_u128(0xA);
        let target = Uuid::from_u128(0xB);
        let out = Uuid::from_u128(0x1);
        let input = Uuid::from_u128(0x2);

        let mut graph = Graph::empty();
        graph.nodes.push(leaf(source, SUCCEED_FUNCTION_ID));
        let mut reader = leaf(target, SUCCEED_FUNCTION_ID);
        reader.bind.insert(
            input,
            Source::Select {
                source: Box::new(Source::Port(Port::new(source, out))),
                path: Key::new(".field"),
            },
        );
        graph.nodes.push(reader);

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
        use arora_behavior::Status;
        let root = Uuid::from_u128(0x100);
        let a = Uuid::from_u128(0x1);
        let b = Uuid::from_u128(0x2);
        let mut graph = Graph::empty();
        graph.root = Some(root);
        graph.nodes = vec![
            control(root, SEQ_FUNCTION_ID, vec![a, b]),
            leaf(a, SUCCEED_FUNCTION_ID),
            leaf(b, SUCCEED_FUNCTION_ID),
        ];
        assert_eq!(run(&graph), Status::Success);

        // Swap the second child to fail via a diff, and the tree now fails.
        graph
            .apply(GraphDiff {
                set: vec![leaf(b, FAIL_FUNCTION_ID)],
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
        // The child listed first: the root still lowers first.
        graph.nodes = vec![
            leaf(child, SUCCEED_FUNCTION_ID),
            control(root, SEQ_FUNCTION_ID, vec![child]),
        ];
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
        use crate::schema_groot::BehaviorTree as GrootTree;
        use arora_behavior::Status;

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

    /// A key binding lowers to the variable of that key, whichever port it
    /// binds: an input reads the variable's cell, an output — here the
    /// return value — is written to it. Every binding to one key shares one
    /// variable.
    #[test]
    fn a_key_lowers_to_its_variable_whichever_port_binds_it() {
        let node_id = Uuid::from_u128(0x1);
        let input = Uuid::from_u128(0x10);
        let other = Uuid::from_u128(0x11);
        let mut node = leaf(node_id, SUCCEED_FUNCTION_ID);
        node.bind.insert(input, Source::Key("face/x".into()));
        node.bind.insert(other, Source::Key("face/x".into()));
        node.bind
            .insert(_RET_PARAM_ID, Source::Key("motor/left".into()));
        let mut graph = Graph::empty();
        graph.root = Some(node_id);
        graph.nodes.push(node);

        let (nodes, variables) = lower_graph(&graph).unwrap();
        let face = gen_uuid_from_str("face/x");
        let motor = gen_uuid_from_str("motor/left");
        assert_eq!(nodes[0].arguments[&input], Expression::VariableId(face));
        assert_eq!(nodes[0].arguments[&other], Expression::VariableId(face));
        assert_eq!(
            nodes[0].arguments[&_RET_PARAM_ID],
            Expression::VariableId(motor)
        );
        assert_eq!(
            variables,
            HashMap::from([
                (face, "face/x".to_string()),
                (motor, "motor/left".to_string())
            ])
        );
    }

    /// The variables reach the loader with the key as their name, so the
    /// Direct resolver is asked for the store slot at exactly that key.
    #[test]
    fn bound_keys_resolve_by_name() {
        use std::cell::RefCell;

        let node_id = Uuid::from_u128(0x1);
        let mut node = leaf(node_id, SUCCEED_FUNCTION_ID);
        node.bind
            .insert(Uuid::from_u128(0x10), Source::Key("face/x".into()));
        let mut graph = Graph::empty();
        graph.root = Some(node_id);
        graph.nodes.push(node);

        let asked = RefCell::new(Vec::new());
        build_behavior_tree(&graph, &|name: &str| {
            asked.borrow_mut().push(name.to_string());
            None
        })
        .expect("tree builds");
        assert!(
            asked.borrow().contains(&"face/x".to_string()),
            "the loader asked the resolver for the bound key, got {:?}",
            asked.borrow()
        );
    }

    /// An argument that reads another node's argument lowers whatever order
    /// the nodes come in: here the reader comes before the node it reads.
    #[test]
    fn a_port_link_lowers_whatever_the_node_order() {
        use crate::schema::NodeParameterId;

        let reader = Uuid::from_u128(0x1);
        let source = Uuid::from_u128(0x2);
        let source_arg = Uuid::from_u128(0x20);
        let nodes = vec![
            SchemaNode {
                id: reader,
                function: SEQ_FUNCTION_ID,
                arguments: HashMap::from([(
                    Uuid::from_u128(0x10),
                    Expression::NodeArgument(NodeParameterId {
                        node: source,
                        parameter: source_arg,
                    }),
                )]),
                children: Some(vec![source]),
            },
            SchemaNode {
                id: source,
                function: SUCCEED_FUNCTION_ID,
                arguments: HashMap::from([(source_arg, Expression::Value(Value::U8(3)))]),
                children: None,
            },
        ];
        crate::load_behavior_tree_nodes_with(nodes, &|_| None, &HashMap::new())
            .expect("the reader lowers after the node it reads");
    }
}
