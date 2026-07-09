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

use std::collections::HashMap;

use arora_behavior::graph::{Graph, LinkSource};
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
    // Group links by the node they feed, so each node collects its arguments.
    let mut args_by_node: HashMap<Uuid, HashMap<Uuid, Expression>> = HashMap::new();
    for link in &graph.links {
        let expr = link_source_to_expression(&link.source);
        args_by_node
            .entry(link.target.node)
            .or_default()
            .insert(link.target.port, expr);
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
    Ok(nodes)
}

/// Build a runnable [`BehaviorTree`] from a shared [`Graph`], binding each
/// variable named in [`Graph::variables`] to a store slot via `resolver` (the
/// Direct convention). Variables the resolver declines stay tree-local.
pub fn build_behavior_tree(
    graph: &Graph,
    resolver: &VariableResolver,
) -> Result<BehaviorTree, BehaviorTreeError> {
    let nodes = graph_to_bt_nodes(graph)?;
    load_behavior_tree_nodes_with(nodes, resolver, &graph.variables)
}

/// A graph link source, as the tree's argument [`Expression`].
fn link_source_to_expression(source: &LinkSource) -> Expression {
    match source {
        LinkSource::Literal(value) => Expression::Value(value.clone()),
        LinkSource::Variable(id) => Expression::VariableId(*id),
        LinkSource::Port(port) => Expression::NodeArgument(NodeParameterId {
            node: port.node,
            parameter: port.port,
        }),
    }
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
        fn arora_call(&mut self, _module: &Uuid, call: Call) -> Result<CallResult, CallError> {
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
}
