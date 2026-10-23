//! Attached nodes keep their callable boundary, ordered calls and writes
//! when native parents invoke their tick functions directly.
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use arora_behavior::Status;
use arora_behavior_tree_types::TickId;
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::{Call, CallBridge, CallError, CallResult, Callable, CallableId};
use arora_types::data::{DataStore, Key};
use arora_types::record::reference::FrozenReference;
use arora_types::record::ty::{FrozenArray, FrozenOption, FrozenTy, PrimitiveKind};
use arora_types::value::{Structure, StructureField, Value};
use arora_types::AroraType;
use uuid::Uuid;

use crate::call_expression_tests::{
    call, control, f, function, index, node, parameter, ADD, ADD_A, ADD_B, BATTERY, DISTANCE, MATH,
    SENSORS,
};
use crate::nodes::{PARALLEL_FUNCTION_ID, RUN_FUNCTION_ID, SEQ_FUNCTION_ID, SUCCEED_FUNCTION_ID};
use crate::schema::{Expression, NodeParameterId, _RET_PARAM_ID};
use crate::variable::VariableCell;
use crate::{load_behavior_tree_nodes, lower_behavior_tree, BehaviorTreeError, FunctionIndex};

const COMPOSITE: Uuid = Uuid::from_u128(0xCA00);
const CHILDREN: Uuid = Uuid::from_u128(0xCA01);

/// Uses the default borrowed-call fallback; records the owned wire contract.
#[derive(Default)]
struct Bridge {
    registered: HashMap<u64, Rc<dyn Callable>>,
    next: u64,
    calls: Vec<Call>,
    indirect: usize,
    responses: VecDeque<CallResult>,
    sensor_write: Option<VariableCell>,
    reenter: Option<CallableId>,
}
impl CallBridge for Bridge {
    fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
        self.calls.push(call.clone());
        if let Some(response) = self.responses.pop_front() {
            return Ok(response);
        }
        let ret = match call.id {
            COMPOSITE => {
                assert_eq!(call.module_id, Some(MATH));
                assert_eq!(call.args[0].id, CHILDREN);
                let children = (*call.args[0].value).clone().into_elements().unwrap();
                let mut status = Status::Success;
                for child in children {
                    let child: TickId = child.try_into().unwrap();
                    status = self
                        .arora_call_indirect(&CallableId {
                            id: child.callable_id,
                        })?
                        .try_into()
                        .unwrap();
                }
                status.into()
            }
            DISTANCE | BATTERY => {
                assert_eq!(call.module_id, Some(SENSORS));
                if let Some(cell) = &self.sensor_write {
                    cell.set(Value::F64(99.0));
                }
                Value::F64(if call.id == DISTANCE { 2.0 } else { 3.0 })
            }
            ADD => {
                assert_eq!(call.module_id, Some(MATH));
                if let Some(id) = self.reenter.take() {
                    assert_eq!(
                        Status::try_from(self.arora_call_indirect(&id)?).unwrap(),
                        Status::Success
                    );
                }
                let number = |id| match call
                    .args
                    .iter()
                    .find(|arg| arg.id == id)
                    .unwrap()
                    .value
                    .as_ref()
                {
                    Value::F64(value) => *value,
                    other => panic!("expected a number, got {other:?}"),
                };
                Value::F64(number(ADD_A) + number(ADD_B))
            }
            _ => return Err(CallError::FunctionNotFound { id: call.id }),
        };
        Ok(CallResult {
            ret,
            mutated: Vec::new(),
        })
    }
    fn arora_register_callable(&mut self, callable: Rc<dyn Callable>) -> CallableId {
        self.next += 1;
        self.registered.insert(self.next, callable);
        CallableId { id: self.next }
    }
    fn arora_unregister_callable(&mut self, id: &CallableId) {
        self.registered.remove(&id.id);
    }
    fn arora_call_indirect(&mut self, id: &CallableId) -> Result<Value, CallError> {
        self.indirect += 1;
        let callable = self.registered[&id.id].clone();
        callable.call(self)
    }
}

