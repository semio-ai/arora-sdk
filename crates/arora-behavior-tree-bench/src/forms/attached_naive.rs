use super::{Fixture, Form, Nodes};
use crate::{
    host::Bridge,
    model::{is_output, written_cell, Workload},
};
use arora_behavior_tree::{
    nodes::PARALLEL_FUNCTION_ID,
    schema::{Expression, Node, NodeParameterId, _RET_PARAM_ID},
    variable::VariableCell,
};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub struct AttachedNaive;

fn sources(expr: &Expression, ports: &mut HashSet<Uuid>, variables: &mut HashSet<Uuid>) {
    match expr {
        Expression::NodeArgument(p) => {
            ports.insert(p.node);
        }
        Expression::VariableId(v) => {
            variables.insert(*v);
        }
        Expression::Select { source, .. } => sources(source, ports, variables),
        _ => {}
    }
}
fn rewrite(expr: &mut Expression, bindings: &HashMap<NodeParameterId, Expression>) {
    match expr {
        Expression::NodeArgument(port) => {
            if let Some(variable) = bindings.get(port) {
                *expr = variable.clone();
            }
        }
        Expression::Select { source, .. } => rewrite(source, bindings),
        _ => {}
    }
}
impl Form for AttachedNaive {
    fn name(&self) -> &'static str {
        "attached-naive"
    }
    fn build(&self, workload: &Workload) -> Nodes {
        let (mut nodes, names) = workload.nodes.clone();
        let functions = Bridge::index();
        let mutable =
            |node: &Node, parameter: &Uuid| is_output(functions, &node.function, parameter);
        let by_id: HashMap<_, _> = nodes.iter().map(|n| (n.id, n)).collect();
        let mut tree_writes: HashSet<_> = nodes
            .iter()
            .filter(|n| workload.attached.contains(&n.id))
            .flat_map(|n| {
                n.arguments
                    .iter()
                    .filter(|(p, _)| mutable(n, p))
                    .filter_map(|(_, e)| written_cell(&by_id, e))
            })
            .collect();
        if workload.dynamic_keys {
            tree_writes.extend(
                names
                    .iter()
                    .filter(|(_, name)| !arora_behavior_tree::variable::is_tree_local(name))
                    .map(|(id, _)| *id),
            );
        }
        let mut pending: Vec<_> = nodes
            .iter()
            .filter(|n| {
                !workload.attached.contains(&n.id)
                    && functions.contains_key(&n.function)
                    && n.children.is_none()
            })
            .map(|n| {
                let (mut ports, mut vars) = (HashSet::new(), HashSet::new());
                for (p, e) in &n.arguments {
                    if *p != _RET_PARAM_ID {
                        sources(e, &mut ports, &mut vars);
                    }
                }
                (n.id, ports, vars.iter().any(|v| tree_writes.contains(v)))
            })
            .collect();
        let graph: HashSet<_> = pending.iter().map(|(id, ..)| *id).collect();
        let mut order = Vec::new();
        let mut done = HashSet::new();
        let mut after = HashSet::new();
        while !pending.is_empty() {
            let at = pending
                .iter()
                .position(|(_, reads, _)| {
                    reads.iter().all(|r| !graph.contains(r) || done.contains(r))
                })
                .expect("the text loader rejects cycles");
            let (id, reads, tree_input) = pending.remove(at);
            if tree_input
                || reads
                    .iter()
                    .any(|r| after.contains(r) || workload.attached.contains(r))
            {
                after.insert(id);
            }
            order.push(id);
            done.insert(id);
        }
        if order.is_empty() {
            return (nodes, names);
        }
        let mut bindings = HashMap::new();
        for node in nodes.iter_mut().filter(|n| graph.contains(&n.id)) {
            // A return must be captured so the attached leaf reports Success.
            let expr = node
                .arguments
                .entry(_RET_PARAM_ID)
                .or_insert_with(|| Expression::VariableId(Uuid::new_v5(&node.id, b"return")));
            bindings.insert(
                NodeParameterId {
                    node: node.id,
                    parameter: _RET_PARAM_ID,
                },
                expr.clone(),
            );
            let outputs: Vec<_> = node
                .arguments
                .keys()
                .filter(|p| mutable(node, p))
                .copied()
                .collect();
            for p in outputs {
                let expr = node.arguments.get_mut(&p).unwrap();
                if let Expression::Value(value) = expr {
                    *expr = Expression::Variable(VariableCell::local_with(value.clone()));
                }
                bindings.insert(
                    NodeParameterId {
                        node: node.id,
                        parameter: p,
                    },
                    expr.clone(),
                );
            }
        }

        for node in &mut nodes {
            for expr in node.arguments.values_mut() {
                rewrite(expr, &bindings);
            }
        }
        let old_root = nodes[0].id;
        let root = Uuid::new_v5(&old_root, b"attached-naive");
        let children = order
            .iter()
            .filter(|id| !after.contains(id))
            .copied()
            .chain([old_root])
            .chain(order.iter().filter(|id| after.contains(id)).copied())
            .collect();
        nodes.insert(
            0,
            Node {
                id: root,
                function: PARALLEL_FUNCTION_ID,
                arguments: HashMap::new(),
                children: Some(children),
            },
        );
        (nodes, names)
    }
    fn fixture_counts(&self, fixture: Fixture) -> (u64, u64) {
        use crate::scale::Scenario;
        match fixture {
            Fixture::Synthetic => (3760, 5520),
            Fixture::Conversation => (714_560, 973_431),
            Fixture::Scale(Scenario::Settled) => (1600, 1920),
            Fixture::Scale(Scenario::OneInput) => (1600, 1920),
            Fixture::Scale(Scenario::AllInputs) => (1600, 1920),
            Fixture::Scale(Scenario::Clock) => (1600, 1920),
            Fixture::Scale(Scenario::Branches) => (2880, 3840),
            Fixture::Scale(Scenario::BranchesSwitching) => (2880, 4200),
        }
    }
}
