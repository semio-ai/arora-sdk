//! Attached calls through a borrowing bridge and the owned-call fallback.
//! Run with `cargo bench -p arora-behavior-tree --bench attached`.
//! Each row is the median of seven samples of 500 ticks after 50 warm-ups.
//! Input updates happen outside the timed tick; lowering is not timed.
use std::collections::HashMap;
use std::hint::black_box;
use std::rc::Rc;
use std::time::{Duration, Instant};

use arora_behavior::{FunctionIndex, ModuleFunction, Status};
use arora_behavior_tree::nodes::*;
use arora_behavior_tree::schema::{Expression, Node, _RET_PARAM_ID};
use arora_behavior_tree::{load_behavior_tree_nodes_with, lower_behavior_tree};
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::{Call, CallBridge, CallError, CallResult, Callable, CallableId};
use arora_types::data::{DataStore, Key, Slot};
use arora_types::record::module::frozen::{Function, Parameter};
use arora_types::record::ty::{FrozenTy, PrimitiveKind};
use arora_types::value::Value;
use uuid::Uuid;

const MODULE: Uuid = Uuid::from_u128(0xB000);
const FUNCTIONS: [Uuid; 4] = [
    Uuid::from_u128(0xB001),
    Uuid::from_u128(0xB002),
    Uuid::from_u128(0xB003),
    Uuid::from_u128(0xB004),
];
const PARAMS: [Uuid; 5] = [
    Uuid::from_u128(0xB011),
    Uuid::from_u128(0xB012),
    Uuid::from_u128(0xB013),
    Uuid::from_u128(0xB014),
    Uuid::from_u128(0xB015),
];

#[derive(Default)]
struct State {
    registered: HashMap<u64, Rc<dyn Callable>>,
    next: u64,
    calls: u64,
    indirect: u64,
}
impl State {
    fn evaluate(
        &mut self,
        module: Option<Uuid>,
        id: Uuid,
        args: &[(Uuid, &Value)],
    ) -> Result<CallResult, CallError> {
        assert_eq!(module, Some(MODULE));
        self.calls += 1;
        let n = |at: usize| match args[at] {
            (parameter, Value::F64(value)) if parameter == PARAMS[at] => *value,
            other => panic!("unexpected argument {other:?}"),
        };
        let ret = match id {
            id if id == FUNCTIONS[0] => n(0) + n(1),
            id if id == FUNCTIONS[1] => n(0) * n(1),
            id if id == FUNCTIONS[2] => n(0).clamp(n(1), n(2)),
            id if id == FUNCTIONS[3] => n(3) + (n(0) - n(1)) / (n(2) - n(1)) * (n(4) - n(3)),
            _ => return Err(CallError::FunctionNotFound { id }),
        };
        Ok(CallResult {
            ret: Value::F64(ret),
            mutated: Vec::new(),
        })
    }
}

trait MeasuredBridge: CallBridge {
    fn state(&mut self) -> &mut State;
}
macro_rules! common_bridge {
    () => {
        fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
            let args: Vec<_> = call
                .args
                .iter()
                .map(|arg| (arg.id, arg.value.as_ref()))
                .collect();
            self.state.evaluate(call.module_id, call.id, &args)
        }
        fn arora_register_callable(&mut self, callable: Rc<dyn Callable>) -> CallableId {
            self.state.next += 1;
            self.state.registered.insert(self.state.next, callable);
            CallableId {
                id: self.state.next,
            }
        }
        fn arora_unregister_callable(&mut self, callable: &CallableId) {
            self.state.registered.remove(&callable.id);
        }
        fn arora_call_indirect(&mut self, callable: &CallableId) -> Result<Value, CallError> {
            self.state.indirect += 1;
            let callable = self.state.registered[&callable.id].clone();
            callable.call(self)
        }
    };
}
#[derive(Default)]
struct Owned {
    state: State,
}
impl CallBridge for Owned {
    common_bridge!();
}
impl MeasuredBridge for Owned {
    fn state(&mut self) -> &mut State {
        &mut self.state
    }
}
#[derive(Default)]
struct Borrowed {
    state: State,
}
impl CallBridge for Borrowed {
    common_bridge!();
    fn arora_call_borrowed(
        &mut self,
        module: Option<Uuid>,
        id: Uuid,
        args: &[(Uuid, &Value)],
    ) -> Result<CallResult, CallError> {
        self.state.evaluate(module, id, args)
    }
}
impl MeasuredBridge for Borrowed {
    fn state(&mut self) -> &mut State {
        &mut self.state
    }
}

fn index() -> FunctionIndex {
    FUNCTIONS
        .into_iter()
        .zip([("add", 2), ("mul", 2), ("clamp", 3), ("remap", 5)])
        .map(|(id, (name, count))| {
            (
                id,
                ModuleFunction {
                    module_id: MODULE,
                    function_id: id,
                    function_name: name.into(),
                    function: Function {
                        parameters: PARAMS[..count]
                            .iter()
                            .enumerate()
                            .map(|(i, id)| {
                                (
                                    *id,
                                    Parameter {
                                        name: format!("arg{i}"),
                                        ty: FrozenTy::from(PrimitiveKind::F64),
                                        mutable: false,
                                    },
                                )
                            })
                            .collect(),
                        parameter_ordering: PARAMS[..count].to_vec(),
                        return_ty: FrozenTy::from(PrimitiveKind::F64),
                    },
                },
            )
        })
        .collect()
}
fn control(id: Uuid, function: Uuid, children: Vec<Uuid>) -> Node {
    Node {
        id,
        function,
        arguments: HashMap::new(),
        children: Some(children),
    }
}
fn leaf(id: Uuid, function: Uuid) -> Node {
    Node {
        id,
        function,
        arguments: HashMap::new(),
        children: None,
    }
}
fn variable(i: usize) -> Uuid {
    Uuid::from_u128(0x100000 + i as u128)
}