fn composite_index() -> FunctionIndex {
    let mut functions = index();
    functions.extend([function(
        MATH,
        COMPOSITE,
        "composite",
        vec![(
            CHILDREN,
            parameter(
                "children",
                FrozenTy::FrozenArray(FrozenArray {
                    reference: FrozenReference {
                        id: TickId::arora_type_id(),
                        version: semver::Version::new(1, 0, 0).into(),
                    },
                }),
                false,
            ),
        )],
    )]);
    functions
}

/// The root uses indirect dispatch, and every native child edge bypasses it;
/// every child is nevertheless registered until the lowering is unregistered.
#[test]
fn native_children_tick_without_indirect_dispatch() {
    let tree = load_behavior_tree_nodes(vec![
        control(1, PARALLEL_FUNCTION_ID, &[2, 3]),
        control(2, SEQ_FUNCTION_ID, &[4]),
        node(3, RUN_FUNCTION_ID, vec![]),
        node(4, SUCCEED_FUNCTION_ID, vec![]),
    ])
    .unwrap();
    let mut bridge = Bridge::default();
    let tree = lower_behavior_tree(tree, &index(), &mut bridge, false).unwrap();
    assert_eq!(bridge.registered.len(), 4);
    for _ in 0..2 {
        assert_eq!(tree.tick(&mut bridge).unwrap(), Status::Running);
    }
    assert_eq!(bridge.indirect, 2, "only the root dispatches indirectly");
    tree.unregister(&mut bridge);
    assert!(bridge.registered.is_empty());
}

/// A module composite receives child ids in tree order and calls them through
/// the engine, including a native subtree that ticks its own child directly.
#[test]
fn module_composites_keep_registered_child_ids() {
    let tree = load_behavior_tree_nodes(vec![
        control(1, COMPOSITE, &[2, 3]),
        control(2, SEQ_FUNCTION_ID, &[4]),
        node(3, RUN_FUNCTION_ID, vec![]),
        node(4, SUCCEED_FUNCTION_ID, vec![]),
    ])
    .unwrap();
    let mut bridge = Bridge::default();
    let tree = lower_behavior_tree(tree, &composite_index(), &mut bridge, false).unwrap();
    assert_eq!(tree.tick(&mut bridge).unwrap(), Status::Running);
    assert_eq!(bridge.indirect, 3, "root and the module's two child ticks");
    let children = bridge.calls[0].args[0]
        .value
        .clone()
        .into_elements()
        .unwrap();
    let ids: Vec<TickId> = children
        .into_iter()
        .map(|value| value.try_into().unwrap())
        .collect();
    assert_eq!(
        ids.iter().map(|id| id.callable_id).collect::<Vec<_>>(),
        [2, 3]
    );
    assert_eq!(bridge.registered.len(), 4);
}

/// Hash-map insertion order does not change nested-call evaluation, and the
/// default bridge receives exactly the ordered owned call (without `_ret`).
#[test]
fn nested_calls_and_owned_fallback_keep_argument_order() {
    let out = VariableCell::local();
    let tree = load_behavior_tree_nodes(vec![
        control(1, SEQ_FUNCTION_ID, &[2]),
        node(
            2,
            ADD,
            vec![
                (ADD_B, call(BATTERY, vec![])),
                (_RET_PARAM_ID, Expression::Variable(out.clone())),
                (ADD_A, call(DISTANCE, vec![])),
            ],
        ),
    ])
    .unwrap();
    let mut bridge = Bridge::default();
    let tree = lower_behavior_tree(tree, &index(), &mut bridge, false).unwrap();
    for _ in 0..2 {
        assert_eq!(tree.tick(&mut bridge).unwrap(), Status::Success);
        assert_eq!(out.get(), Some(Value::F64(5.0)));
        assert_eq!(
            bridge.calls.drain(..).collect::<Vec<_>>(),
            vec![
                Call {
                    module_id: Some(SENSORS),
                    id: DISTANCE,
                    args: vec![]
                },
                Call {
                    module_id: Some(SENSORS),
                    id: BATTERY,
                    args: vec![]
                },
                Call {
                    module_id: Some(MATH),
                    id: ADD,
                    args: vec![
                        StructureField {
                            id: ADD_A,
                            value: Box::new(Value::F64(2.0))
                        },
                        StructureField {
                            id: ADD_B,
                            value: Box::new(Value::F64(3.0))
                        },
                    ]
                },
            ]
        );
    }
}

