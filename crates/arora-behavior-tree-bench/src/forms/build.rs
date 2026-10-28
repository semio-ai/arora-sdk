//! Shared graph construction: attached leaves or detached data nodes.
use crate::{
    host::{output, parameter, state, Op, Signature, STATE, VALUE},
    model::{Source, Tree, Workload},
};
use arora_behavior_tree::{
    nodes::{
        EQUAL_A_PARAM_ID, EQUAL_B_PARAM_ID, EQUAL_FUNCTION_ID, PARALLEL_FUNCTION_ID,
        RUN_FUNCTION_ID,
    },
    schema::{Expression, Node, NodeParameterId, _RET_PARAM_ID},
};
use arora_types::value::Value;
use std::collections::HashMap;
use uuid::Uuid;
fn call_id(c: usize) -> Uuid {
    Uuid::from_u128(0x4000_0000 + c as u128)
}
fn function_id(c: usize) -> Uuid {
    Uuid::from_u128(0x5000_0000 + c as u128)
}
/// The `publish` function the tree calls for call `c`.
fn publish_id(c: usize) -> Uuid {
    Uuid::from_u128(0x5100_0000 + c as u128)
}
/// The function of tree call site `s`.
fn site_id(s: usize) -> Uuid {
    Uuid::from_u128(0x5200_0000 + s as u128)
}
fn key_variable(k: usize) -> Uuid {
    Uuid::from_u128(0x2000_0000 + k as u128)
}
fn local_variable(c: usize) -> Uuid {
    Uuid::from_u128(0x3000_0000 + c as u128)
}

/// Graph calls as attached leaves or detached nodes, root first, with store names.
pub fn build(workload: &Workload, detached: bool) -> (Vec<Node>, HashMap<Uuid, String>) {
    let names: HashMap<_, _> = workload
        .keys
        .iter()
        .enumerate()
        .map(|(k, (name, _))| (key_variable(k), name.clone()))
        .collect();

    let bound = |c: usize| workload.calls[c].publishes;
    let source = |source: &Source| match source {
        Source::Key(k) => Expression::VariableId(key_variable(*k)),
        Source::Value(x) => Expression::Value(x.clone()),
        Source::Call(c) if detached => Expression::NodeArgument(NodeParameterId {
            node: call_id(*c),
            parameter: _RET_PARAM_ID,
        }),
        Source::Call(c) => Expression::VariableId(match bound(*c) {
            Some(k) => key_variable(k),
            None => local_variable(*c),
        }),
    };
    let call = |c: usize| {
        let graph_call = &workload.calls[c];
        let mut arguments: HashMap<_, _> = graph_call
            .arguments
            .iter()
            .map(|(parameter, from)| (*parameter, source(from)))
            .collect();
        if graph_call.op == Op::Damp {
            arguments.insert(STATE, Expression::Value(Value::F64(f64::NAN)));
        }
        match (bound(c), detached) {
            (Some(k), _) => {
                arguments.insert(_RET_PARAM_ID, Expression::VariableId(key_variable(k)));
            }
            (None, false) => {
                arguments.insert(_RET_PARAM_ID, Expression::VariableId(local_variable(c)));
            }
            // An unpublished detached call runs because something reads it.
            (None, true) => {}
        }
        Node {
            id: call_id(c),
            function: function_id(c),
            arguments,
            children: None,
        }
    };
    fn convert(
        node: &Tree,
        tree: &mut Builder,
        source: &dyn Fn(&Source) -> Expression,
    ) -> Vec<Uuid> {
        match node {
            Tree::Composite(function, children) => {
                let children = children
                    .iter()
                    .flat_map(|child| convert(child, tree, source))
                    .collect();
                vec![tree.push(*function, HashMap::new(), Some(children))]
            }
            Tree::Equal(k, value) => {
                let arguments = [
                    (EQUAL_A_PARAM_ID, Expression::VariableId(key_variable(*k))),
                    (EQUAL_B_PARAM_ID, Expression::Value(value.clone())),
                ];
                vec![tree.push(EQUAL_FUNCTION_ID, HashMap::from(arguments), None)]
            }
            Tree::Call(site, _, arguments, outputs, states) => {
                let arguments = (arguments.iter().enumerate())
                    .map(|(i, a)| (parameter(i), source(a)))
                    .chain(
                        outputs
                            .iter()
                            .enumerate()
                            .map(|(i, k)| (output(i), Expression::VariableId(key_variable(*k)))),
                    )
                    .chain(
                        (0..*states).map(|i| (state(i), Expression::Value(Value::F64(f64::NAN)))),
                    )
                    .collect();
                vec![tree.push(site_id(*site), arguments, None)]
            }
            Tree::RealCall(function, arguments, outputs) => {
                let arguments = arguments
                    .iter()
                    .map(|(id, a)| (*id, source(a)))
                    .chain(
                        outputs
                            .iter()
                            .map(|(id, k)| (*id, Expression::VariableId(key_variable(*k)))),
                    )
                    .collect();
                vec![tree.push(*function, arguments, None)]
            }
            Tree::Publish(c, k) => {
                let key = Expression::VariableId(key_variable(*k));
                let arguments = [(VALUE, source(&Source::Call(*c))), (_RET_PARAM_ID, key)];
                vec![tree.push(publish_id(*c), HashMap::from(arguments), None)]
            }
            Tree::Calls(_) => Vec::new(),
            Tree::Run => vec![tree.push(RUN_FUNCTION_ID, HashMap::new(), None)],
        }
    }
    let mut tree = Builder::default();
    let top = convert(&workload.tree, &mut tree, &source);
    // The calls the root holds as leaves, in data order, around the tree.
    let leaves: Vec<usize> = if detached {
        Vec::new()
    } else {
        (0..workload.calls.len()).collect()
    };
    let root = if leaves.is_empty() {
        top[0]
    } else {
        tree.nodes.extend(leaves.iter().map(|&c| call(c)));
        let (after, before): (Vec<usize>, Vec<usize>) =
            leaves.iter().partition(|&&c| workload.after.contains(&c));
        let children = (before.iter().map(|&c| call_id(c)))
            .chain(top)
            .chain(after.iter().map(|&c| call_id(c)))
            .collect();
        tree.push(PARALLEL_FUNCTION_ID, HashMap::new(), Some(children))
    };
    if detached {
        tree.nodes.extend((0..workload.calls.len()).map(call));
    }
    let mut nodes = tree.nodes;
    let at = nodes.iter().position(|node| node.id == root).unwrap();
    let root = nodes.remove(at);
    nodes.insert(0, root);
    (nodes, names)
}