fn measure(bridge: &mut dyn MeasuredBridge, mode: &str, n: usize, guarded: bool, scenario: usize) {
    let k = 2.max(n / 10);
    let store = SimpleDataStore::new();
    let inputs: Vec<Box<dyn Slot>> = (0..k)
        .map(|i| store.slot(&Key::new(format!("input/{i}"))))
        .collect();
    for input in &inputs {
        input.set(Some(Value::F64(0.25))).unwrap();
    }
    let mut names: HashMap<_, _> = (0..k)
        .map(|i| (variable(i), format!("input/{i}")))
        .collect();
    let root = Uuid::from_u128(1);
    let run = Uuid::from_u128(2);
    let mut nodes = vec![
        control(root, PARALLEL_FUNCTION_ID, Vec::new()),
        leaf(run, RUN_FUNCTION_ID),
    ];
    let mut children = Vec::new();
    let signatures = index();
    for i in 0..n {
        let id = Uuid::from_u128(10 + i as u128 * 5);
        let function = FUNCTIONS[i % 4];
        let mut node = leaf(id, function);
        for (at, param) in signatures[&function]
            .function
            .parameter_ordering
            .iter()
            .enumerate()
        {
            node.arguments.insert(
                *param,
                if at == 0 {
                    Expression::VariableId(variable(i % k))
                } else {
                    Expression::Value(Value::F64([0.0, 0.0, 1.0, 0.0, 100.0][at]))
                },
            );
        }
        let output = variable(k + i);
        names.insert(output, format!("output/{i}"));
        node.arguments
            .insert(_RET_PARAM_ID, Expression::VariableId(output));
        nodes.push(node);
        if guarded {
            let [fallback, seq, equal, succeed] =
                [1, 2, 3, 4].map(|offset| Uuid::from_u128(id.as_u128() + offset));
            nodes.push(control(fallback, FALLBACK_FUNCTION_ID, vec![seq, succeed]));
            nodes.push(control(seq, SEQ_FUNCTION_ID, vec![equal, id]));
            let mut guard = leaf(equal, EQUAL_FUNCTION_ID);
            guard
                .arguments
                .insert(EQUAL_A_PARAM_ID, Expression::Value(Value::F64(0.0)));
            guard
                .arguments
                .insert(EQUAL_B_PARAM_ID, Expression::Value(Value::F64(1.0)));
            nodes.push(guard);
            nodes.push(leaf(succeed, SUCCEED_FUNCTION_ID));
            children.push(fallback);
        } else {
            children.push(id);
        }
    }
    children.push(run);
    nodes[0].children = Some(children);
    let tree =
        load_behavior_tree_nodes_with(nodes, &|name| Some(store.slot(&Key::from(name))), &names)
            .unwrap();
    let tree = lower_behavior_tree(tree, &signatures, bridge, false).unwrap();
    for _ in 0..50 {
        assert_eq!(tree.tick(bridge).unwrap(), Status::Running);
    }
    let mut samples = Vec::new();
    let mut counts = (0, 0);
    for sample in 0..7 {
        bridge.state().calls = 0;
        bridge.state().indirect = 0;
        let mut elapsed = Duration::ZERO;
        for tick in 0..500 {
            let value = Value::F64((sample * 500 + tick) as f64 / 1000.0);
            for input in inputs.iter().take(match scenario {
                0 => 0,
                1 => 1,
                _ => k,
            }) {
                input.set(Some(value.clone())).unwrap();
            }
            let start = Instant::now();
            black_box(tree.tick(bridge).unwrap());
            elapsed += start.elapsed();
        }
        samples.push(elapsed.as_secs_f64() * 1e6 / 500.0);
        counts = (bridge.state().calls / 500, bridge.state().indirect / 500);
    }
    samples.sort_by(f64::total_cmp);
    let expected_calls = if guarded { 0 } else { n as u64 };
    assert_eq!(counts.0, expected_calls);
    // Every child stays registered, including those native parents tick directly.
    assert!(bridge.state().registered.len() >= n + 2);
    println!(
        "{mode},{},{n},{},{:.3},{},{}",
        if guarded { "guarded" } else { "leaves" },
        ["settled", "one input", "all inputs"][scenario],
        samples[3],
        counts.0,
        counts.1
    );
    tree.unregister(bridge);
    assert!(bridge.state().registered.is_empty());
}
fn main() {
    println!("bridge,shape,N,scenario,median_us,module_calls,indirect_dispatches");
    for (name, mut bridge) in [
        (
            "borrowed",
            Box::<Borrowed>::default() as Box<dyn MeasuredBridge>,
        ),
        ("owned", Box::<Owned>::default()),
    ] {
        for n in [10, 100, 1000] {
            for scenario in 0..3 {
                measure(&mut *bridge, name, n, false, scenario);
            }
            measure(&mut *bridge, name, n, true, 0);
        }
    }
}