/// A bridge can answer borrowed calls and re-enter another callable while a
/// later argument and a re-entered callable both write its snapshotted cell.
#[test]
fn borrowed_arguments_allow_cell_writes_and_reentrancy() {
    #[derive(Default)]
    struct Borrowing {
        inner: Bridge,
    }
    impl CallBridge for Borrowing {
        fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
            self.inner.arora_call(call)
        }
        fn arora_call_borrowed(
            &mut self,
            module: Option<Uuid>,
            id: Uuid,
            args: &[(Uuid, &Value)],
        ) -> Result<CallResult, CallError> {
            assert_eq!(
                args,
                &[(ADD_A, &Value::F64(10.0)), (ADD_B, &Value::F64(2.0))]
            );
            let result = self.arora_call(Call {
                module_id: module,
                id,
                args: args
                    .iter()
                    .map(|(id, value)| StructureField {
                        id: *id,
                        value: Box::new((*value).clone()),
                    })
                    .collect(),
            });
            assert_eq!(args[0].1, &Value::F64(10.0), "snapshot survives re-entry");
            result
        }
        fn arora_register_callable(&mut self, callable: Rc<dyn Callable>) -> CallableId {
            self.inner.arora_register_callable(callable)
        }
        fn arora_unregister_callable(&mut self, id: &CallableId) {
            self.inner.arora_unregister_callable(id);
        }
        fn arora_call_indirect(&mut self, id: &CallableId) -> Result<Value, CallError> {
            self.inner.indirect += 1;
            let callable = self.inner.registered[&id.id].clone();
            callable.call(self)
        }
    }
    struct WriteCell(VariableCell);
    impl Callable for WriteCell {
        fn call(&self, _: &mut dyn CallBridge) -> Result<Value, CallError> {
            assert_eq!(self.0.get(), Some(Value::F64(99.0)));
            self.0.set(Value::F64(77.0));
            Ok(Status::Success.into())
        }
    }
    let shared = VariableCell::local_with(Value::F64(10.0));
    let out = VariableCell::local();
    let tree = load_behavior_tree_nodes(vec![
        control(1, SEQ_FUNCTION_ID, &[2, 3]),
        node(2, SUCCEED_FUNCTION_ID, vec![]),
        node(
            3,
            ADD,
            vec![
                (ADD_A, Expression::Variable(shared.clone())),
                (ADD_B, call(DISTANCE, vec![])),
                (_RET_PARAM_ID, Expression::Variable(out.clone())),
            ],
        ),
    ])
    .unwrap();
    let mut bridge = Borrowing::default();
    bridge.inner.sensor_write = Some(shared.clone());
    let tree = lower_behavior_tree(tree, &index(), &mut bridge, false).unwrap();
    bridge.inner.reenter = Some(bridge.arora_register_callable(Rc::new(WriteCell(shared.clone()))));
    assert_eq!(tree.tick(&mut bridge).unwrap(), Status::Success);
    let received = bridge.inner.calls.last().unwrap();
    assert_eq!(received.args[0].value.as_ref(), &Value::F64(10.0));
    assert_eq!(shared.get(), Some(Value::F64(77.0)));
    assert_eq!(out.get(), Some(Value::F64(12.0)));
}

