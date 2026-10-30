//! The module's functions called as a loader calls them: by their ids, with
//! the arguments `functions` names, alone and on a behavior tree.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use arora_behavior_tree::nodes::SUCCEED_FUNCTION_ID;
use arora_behavior_tree::schema::{Expression, Node, NodeParameterId, _RET_PARAM_ID};
use arora_behavior_tree::{load_behavior_tree_nodes_with, lower_behavior_tree};
use arora_dataflow_nodes::eval::context::NodeRuntimeState;
use arora_dataflow_nodes::eval::plan::output_layout;
use arora_dataflow_nodes::functions::{
    dt_param, encode, encode_selector, external_function, function_id, function_index, input_param,
    node_param, output_field, placement, ports_param, select_function, selector_param, state_param,
    DataflowNodes, Placement,
};
use arora_dataflow_nodes::{vocab, Kind, Params, SelectorSegment};
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::{Call, CallBridge, CallError, CallResult, Callable, CallableId};
use arora_types::data::{DataStore, Key, StateChange};
use arora_types::value::Value;
use uuid::Uuid;

/// A bridge answering the calls the module makes to other modules: each
/// call it receives is recorded, and answered with `ret`.
#[derive(Default)]
struct Modules {
    calls: Rc<RefCell<Vec<Call>>>,
    ret: Option<Value>,
}

impl CallBridge for Modules {
    fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
        let id = call.id;
        self.calls.borrow_mut().push(call);
        match &self.ret {
            Some(ret) => Ok(CallResult {
                ret: ret.clone(),
                mutated: Vec::new(),
            }),
            None => Err(CallError::FunctionNotFound { id }),
        }
    }

    fn arora_register_callable(&mut self, _: Rc<dyn Callable>) -> CallableId {
        unreachable!("the module's bridge registers the tree's callables")
    }

    fn arora_unregister_callable(&mut self, _: &CallableId) {}

    fn arora_call_indirect(&mut self, callable_id: &CallableId) -> Result<Value, CallError> {
        Err(CallError::Internal {
            message: format!("no callable {}", callable_id.id),
        })
    }
}

/// A call of `kind`'s function with `args`, as the node `node`.
fn call(
    nodes: &mut DataflowNodes,
    kind: &Kind,
    args: &[(Uuid, Value)],
) -> Result<CallResult, CallError> {
    let node = Value::String("n".into());
    let args: Vec<(Uuid, &Value)> = std::iter::once((node_param(), &node))
        .chain(args.iter().map(|(id, value)| (*id, value)))
        .collect();
    nodes.call_borrowed(function_id(kind), &args, &mut Modules::default())
}

fn ret(result: Result<CallResult, CallError>) -> Value {
    result.expect("the call succeeds").ret
}

#[test]
fn a_function_computes_from_its_ports() {
    let mut nodes = DataflowNodes::new();
    let sum = ret(call(
        &mut nodes,
        &Kind::Add,
        &[
            (input_param("operand_0"), Value::F32(1.0)),
            (input_param("operand_1"), Value::F32(2.5)),
        ],
    ));
    assert_eq!(sum, Value::F32(3.5));
}

/// A number applies to every component of a structure, which keeps its form.
#[test]
fn arithmetic_keeps_the_form_of_a_structure() {
    let mut nodes = DataflowNodes::new();
    let sum = ret(call(
        &mut nodes,
        &Kind::Add,
        &[
            (
                input_param("operand_0"),
                arora_geometry::vec3([1.0, 2.0, 3.0]),
            ),
            (input_param("operand_1"), Value::F32(1.0)),
        ],
    ));
    assert_eq!(sum, arora_geometry::vec3([2.0, 3.0, 4.0]));
}

/// A node parameter is an argument, as `encode` gives it.
#[test]
fn a_parameter_is_an_argument() {
    let mut nodes = DataflowNodes::new();
    let params = Params {
        value: Some(Value::F32(4.0)),
        ..Default::default()
    };
    let args = encode(&params).expect("the parameters encode");
    assert_eq!(
        ret(call(&mut nodes, &Kind::Constant, &args)),
        Value::F32(4.0)
    );
}