/// Control and check nodes, numbered from 1, beside the calls.
#[derive(Default)]
struct Builder {
    next: u128,
    nodes: Vec<Node>,
}
impl Builder {
    fn push(
        &mut self,
        function: Uuid,
        arguments: HashMap<Uuid, Expression>,
        children: Option<Vec<Uuid>>,
    ) -> Uuid {
        self.next += 1;
        let id = Uuid::from_u128(self.next);
        self.nodes.push(Node {
            id,
            function,
            arguments,
            children,
        });
        id
    }
}

/// The functions `workload`'s forms call.
pub fn signatures(workload: &Workload) -> HashMap<Uuid, Signature> {
    let mut functions = HashMap::new();
    for (c, call) in workload.calls.iter().enumerate() {
        let shape = Signature {
            op: call.op,
            arguments: call.arguments.len(),
            outputs: 0,
            state: 0,
        };
        functions.insert(function_id(c), shape);
    }
    for &c in workload.published.keys() {
        functions.insert(
            publish_id(c),
            Signature {
                op: Op::Publish,
                arguments: 1,
                outputs: 0,
                state: 0,
            },
        );
    }
    fn sites(node: &Tree, functions: &mut HashMap<Uuid, Signature>) {
        match node {
            Tree::Composite(_, children) => {
                children.iter().for_each(|child| sites(child, functions))
            }
            Tree::Call(site, op, arguments, outputs, states) => {
                let shape = Signature {
                    op: *op,
                    arguments: arguments.len(),
                    outputs: outputs.len(),
                    state: *states,
                };
                functions.insert(site_id(*site), shape);
            }
            _ => {}
        }
    }
    sites(&workload.tree, &mut functions);
    functions
}