/// A child selection error reaches the root with the same CallError text
/// as converting the error at the child callable boundary.
#[test]
fn child_errors_reach_root_as_the_same_call_error() {
    let invalid = node(
        2,
        ADD,
        vec![
            (
                ADD_A,
                Expression::Select {
                    source: Box::new(f(1.0)),
                    path: ".missing".into(),
                },
            ),
            (ADD_B, f(2.0)),
        ],
    );
    let tree = load_behavior_tree_nodes(vec![control(1, SEQ_FUNCTION_ID, &[2]), invalid]).unwrap();
    let mut bridge = Bridge::default();
    let tree = lower_behavior_tree(tree, &index(), &mut bridge, false).unwrap();
    let error = tree.tick(&mut bridge).unwrap_err();
    let BehaviorTreeError::CallError(CallError::Generic { message }) = error else {
        panic!("expected the selection error at the callable boundary, got {error:?}");
    };
    assert_eq!(
        message,
        "inconsistent behavior tree: selector attribute 'missing' is neither a field id nor an index"
    );
    assert!(bridge.calls.is_empty());
}

/// Spawn this fixture under `--nocapture` to verify the actual printed trace.
/// A subprocess is needed because libtest captures println output per thread.
#[test]
#[ignore = "run by traces_keep_text_and_order in a subprocess to capture trace output"]
fn trace_fixture() {
    let tree = load_behavior_tree_nodes(vec![
        control(1, SEQ_FUNCTION_ID, &[2, 3]),
        node(
            2,
            ADD,
            vec![
                (ADD_A, call(DISTANCE, vec![])),
                (ADD_B, f(4.0)),
                (_RET_PARAM_ID, Expression::VariableId(Uuid::from_u128(9))),
            ],
        ),
        node(3, RUN_FUNCTION_ID, vec![]),
    ])
    .unwrap();
    let mut bridge = Bridge::default();
    let tree = lower_behavior_tree(tree, &index(), &mut bridge, true).unwrap();
    assert_eq!(tree.tick(&mut bridge).unwrap(), Status::Running);
    assert_eq!(
        bridge.calls.iter().map(|call| call.id).collect::<Vec<_>>(),
        [DISTANCE, ADD]
    );
}

/// Composite, leaf and nested-call traces retain their text and postorder.
#[test]
fn traces_keep_text_and_order() {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "attached_tests::trace_fixture",
            "--nocapture",
        ])
        .env_remove("RUST_TEST_THREADS")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("trace fixture exceeded ten seconds");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = String::from_utf8(result.stdout).unwrap();
    let lines: Vec<_> = output
        .lines()
        // Libtest can put its progress prefix before the first trace line.
        .filter_map(|line| line.find("tick ").map(|start| &line[start..]))
        .collect();
    assert_eq!(lines, [
        "tick 00000000-0000-0000-0000-000000000002 {00000000-0000-0000-0000-000000000009} = add(a: distance(), b: 4f64) -> Success",
        "tick 00000000-0000-0000-0000-000000000003 41ae5ed0-1d12-4b71-aab8-02e7efedf177() -> Running",
        "tick 00000000-0000-0000-0000-000000000001 32246df6-ab5d-4f18-9221-23e28731de93() -> Running",
    ]);
}

const FIELD: Uuid = Uuid::from_u128(0xCA02);

fn record(value: f64) -> Value {
    Value::Structure(Structure {
        id: Uuid::from_u128(0xCA03),
        fields: vec![StructureField {
            id: FIELD,
            value: Box::new(Value::F64(value)),
        }],
    })
}

fn selection(source: VariableCell) -> Expression {
    Expression::Select {
        source: Box::new(Expression::Variable(source)),
        path: Key::new(format!(".{FIELD}")),
    }
}

fn response(ret: Value) -> CallResult {
    CallResult {
        ret,
        mutated: vec![],
    }
}