/// A node with ports beyond `out` returns a structure of them, in the order
/// its `ports` argument lists them, which its output layout gives.
#[test]
fn a_node_of_several_ports_returns_a_structure_of_them() {
    let mut nodes = DataflowNodes::new();
    let params = Params {
        sizes: Some(vec![1.0, 2.0]),
        ..Default::default()
    };
    let ports = output_layout(&Kind::Split, &params, &[], &Default::default()).slots;
    assert_eq!(ports.len(), 2);
    let mut args = encode(&params).expect("the parameters encode");
    args.push((input_param("in"), Value::ArrayF32(vec![1.0, 2.0, 3.0])));
    args.push((ports_param(), Value::ArrayString(ports.clone())));
    let Value::Structure(parts) = ret(call(&mut nodes, &Kind::Split, &args)) else {
        panic!("a structure of the ports");
    };
    let part = |port: &str| {
        parts
            .fields
            .iter()
            .find(|field| field.id == output_field(port))
            .map(|field| (*field.value).clone())
    };
    assert_eq!(part(&ports[0]), Some(vocab::vector(vec![1.0])));
    assert_eq!(part(&ports[1]), Some(vocab::vector(vec![2.0, 3.0])));
}

/// A stateful node's state is its mutable argument `state`: a call writes it
/// back, and the next starts from it. A smoother starts at its first input.
#[test]
fn a_stateful_node_writes_its_state_back() {
    let mut nodes = DataflowNodes::new();
    let params = Params {
        half_life: Some(0.1),
        ..Default::default()
    };
    let mut state = Value::Unit;
    let mut outs = Vec::new();
    for input in [0.0, 1.0, 1.0] {
        let mut args = encode(&params).expect("the parameters encode");
        args.push((input_param("in"), Value::F32(input)));
        args.push((dt_param(), Value::F32(0.1)));
        args.push((state_param(), state.clone()));
        let result = call(&mut nodes, &Kind::Damp, &args).expect("the call succeeds");
        state = result
            .mutated
            .iter()
            .find(|field| field.id == state_param())
            .map(|field| (*field.value).clone())
            .expect("the state written back");
        outs.push(vocab::as_float(&result.ret).expect("a number"));
    }
    assert!(
        outs[0] == 0.0
            && outs
                .windows(2)
                .all(|pair| pair[0] < pair[1] && pair[1] < 1.0),
        "it moves on towards its input from where it was: {outs:?}"
    );
    assert!(matches!(
        nodes.state(&Kind::Damp, &params, Some(&state)),
        Some(NodeRuntimeState::Damp(_))
    ));
    assert!(nodes.state(&Kind::Damp, &params, None).is_none());
}

/// The runs a spawn node requests over three evaluations of `shapes`, its
/// one argument, the parameter `SHAPE`: one entry per evaluation, the
/// arguments of the run it requested, if any.
fn spawned(spawn_on_load: Option<bool>, shapes: [&str; 3]) -> Vec<Option<Vec<(Uuid, Value)>>> {
    const FUNCTION: Uuid = Uuid::from_u128(0x7771);
    const SHAPE: Uuid = Uuid::from_u128(0x7772);
    let mut nodes = DataflowNodes::new();
    let params = Params {
        function: Some(FUNCTION),
        record_keys: Some(vec![SHAPE.to_string()]),
        spawn_on_load,
        ..Default::default()
    };
    let mut state = Value::Unit;
    shapes
        .into_iter()
        .map(|shape| {
            let mut args = encode(&params).expect("the parameters encode");
            args.push((input_param("args_0"), Value::String(shape.into())));
            args.push((state_param(), state.clone()));
            let result = call(&mut nodes, &Kind::Spawn, &args).expect("the call succeeds");
            state = result
                .mutated
                .iter()
                .find(|field| field.id == state_param())
                .map(|field| (*field.value).clone())
                .expect("the state written back");
            let mut requests = nodes.take_spawns();
            assert!(requests.len() <= 1, "one run at most per evaluation");
            requests.pop().map(|request| {
                assert_eq!(request.function, FUNCTION);
                request.args
            })
        })
        .collect()
}

/// A spawn node's first evaluation records its arguments and spawns
/// nothing; a later one spawns when they change.
#[test]
fn a_spawn_node_spawns_on_a_change_of_its_arguments() {
    let shape = |text: &str| vec![(Uuid::from_u128(0x7772), Value::String(text.into()))];
    for spawn_on_load in [None, Some(false)] {
        assert_eq!(
            spawned(spawn_on_load, ["aa", "aa", "oh"]),
            [None, None, Some(shape("oh"))]
        );
    }
}

