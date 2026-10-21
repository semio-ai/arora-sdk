//! Compute nodes as plain leaves of the tree, in the order their data needs.
//!
//! The same compute nodes as [`compute`](crate::compute), with the same
//! resolution: the graph is checked, sorted so that each node comes after the
//! nodes it reads, and classified by what reads it. Here the resolution's only
//! product is an order. Each compute node that something reads, or that writes
//! a store key, becomes an ordinary module leaf, and the leaves go under a new
//! root `PARALLEL`, in that order, ahead of the tree:
//!
//! ```text
//! PARALLEL
//!   {.#raw} = mul(a: {sensor/distance}, b: 100f64)
//!   {.#cm} = clamp(x: {.#raw}, lo: 0f64)
//!   {face/level} = add(a: {.#cm}, b: 1f64)
//!   PARALLEL                                  the tree as given
//!     below(x: {.#cm}, limit: 30f64)
//! ```
//!
//! - **An edge is a variable.** A node's return value is bound (`_ret`) to its
//!   output when it has one, else to a tree-local variable of its own,
//!   [`edge_variable`], named `.#<node>`. A `#node` reference reads that
//!   variable. A `#node.parameter` reference reads the leaf's argument cell
//!   for that parameter, where the leaf writes the parameter back.
//! - **Every leaf runs every tick,** in that order, before the tree: the
//!   `PARALLEL` ticks every child every tick. Nothing is evaluated on demand
//!   and nothing is cached: a node read by two leaves runs once per tick,
//!   because it is one leaf, and a settled graph runs as a moving one.
//! - **The tree reads this tick's values,** computed before it ticks. A
//!   variable the tree writes reaches the leaves at the next tick.
//! - **A node nothing reads that writes no store key is dropped,** as the
//!   resolution classifies it unused.
//! - **A leaf returns `Success`,** so the root's status is the tree's.
use std::collections::HashMap;

use uuid::Uuid;

use crate::compute::{references_in, ComputeGraph, ComputeNode};
use crate::error::BehaviorTreeError;
use crate::nodes::PARALLEL_FUNCTION_ID;
use crate::schema::{CallExpression, Expression, Node, NodeParameterId, _RET_PARAM_ID};
use crate::variable::VariableResolver;
use crate::{load_behavior_tree_nodes_with, BehaviorTree};

/// The tree-local variable a compute node with no output returns into, when
/// it is a leaf: its id with the high bits flipped. Its name is `.#<node>`.
pub fn edge_variable(node: Uuid) -> Uuid {
    Uuid::from_u128(node.as_u128() ^ (0xED6E_u128 << 112))
}

/// The id of the root `PARALLEL` the leaves go under, from the tree's root.
fn root_id(tree_root: Uuid) -> Uuid {
    Uuid::from_u128(tree_root.as_u128() ^ (0x7007_u128 << 112))
}

/// `expression` with each `#node` reference read from the variable or the
/// argument cell that holds it once the node is a leaf.
fn without_references(expression: Expression, returns: &HashMap<Uuid, Uuid>) -> Expression {
    match expression {
        Expression::Computed(port) if port.parameter == _RET_PARAM_ID => {
            Expression::VariableId(returns[&port.node])
        }
        Expression::Computed(port) => Expression::NodeArgument(NodeParameterId {
            node: port.node,
            parameter: port.parameter,
        }),
        Expression::Select { source, path } => Expression::Select {
            source: Box::new(without_references(*source, returns)),
            path,
        },
        Expression::Call(call) => {
            let CallExpression {
                module,
                function,
                arguments,
            } = *call;
            Expression::Call(Box::new(CallExpression {
                module: without_references(module, returns),
                function: without_references(function, returns),
                arguments: arguments
                    .into_iter()
                    .map(|(parameter, argument)| {
                        (
                            without_references(parameter, returns),
                            without_references(argument, returns),
                        )
                    })
                    .collect(),
            }))
        }
        expression => expression,
    }
}

/// Build a tree from `nodes` and `compute` nodes, each compute node a leaf
/// under a new root `PARALLEL`, ahead of the tree: see the
/// [module docs](self).
///
/// Refuses what [`load_behavior_tree_with_compute`](crate::load_behavior_tree_with_compute)
/// refuses, and a compute node with the id of a tree node.
pub fn load_behavior_tree_with_compute_as_leaves(
    nodes: Vec<Node>,
    compute: Vec<ComputeNode>,
    resolver: &VariableResolver,
    names: &HashMap<Uuid, String>,
) -> Result<BehaviorTree, BehaviorTreeError> {
    let tree_root = nodes
        .first()
        .ok_or(BehaviorTreeError::InconsistentTreeError {
            message: "behavior tree has no nodes".to_string(),
        })?
        .id;
    if let Some(clash) = compute
        .iter()
        .find(|compute| nodes.iter().any(|node| node.id == compute.id))
    {
        return Err(BehaviorTreeError::InconsistentTreeError {
            message: format!("compute node {} has the id of a tree node", clash.id),
        });
    }

    // The resolution: the graph checked, sorted and classified.
    let mut tree_reads = Vec::new();
    for node in &nodes {
        for argument in node.arguments.values() {
            references_in(argument, &mut tree_reads);
        }
    }
    let resolved = ComputeGraph::build(compute, tree_reads, &mut HashMap::new(), resolver, names)?;
    let unused = &resolved.plan().unused;
    let ordered: Vec<&ComputeNode> = resolved
        .nodes()
        .filter(|node| !unused.contains(&node.id))
        .collect();

    // Its product: leaves in that order.
    let returns: HashMap<Uuid, Uuid> = ordered
        .iter()
        .map(|node| {
            (
                node.id,
                node.output.unwrap_or_else(|| edge_variable(node.id)),
            )
        })
        .collect();
    let mut names = names.clone();
    for node in &ordered {
        if node.output.is_none() {
            names.insert(edge_variable(node.id), format!(".#{}", node.id));
        }
    }
    let leaves = ordered.iter().map(|node| Node {
        id: node.id,
        function: node.function,
        arguments: node
            .inputs
            .iter()
            .map(|(parameter, input)| (*parameter, without_references(input.clone(), &returns)))
            .chain([(_RET_PARAM_ID, Expression::VariableId(returns[&node.id]))])
            .collect(),
        children: None,
    });
    let root = Node {
        id: root_id(tree_root),
        function: PARALLEL_FUNCTION_ID,
        arguments: HashMap::new(),
        children: Some(
            ordered
                .iter()
                .map(|node| node.id)
                .chain([tree_root])
                .collect(),
        ),
    };
    let tree = nodes.into_iter().map(|mut node| {
        node.arguments = node
            .arguments
            .into_iter()
            .map(|(parameter, argument)| (parameter, without_references(argument, &returns)))
            .collect();
        node
    });
    let all = std::iter::once(root).chain(leaves).chain(tree).collect();
    load_behavior_tree_nodes_with(all, resolver, &names)
}