/// A node declaring children needs a first parameter named `children` whose
/// array element is TickId. Each rejected signature fails before dispatch.
#[test]
fn module_children_require_a_first_tick_array_named_children() {
    let valid = composite_index()[&COMPOSITE].function.parameters[&CHILDREN].clone();
    let mut wrong_name = valid.clone();
    wrong_name.name = "kids".into();
    let mut wrong_element = valid.clone();
    wrong_element.ty = FrozenTy::FrozenArray(FrozenArray {
        reference: FrozenReference {
            id: FIELD,
            version: semver::Version::new(1, 0, 0).into(),
        },
    });
    let non_array = parameter("children", FrozenTy::from(PrimitiveKind::F64), false);
    for (case, parameters) in [
        ("no parameters", vec![]),
        ("wrong name", vec![(CHILDREN, wrong_name)]),
        ("wrong array element", vec![(CHILDREN, wrong_element)]),
        ("not an array", vec![(CHILDREN, non_array.clone())]),
        ("not first", vec![(ADD_A, non_array), (CHILDREN, valid)]),
    ] {
        let functions = FunctionIndex::from([function(MATH, COMPOSITE, "composite", parameters)]);
        let tree = load_behavior_tree_nodes(vec![
            control(1, COMPOSITE, &[2]),
            node(2, SUCCEED_FUNCTION_ID, vec![]),
        ])
        .unwrap();
        let mut bridge = Bridge {
            responses: VecDeque::from([response(Status::Success.into())]),
            ..Default::default()
        };
        let tree = lower_behavior_tree(tree, &functions, &mut bridge, false).unwrap();
        let error = tree.tick(&mut bridge).expect_err(case);
        // The root callable converts MissingChildrenParameter to CallError.
        let expected = BehaviorTreeError::MissingChildrenParameter {
            node: Uuid::from_u128(1),
            function: COMPOSITE,
        };
        let BehaviorTreeError::CallError(CallError::Generic { message }) = error else {
            panic!(
                "{case}: expected MissingChildrenParameter at the callable boundary, got {error:?}"
            );
        };
        assert_eq!(message, expected.to_string(), "{case}");
        assert!(bridge.calls.is_empty(), "{case}: rejected before dispatch");
    }
}

/// No children field omits the argument; an explicitly empty field passes a
/// typed empty array. The function signature alone does not add children.
#[test]
fn childless_module_nodes_omit_the_children_argument() {
    for children in [None, Some(vec![])] {
        let mut root = node(1, COMPOSITE, vec![]);
        root.children = children.clone();
        let tree = load_behavior_tree_nodes(vec![root]).unwrap();
        let mut bridge = Bridge {
            responses: VecDeque::from([response(Status::Success.into())]),
            ..Default::default()
        };
        let tree = lower_behavior_tree(tree, &composite_index(), &mut bridge, false).unwrap();
        assert_eq!(tree.tick(&mut bridge).unwrap(), Status::Success);
        let expected = if children.is_some() {
            vec![StructureField {
                id: CHILDREN,
                value: Box::new(Value::array_of_type::<TickId>(vec![])),
            }]
        } else {
            vec![]
        };
        assert_eq!(bridge.calls[0].args, expected, "children: {children:?}");
    }
}

/// A selected return binds the whole source cell as its destination. Its path
/// is never read, even when the current value cannot supply that path.
#[test]
fn selected_returns_write_the_source_without_reading_the_path() {
    for initial in [Some(record(3.0)), Some(Value::F64(3.0)), None] {
        let source = VariableCell::local();
        source.set_opt(initial);
        let tree = load_behavior_tree_nodes(vec![node(
            1,
            ADD,
            vec![
                (ADD_A, f(1.0)),
                (ADD_B, f(4.0)),
                (_RET_PARAM_ID, selection(source.clone())),
            ],
        )])
        .unwrap();
        let mut bridge = Bridge::default();
        let tree = lower_behavior_tree(tree, &index(), &mut bridge, false).unwrap();
        assert_eq!(tree.tick(&mut bridge).unwrap(), Status::Success);
        assert_eq!(source.get(), Some(Value::F64(5.0)));
        assert_eq!(bridge.calls[0].args.len(), 2, "_ret is not passed");
    }
}