/// With `spawn_on_load`, the first evaluation spawns with the arguments as
/// loaded too, and later ones spawn on changes as before.
#[test]
fn a_spawn_node_on_load_spawns_on_its_first_evaluation_too() {
    let shape = |text: &str| vec![(Uuid::from_u128(0x7772), Value::String(text.into()))];
    assert_eq!(
        spawned(Some(true), ["sil", "sil", "aa"]),
        [Some(shape("sil")), None, Some(shape("aa"))]
    );
}

/// `spawn_on_load` is a node parameter, encoded only when set.
#[test]
fn spawn_on_load_encodes_as_a_parameter() {
    let set = Params {
        spawn_on_load: Some(true),
        ..Default::default()
    };
    let encoded = encode(&set).expect("the parameters encode");
    assert!(encoded.contains(&(
        arora_dataflow_nodes::functions::param_field("spawn_on_load"),
        Value::Boolean(true)
    )));
    assert!(encode(&Params::default())
        .expect("the parameters encode")
        .is_empty());
}

/// An integer array is a vector to the vector nodes: a call's `Vec<u8>`
/// argument computes as the numbers it holds.
#[test]
fn an_integer_array_computes_as_a_vector() {
    let mut nodes = DataflowNodes::new();
    let difference = ret(call(
        &mut nodes,
        &Kind::Subtract,
        &[
            (input_param("lhs"), Value::ArrayU8(vec![4, 26, 12])),
            (input_param("rhs"), Value::F32(12.0)),
        ],
    ));
    assert_eq!(difference, vocab::vector(vec![-8.0, 14.0, 0.0]));
    let highest = ret(call(
        &mut nodes,
        &Kind::VectorMax,
        &[(input_param("in"), Value::ArrayI32(vec![3, -1, 7]))],
    ));
    assert_eq!(highest, Value::F32(7.0));
}

/// A URDF node has no state before its first call, though the module keeps
/// the chain another node of the same configuration built.
#[cfg(feature = "urdf_ik")]
#[test]
fn a_urdf_node_has_no_state_before_its_first_call() {
    const URDF: &str = r#"<robot name="arm">
        <link name="base"/><link name="tip"/>
        <joint name="j" type="revolute">
            <parent link="base"/><child link="tip"/>
            <origin xyz="0 0 1"/><axis xyz="0 0 1"/>
            <limit lower="-3" upper="3" effort="1" velocity="1"/>
        </joint>
    </robot>"#;
    let mut nodes = DataflowNodes::new();
    let params = Params {
        urdf_xml: Some(URDF.to_string()),
        root_link: Some("base".to_string()),
        tip_link: Some("tip".to_string()),
        ..Default::default()
    };
    let mut args = encode(&params).expect("the parameters encode");
    args.push((input_param("joints"), Value::ArrayF32(vec![0.5])));
    let ports = output_layout(&Kind::UrdfFk, &params, &[], &Default::default()).slots;
    args.push((ports_param(), Value::ArrayString(ports)));
    call(&mut nodes, &Kind::UrdfFk, &args).expect("the call succeeds");
    assert!(nodes.state(&Kind::UrdfIkPosition, &params, None).is_none());
    assert!(nodes.state(&Kind::UrdfFk, &params, None).is_none());
}

#[test]
fn select_reads_the_part_a_selector_names() {
    let mut nodes = DataflowNodes::new();
    let record = vocab::record([("a", Value::F32(1.0)), ("b", Value::F32(2.0))]);
    let selector = encode_selector(&vec![SelectorSegment::Field("b".into())]);
    let selected = nodes
        .call_borrowed(
            select_function(),
            &[(input_param("in"), &record), (selector_param(), &selector)],
            &mut Modules::default(),
        )
        .expect("the selection succeeds")
        .ret;
    assert_eq!(selected, Value::F32(2.0));
}