/// Optional returns map Some to a bare value and None to absence, both in a
/// local cell and in a store key, including repeated absence and recreation.
#[test]
fn optional_returns_set_and_unset_cells_and_store_keys() {
    let store = SimpleDataStore::new();
    let key = Key::from("attached/result");
    for out in [
        VariableCell::local(),
        VariableCell::stored(store.slot(&key)),
    ] {
        let mut functions = index();
        functions.get_mut(&DISTANCE).unwrap().function.return_ty =
            FrozenTy::FrozenOption(FrozenOption {
                element: Box::new(FrozenTy::from(PrimitiveKind::F64)),
            });
        let tree = load_behavior_tree_nodes(vec![node(
            1,
            DISTANCE,
            vec![(_RET_PARAM_ID, Expression::Variable(out.clone()))],
        )])
        .unwrap();
        let mut bridge = Bridge {
            responses: [
                Value::Option(Some(Box::new(Value::F64(5.0)))),
                Value::Option(None),
                Value::Option(None),
                Value::Option(Some(Box::new(Value::F64(8.0)))),
            ]
            .into_iter()
            .map(response)
            .collect(),
            ..Default::default()
        };
        let tree = lower_behavior_tree(tree, &functions, &mut bridge, false).unwrap();
        for expected in [Some(Value::F64(5.0)), None, None, Some(Value::F64(8.0))] {
            assert_eq!(tree.tick(&mut bridge).unwrap(), Status::Success);
            assert_eq!(out.get(), expected);
            if matches!(out, VariableCell::Stored(_)) {
                assert_eq!(store.read(std::slice::from_ref(&key)), vec![expected]);
            }
            assert!(bridge.calls.last().unwrap().args.is_empty());
        }
    }
}

/// A selection over an absent source passes None to an optional parameter;
/// when the source appears, the selected field passes as Some(value).
#[test]
fn selections_over_absent_values_pass_none_to_optional_parameters() {
    let source = VariableCell::local();
    let mut functions = index();
    functions
        .get_mut(&ADD)
        .unwrap()
        .function
        .parameters
        .get_mut(&ADD_B)
        .unwrap()
        .ty = FrozenTy::FrozenOption(FrozenOption {
        element: Box::new(FrozenTy::from(PrimitiveKind::F64)),
    });
    let tree = load_behavior_tree_nodes(vec![node(
        1,
        ADD,
        vec![(ADD_A, f(1.0)), (ADD_B, selection(source.clone()))],
    )])
    .unwrap();
    let mut bridge = Bridge {
        responses: VecDeque::from([
            response(Status::Success.into()),
            response(Status::Success.into()),
        ]),
        ..Default::default()
    };
    let tree = lower_behavior_tree(tree, &functions, &mut bridge, false).unwrap();
    for (value, expected) in [
        (None, Value::Option(None)),
        (
            Some(record(6.0)),
            Value::Option(Some(Box::new(Value::F64(6.0)))),
        ),
    ] {
        source.set_opt(value.clone());
        assert_eq!(tree.tick(&mut bridge).unwrap(), Status::Success);
        assert_eq!(
            bridge.calls.last().unwrap().args[1].value.as_ref(),
            &expected
        );
        assert_eq!(
            source.get(),
            value,
            "reading a selection leaves its source alone"
        );
    }
}

/// A module reporting a mutation for a read-only selected parameter must not
/// overwrite the source record, or fail because the selection has no output.
#[test]
fn reported_mutations_of_selected_parameters_leave_the_source_unchanged() {
    let source = VariableCell::local_with(record(4.0));
    let tree = load_behavior_tree_nodes(vec![node(
        1,
        ADD,
        vec![(ADD_A, selection(source.clone())), (ADD_B, f(2.0))],
    )])
    .unwrap();
    let mut bridge = Bridge {
        responses: VecDeque::from([CallResult {
            ret: Status::Success.into(),
            mutated: vec![StructureField {
                id: ADD_A,
                value: Box::new(Value::F64(100.0)),
            }],
        }]),
        ..Default::default()
    };
    let tree = lower_behavior_tree(tree, &index(), &mut bridge, false).unwrap();
    assert_eq!(tree.tick(&mut bridge).unwrap(), Status::Success);
    assert_eq!(bridge.calls[0].args[0].value.as_ref(), &Value::F64(4.0));
    assert_eq!(source.get(), Some(record(4.0)));
}

/// A module argument computed by a nested call keeps its result in its cell,
/// so a later attached node reads it through NodeArgument on every tick.
#[test]
fn nested_call_results_remain_in_argument_cells_for_later_nodes() {
    let out = VariableCell::local();
    let tree = load_behavior_tree_nodes(vec![
        control(1, SEQ_FUNCTION_ID, &[2, 3]),
        node(
            2,
            ADD,
            vec![(ADD_A, call(DISTANCE, vec![])), (ADD_B, f(1.0))],
        ),
        node(
            3,
            ADD,
            vec![
                (
                    ADD_A,
                    Expression::NodeArgument(NodeParameterId {
                        node: Uuid::from_u128(2),
                        parameter: ADD_A,
                    }),
                ),
                (ADD_B, f(3.0)),
                (_RET_PARAM_ID, Expression::Variable(out.clone())),
            ],
        ),
    ])
    .unwrap();
    let mut bridge = Bridge {
        responses: (0..2)
            .flat_map(|_| {
                [
                    response(Value::F64(2.0)),
                    response(Status::Success.into()),
                    response(Value::F64(5.0)),
                ]
            })
            .collect(),
        ..Default::default()
    };
    let tree = lower_behavior_tree(tree, &index(), &mut bridge, false).unwrap();
    for _ in 0..2 {
        assert_eq!(tree.tick(&mut bridge).unwrap(), Status::Success);
        assert_eq!(out.get(), Some(Value::F64(5.0)));
        let reads: Vec<_> = bridge
            .calls
            .drain(..)
            .filter(|call| call.id == ADD)
            .map(|call| (*call.args[0].value).clone())
            .collect();
        assert_eq!(reads, [Value::F64(2.0), Value::F64(2.0)]);
    }
}

/// Full detached nodes register even when unused, keep Success for bound
/// returns and the returned Status otherwise, and retain that status on cache
/// hits. Releasing a lowering releases its callables and their cells.
#[test]
fn full_detached_nodes_register_keep_status_and_unregister() {
    use crate::{lower_behavior_tree_with_options, LoweringOptions};
    let make = || {
        load_behavior_tree_nodes(vec![
            node(1, RUN_FUNCTION_ID, vec![]),
            node(10, ADD, vec![(ADD_A, f(1.0)), (ADD_B, f(2.0))]),
            node(
                11,
                ADD,
                vec![
                    (ADD_A, f(1.0)),
                    (ADD_B, f(2.0)),
                    (_RET_PARAM_ID, Expression::VariableId(Uuid::from_u128(99))),
                ],
            ),
            node(12, DISTANCE, vec![]),
        ])
        .unwrap()
    };
    let mut bridge = Bridge::default();
    let direct = lower_behavior_tree(make(), &index(), &mut bridge, false).unwrap();
    assert_eq!(bridge.registered.len(), 1);
    assert_eq!(direct.tree().detached_tick_id(Uuid::from_u128(10)), None);
    direct.unregister(&mut bridge);
    let tree = make();
    tree.observe_detached([Uuid::from_u128(10), Uuid::from_u128(11)]);
    let tree = lower_behavior_tree_with_options(
        tree,
        &index(),
        &mut bridge,
        LoweringOptions {
            trace: false,
            detached_tree_nodes: true,
        },
    )
    .unwrap();
    assert_eq!(bridge.registered.len(), 4);
    for id in 10..=12 {
        let id = Uuid::from_u128(id);
        let tick = tree.tree().detached_tick_id(id).unwrap();
        assert!(bridge.registered.contains_key(&tick.callable_id));
        assert_eq!(tree.tree().detached_status(id), None);
    }
    for status in [Status::Running, Status::Failure, Status::Success] {
        bridge
            .responses
            .extend([status, Status::Failure].map(|ret| CallResult {
                ret: ret.into(),
                mutated: vec![],
            }));
        for id in [10, 11] {
            tree.tree().set_argument_value(
                Uuid::from_u128(id),
                ADD_A,
                Some(Value::F64(bridge.calls.len() as f64)),
            );
        }
        assert_eq!(tree.tick(&mut bridge).unwrap(), Status::Running);
        assert_eq!(
            tree.tree().detached_status(Uuid::from_u128(10)),
            Some(status)
        );
        assert_eq!(
            tree.tree().detached_status(Uuid::from_u128(11)),
            Some(Status::Success)
        );
        let calls = bridge.calls.len();
        tree.tick(&mut bridge).unwrap();
        assert_eq!(
            bridge.calls.len(),
            calls,
            "cached ticks keep status without calling"
        );
        assert_eq!(
            tree.tree().detached_status(Uuid::from_u128(10)),
            Some(status)
        );
    }
    assert_eq!(
        tree.tree().detached_status(Uuid::from_u128(12)),
        None,
        "unused node has no status"
    );
    let weak: Vec<_> = bridge.registered.values().map(Rc::downgrade).collect();
    tree.unregister(&mut bridge);
    assert!(bridge.registered.is_empty());
    drop(tree);
    assert!(
        weak.iter().all(|function| function.upgrade().is_none()),
        "full ticks have no ownership cycle"
    );
}