/// A call to another module goes through the bridge the module wraps, to the
/// module its function is routed to, with its `args` ports under the
/// parameter ids the node names.
#[test]
fn an_external_call_goes_to_its_module() {
    let (module, function, parameter) =
        (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
    let params = Params {
        function: Some(function),
        param_ids: Some(vec![parameter]),
        ..Default::default()
    };
    let mut args = encode(&params).expect("the parameters encode");
    args.push((input_param("args_0"), Value::F32(3.0)));
    let args: Vec<(Uuid, &Value)> = args.iter().map(|(id, value)| (*id, value)).collect();
    let mut modules = Modules {
        ret: Some(Value::F32(7.0)),
        ..Default::default()
    };
    let calls = modules.calls.clone();
    let mut nodes = DataflowNodes::new();
    nodes.set_function_modules(HashMap::from([(function, module)]));
    let result = nodes
        .call_borrowed(external_function(), &args, &mut modules)
        .expect("the call succeeds");
    assert_eq!(result.ret, Value::F32(7.0));
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!((calls[0].module_id, calls[0].id), (Some(module), function));
    assert_eq!(calls[0].args.len(), 1);
    assert_eq!(calls[0].args[0].id, parameter);
    assert_eq!(*calls[0].args[0].value, Value::F32(3.0));
}

#[test]
fn a_tree_places_a_call_by_what_it_does() {
    let none = arora_behavior::FunctionIndex::new();
    let params = Params::default();
    assert_eq!(placement(&Kind::Add, &params, &none), Placement::Detached);
    assert_eq!(
        placement(&Kind::TaskRun, &params, &none),
        Placement::Attached
    );
    assert_eq!(
        placement(&Kind::ExternalFunction, &params, &none),
        Placement::Attached,
        "a call no description names runs at every tick"
    );
}

/// On a behavior tree, each call is a detached node: it runs when what it
/// returns is read, and again only when an argument changed.
#[test]
fn a_tree_of_detached_calls_runs_what_changed() {
    let store = SimpleDataStore::new();
    let x = Uuid::from_u128(10);
    let names = HashMap::from([(x, "x".to_string())]);
    let (gain, double) = (Uuid::from_u128(20), Uuid::from_u128(21));
    let constant = Params {
        value: Some(Value::F32(2.0)),
        ..Default::default()
    };
    let mut gain_args: HashMap<Uuid, Expression> = encode(&constant)
        .expect("the parameters encode")
        .into_iter()
        .map(|(id, value)| (id, Expression::Value(value)))
        .collect();
    gain_args.insert(
        node_param(),
        Expression::Value(Value::String("gain".into())),
    );
    let nodes = vec![
        Node {
            id: Uuid::from_u128(1),
            function: SUCCEED_FUNCTION_ID,
            arguments: HashMap::new(),
            children: None,
        },
        Node {
            id: gain,
            function: function_id(&Kind::Constant),
            arguments: gain_args,
            children: None,
        },
        Node {
            id: double,
            function: function_id(&Kind::Multiply),
            arguments: HashMap::from([
                (input_param("operand_0"), Expression::VariableId(x)),
                (
                    input_param("operand_1"),
                    Expression::NodeArgument(NodeParameterId {
                        node: gain,
                        parameter: _RET_PARAM_ID,
                    }),
                ),
                (
                    node_param(),
                    Expression::Value(Value::String("double".into())),
                ),
            ]),
            children: None,
        },
    ];
    let resolver = |name: &str| Some(store.slot(&Key::from(name)));
    let tree = load_behavior_tree_nodes_with(nodes, &resolver, &names).expect("the tree loads");
    tree.observe_detached([double]);

    let mut module = DataflowNodes::new();
    let mut modules = Modules::default();
    let mut bridge = module.bridge(&mut modules);
    let lowered =
        lower_behavior_tree(tree, function_index(), &mut bridge, false).expect("the tree lowers");
    let set_x = |value: f32| {
        let mut change = StateChange::new();
        change.set.insert(Key::from("x"), Some(Value::F32(value)));
        store.write(change).expect("the store takes it");
    };

    set_x(3.0);
    lowered.tick(&mut bridge).expect("the tree ticks");
    let tree = lowered.tree();
    assert_eq!(
        tree.detached_output(double, _RET_PARAM_ID),
        Some(Value::F32(6.0))
    );
    let calls = tree.detached_calls(double);

    lowered.tick(&mut bridge).expect("the tree ticks");
    assert_eq!(
        lowered.tree().detached_calls(double),
        calls,
        "nothing changed, so nothing ran"
    );

    set_x(5.0);
    lowered.tick(&mut bridge).expect("the tree ticks");
    assert_eq!(
        lowered.tree().detached_output(double, _RET_PARAM_ID),
        Some(Value::F32(10.0))
    );
    assert_eq!(
        lowered.tree().detached_calls(gain),
        Some(1),
        "the constant ran once"
    );
}