/// The detached and attached trace lines for the same node are identical;
/// the cached second detached tick emits no line for its skipped module call.
#[test]
#[ignore = "run by full_detached_trace_matches_attached in a subprocess"]
fn full_detached_trace_fixture() {
    use crate::{lower_behavior_tree_with_options, LoweringOptions};
    let make_node = || {
        node(
            10,
            ADD,
            vec![
                (ADD_A, call(DISTANCE, vec![])),
                (ADD_B, f(4.0)),
                (_RET_PARAM_ID, Expression::VariableId(Uuid::from_u128(9))),
            ],
        )
    };
    let mut bridge = Bridge::default();
    let attached = lower_behavior_tree(
        load_behavior_tree_nodes(vec![make_node()]).unwrap(),
        &index(),
        &mut bridge,
        true,
    )
    .unwrap();
    attached.tick(&mut bridge).unwrap();
    attached.tick(&mut bridge).unwrap();
    attached.unregister(&mut bridge);
    let full =
        load_behavior_tree_nodes(vec![node(1, RUN_FUNCTION_ID, vec![]), make_node()]).unwrap();
    full.observe_detached([Uuid::from_u128(10)]);
    let full = lower_behavior_tree_with_options(
        full,
        &index(),
        &mut bridge,
        LoweringOptions {
            trace: true,
            detached_tree_nodes: true,
        },
    )
    .unwrap();
    full.tick(&mut bridge).unwrap();
    full.tick(&mut bridge).unwrap();
    assert_eq!(full.tree().detached_calls(Uuid::from_u128(10)), Some(1));
}

#[test]
fn full_detached_trace_matches_attached() {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "attached_tests::full_detached_trace_fixture",
            "--nocapture",
        ])
        .env_remove("RUST_TEST_THREADS")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("trace fixture exceeded ten seconds");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = String::from_utf8(result.stdout).unwrap();
    let lines: Vec<_> = output
        .lines()
        .filter_map(|line| {
            line.find("tick 00000000-0000-0000-0000-00000000000a ")
                .map(|at| &line[at..])
        })
        .collect();
    assert_eq!(
        lines.len(),
        3,
        "two attached calls, one detached call: {output}"
    );
    assert!(lines.iter().all(|line| *line == lines[0]));
    assert!(lines[0].ends_with("add(a: distance(), b: 4f64) -> Success"));
}
