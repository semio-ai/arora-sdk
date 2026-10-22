//! The node vocabulary as the functions of one Arora module,
//! [`MODULE_NAME`].
//!
//! Each node kind is one function, named by the kind's graph-JSON name
//! (`add`, `damp`, `urdfikposition`). A node of the graph is one call of it:
//!
//! - **an input port** is a parameter of the port's name (`lhs`, `operand_0`),
//!   optional and dynamically typed: a port with no edge and no default is an
//!   absent argument, as a node of the graph reads it;
//! - **a node parameter** (`half_life`, `case_labels`) is a parameter named
//!   `param.<field>`, its value the graph's literal;
//! - **the clock** is two parameters, `t` and `dt`, in seconds or, as an
//!   unsigned integer, in nanoseconds (`arora/time`, `arora/dt`);
//! - **hidden state** (a spring's position and velocity, an IK node's last
//!   solve) is the mutable parameter `state`, which the call writes back;
//! - **the outputs** are the return value: the `out` port's value for a node
//!   with that port alone, and otherwise a structure holding every port of
//!   `ports`, each under the field [`output_field`] names, `()` for a port the
//!   evaluation left unset;
//! - `node`, the node's id in the graph, names it in an error, and
//!   `output_shapes`, the JSON of its declared output shapes, has the
//!   evaluation check them.
//!
//! Parameter ids derive from the role alone ([`input_param`],
//! [`param_field`]), so a port is one id whatever function it is a parameter
//! of. A call is evaluated by the per-node code of the [reference
//! evaluator](crate::eval), so a node computes what it computes in a
//! [`GraphSpec`](crate::types::GraphSpec) evaluation.
//!
//! Besides the node kinds the module has [`SELECT`], which applies an edge's
//! selector, and [`EXTERNAL`], which calls another module's function as an
//! `externalfunction` node does.
//!
//! [`NodeGraphModule`] runs the functions in-process: [`NodeGraphModule::bridge`]
//! wraps the device's [`CallBridge`] and answers the calls to this module
//! itself, passing every other call through.
use std::collections::HashMap;
use std::sync::OnceLock;

use arora_behavior::{FunctionIndex, ModuleFunction};
use arora_shape::{Shape, TypedPath, Value};
use arora_types::call::{Call, CallBridge, CallError, CallResult, Callable, CallableId};
use arora_types::record::module::frozen::{Function, Parameter};
use arora_types::record::ty::{FrozenOption, FrozenScalar, FrozenTy};
use arora_types::record::{FrozenReference, Version};
use arora_types::value::{Structure, StructureField};
use uuid::Uuid;

use crate::eval::graph_runtime::{
    DampState, NodeRuntimeState, SlewState, SpawnRequest, SpawnState, SpringState, TaskRunState,
};
use crate::eval::plan::{input_layout, layout_of, signatures, PortLayout, VariadicRange};
use crate::eval::value_layout::flatten_numeric;
use crate::eval::{eval_node, GraphRuntime, NodeFunctions, PortValue};
use crate::types::{NodeParams, NodeSpec, NodeType, Selector, SelectorSegment};

/// The module's id.
pub const MODULE_ID: Uuid = Uuid::from_u128(0x6e6f_6465_2d67_7261_7068_0000_0000_0001);

/// The module's name.
pub const MODULE_NAME: &str = "@arora/node-graph";

/// How many ports of a variadic group (`operand_0` … `operand_63`) a function
/// declares.
pub const VARIADIC_PORTS: usize = 64;

fn derived(role: &str) -> Uuid {
    Uuid::new_v5(&MODULE_ID, role.as_bytes())
}

/// The graph-JSON name of `kind`, the name of its function.
pub fn kind_name(kind: &NodeType) -> String {
    match serde_json::to_value(kind) {
        Ok(serde_json::Value::String(name)) => name,
        _ => format!("{kind:?}").to_lowercase(),
    }
}

/// Every node kind, in the order the format lists them.
pub const KINDS: &[NodeType] = &[
    NodeType::Constant,
    NodeType::Slider,
    NodeType::MultiSlider,
    NodeType::Add,
    NodeType::Subtract,
    NodeType::Multiply,
    NodeType::Divide,
    NodeType::Power,
    NodeType::Log,
    NodeType::Abs,
    NodeType::Modulo,
    NodeType::Sqrt,
    NodeType::Sign,
    NodeType::Min,
    NodeType::Max,
    NodeType::Round,
    NodeType::Sin,
    NodeType::Cos,
    NodeType::Tan,
    NodeType::Time,
    NodeType::Oscillator,
    NodeType::Spring,
    NodeType::Damp,
    NodeType::Slew,
    NodeType::And,
    NodeType::Or,
    NodeType::Not,
    NodeType::Xor,
    NodeType::GreaterThan,
    NodeType::LessThan,
    NodeType::Equal,
    NodeType::NotEqual,
    NodeType::If,
    NodeType::Case,
    NodeType::Clamp,
    NodeType::Remap,
    NodeType::CenteredRemap,
    NodeType::PiecewiseRemap,
    NodeType::Vec3Cross,
    NodeType::VectorConstant,
    NodeType::VectorAdd,
    NodeType::VectorSubtract,
    NodeType::VectorMultiply,
    NodeType::VectorScale,
    NodeType::VectorNormalize,
    NodeType::VectorDot,
    NodeType::VectorLength,
    NodeType::VectorIndex,
    NodeType::Join,
    NodeType::Split,
    NodeType::VectorMin,
    NodeType::VectorMax,
    NodeType::VectorMean,
    NodeType::VectorMedian,
    NodeType::VectorMode,
    NodeType::ToVector,
    NodeType::FromVector,
    NodeType::SimpleNoise,
    NodeType::PerlinNoise,
    NodeType::SimplexNoise,
    NodeType::WeightedSumVector,
    NodeType::DefaultBlend,
    NodeType::BlendWeightedAverage,
    NodeType::BlendAdditive,
    NodeType::BlendMultiply,
    NodeType::BlendWeightedOverlay,
    NodeType::BlendWeightedAverageOverlay,
    NodeType::BlendMax,
    NodeType::InverseKinematics,
    NodeType::UrdfIkPosition,
    NodeType::UrdfIkPose,
    NodeType::UrdfFk,
    NodeType::BuildRecord,
    NodeType::ReadRecord,
    NodeType::SwitchRecord,
    NodeType::MergeRecord,
    NodeType::SplitRecord,
    NodeType::MathMultRecord,
    NodeType::MathAddRecord,
    NodeType::MathDivRecord,
    NodeType::MathSubRecord,
    NodeType::Input,
    NodeType::Output,
    NodeType::ExternalFunction,
    NodeType::TaskRun,
    NodeType::Spawn,
];

/// The id of the function a node of `kind` calls.
pub fn function_id(kind: &NodeType) -> Uuid {
    derived(&format!("function:{}", kind_name(kind)))
}

/// The id of the input port `port`, as a parameter of any function.
pub fn input_param(port: &str) -> Uuid {
    derived(&format!("in:{port}"))
}

/// The id of the node parameter `field` (a [`NodeParams`] field), as a
/// parameter of any function.
pub fn param_field(field: &str) -> Uuid {
    derived(&format!("param:{field}"))
}

/// The field id of the output port `port` in the structure a multi-port
/// node returns.
pub fn output_field(port: &str) -> Uuid {
    derived(&format!("out:{port}"))
}

/// The structure id of a multi-port node's outputs.
pub fn outputs_structure() -> Uuid {
    derived("structure:outputs")
}

/// The parameter holding a stateful node's state, which a call writes back.
pub fn state_param() -> Uuid {
    derived("state")
}

/// The parameter holding the graph's time.
pub fn time_param() -> Uuid {
    derived("t")
}

/// The parameter holding the step's duration.
pub fn dt_param() -> Uuid {
    derived("dt")
}

/// The parameter listing a multi-port node's output ports, in order.
pub fn ports_param() -> Uuid {
    derived("ports")
}

/// The parameter holding a node's declared output shapes, as JSON.
pub fn output_shapes_param() -> Uuid {
    derived("output_shapes")
}

/// The parameter holding the node's id in the graph.
pub fn node_param() -> Uuid {
    derived("node")
}

/// The parameter of [`SELECT`] holding the selector, as an array of field
/// names (strings) and indices (`u64`).
pub fn selector_param() -> Uuid {
    derived("selector")
}

/// The function applying an edge's selector to the value it reads:
/// `select(in, selector)`.
pub fn select_function() -> Uuid {
    derived("function:select")
}

/// The name of [`select_function`].
pub const SELECT: &str = "select";

/// The function an `externalfunction` node calls: it calls the function
/// `param.function` with its `args_<i>` inputs as the parameters
/// `param.param_ids` names, and returns what it returns.
pub fn external_function() -> Uuid {
    derived("function:external")
}

/// The name of [`external_function`].
pub const EXTERNAL: &str = "external";

/// The fixed input ports the functions declare: every port a node kind's
/// signature names or its evaluation reads.
const FIXED_INPUTS: &[&str] = &[
    "a",
    "args",
    "b",
    "base",
    "baseline",
    "cond",
    "default",
    "else",
    "exp",
    "fallback",
    "frequency",
    "in",
    "in_max",
    "in_min",
    "in_anchor",
    "in_high",
    "in_low",
    "index",
    "input_breakpoints",
    "joints",
    "lhs",
    "masks",
    "max",
    "max_effective_weight",
    "min",
    "offset",
    "out_anchor",
    "out_high",
    "out_low",
    "out_max",
    "out_min",
    "output_breakpoints",
    "phase",
    "rhs",
    "scalar",
    "seed",
    "selector",
    "switch",
    "target_pos",
    "target_rot",
    "then",
    "theta",
    "total_weight",
    "total_weighted_sum",
    "v",
    "value",
    "values",
    "weights",
    "when",
    "x",
    "y",
];

/// A [`NodeParams`] field: its name and how its value is a [`Value`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FieldKind {
    /// Any value, as it is.
    Value,
    /// `f32`, for a field that is not optional.
    F32,
    OptF32,
    OptU32,
    OptBool,
    OptString,
    /// `Vec<f32>`, as `ArrayF32`.
    OptVecF32,
    /// `Vec<String>`, as `ArrayString`.
    OptVecString,
    /// A typed path, as its string.
    OptPath,
    /// A uuid, as its string.
    OptUuid,
    /// Uuids, as `ArrayString`.
    OptVecUuid,
    /// A rounding mode, as its JSON name.
    OptRoundMode,
    /// Any other serde value, as its JSON text.
    OptJson,
}

const FIELDS: &[(&str, FieldKind)] = &[
    ("value", FieldKind::Value),
    ("frequency", FieldKind::OptF32),
    ("phase", FieldKind::OptF32),
    ("min", FieldKind::F32),
    ("max", FieldKind::F32),
    ("x", FieldKind::OptF32),
    ("y", FieldKind::OptF32),
    ("z", FieldKind::OptF32),
    ("in_min", FieldKind::OptF32),
    ("in_max", FieldKind::OptF32),
    ("out_min", FieldKind::OptF32),
    ("out_max", FieldKind::OptF32),
    ("round_mode", FieldKind::OptRoundMode),
    ("clamp", FieldKind::OptBool),
    ("in_low", FieldKind::OptF32),
    ("in_anchor", FieldKind::OptF32),
    ("in_high", FieldKind::OptF32),
    ("out_low", FieldKind::OptF32),
    ("out_anchor", FieldKind::OptF32),
    ("out_high", FieldKind::OptF32),
    ("bone1", FieldKind::OptF32),
    ("bone2", FieldKind::OptF32),
    ("bone3", FieldKind::OptF32),
    ("urdf_xml", FieldKind::OptString),
    ("root_link", FieldKind::OptString),
    ("tip_link", FieldKind::OptString),
    ("seed", FieldKind::OptVecF32),
    ("weights", FieldKind::OptVecF32),
    ("max_iters", FieldKind::OptU32),
    ("tol_pos", FieldKind::OptF32),
    ("tol_rot", FieldKind::OptF32),
    ("joint_defaults", FieldKind::OptJson),
    ("index", FieldKind::OptF32),
    ("sizes", FieldKind::OptVecF32),
    ("noise_seed", FieldKind::OptF32),
    ("octaves", FieldKind::OptF32),
    ("lacunarity", FieldKind::OptF32),
    ("persistence", FieldKind::OptF32),
    ("stiffness", FieldKind::OptF32),
    ("damping", FieldKind::OptF32),
    ("mass", FieldKind::OptF32),
    ("half_life", FieldKind::OptF32),
    ("max_rate", FieldKind::OptF32),
    ("case_labels", FieldKind::OptVecString),
    ("record_keys", FieldKind::OptVecString),
    ("keys", FieldKind::OptString),
    ("path", FieldKind::OptPath),
    ("function", FieldKind::OptUuid),
    ("param_ids", FieldKind::OptVecUuid),
    ("key_field", FieldKind::OptUuid),
    ("value_field", FieldKind::OptUuid),
    ("module", FieldKind::OptUuid),
];

/// What a parameter id stands for in a call.
#[derive(Clone, Debug)]
enum Role {
    Input(String),
    Field(usize),
    State,
    Time,
    Dt,
    Ports,
    OutputShapes,
    Node,
    Selector,
}

/// Every parameter id's role.
fn roles() -> &'static HashMap<Uuid, Role> {
    static ROLES: OnceLock<HashMap<Uuid, Role>> = OnceLock::new();
    ROLES.get_or_init(|| {
        let mut roles = HashMap::new();
        for port in all_input_ports() {
            roles.insert(input_param(&port), Role::Input(port));
        }
        for (at, (field, _)) in FIELDS.iter().enumerate() {
            roles.insert(param_field(field), Role::Field(at));
        }
        roles.insert(state_param(), Role::State);
        roles.insert(time_param(), Role::Time);
        roles.insert(dt_param(), Role::Dt);
        roles.insert(ports_param(), Role::Ports);
        roles.insert(output_shapes_param(), Role::OutputShapes);
        roles.insert(node_param(), Role::Node);
        roles.insert(selector_param(), Role::Selector);
        roles
    })
}

/// The kinds by function id.
fn kinds() -> &'static HashMap<Uuid, NodeType> {
    static KINDS_BY_ID: OnceLock<HashMap<Uuid, NodeType>> = OnceLock::new();
    KINDS_BY_ID.get_or_init(|| {
        KINDS
            .iter()
            .map(|kind| (function_id(kind), kind.clone()))
            .collect()
    })
}

/// The kind whose function is `function`.
pub fn kind_of(function: Uuid) -> Option<&'static NodeType> {
    kinds().get(&function)
}

/// The variadic input group of `kind`, if it has one.
fn variadic_input(kind: &NodeType) -> Option<&'static str> {
    signatures()
        .get(kind)
        .and_then(|signature| signature.variadic_inputs.as_ref())
        .map(|group| group.id)
}

/// The ports of a variadic group a function declares: the bare group name,
/// and `<group>_<i>` for every `i` below [`VARIADIC_PORTS`].
fn variadic_ports(group: &str) -> impl Iterator<Item = String> + '_ {
    std::iter::once(group.to_string())
        .chain((0..VARIADIC_PORTS).map(move |i| format!("{group}_{i}")))
}

/// Every input port some function declares.
fn all_input_ports() -> Vec<String> {
    let mut ports: Vec<String> = FIXED_INPUTS.iter().map(|port| port.to_string()).collect();
    for signature in signatures().values() {
        ports.extend(signature.inputs.iter().map(|port| port.id.to_string()));
        if let Some(group) = &signature.variadic_inputs {
            ports.extend(variadic_ports(group.id));
        }
    }
    ports.sort();
    ports.dedup();
    ports
}

/// The input ports `kind`'s function declares, in order: its signature's
/// ports, every other fixed port, then its variadic group.
pub fn declared_inputs(kind: &NodeType) -> Vec<String> {
    let mut ports: Vec<String> = signatures()
        .get(kind)
        .map(|signature| {
            signature
                .inputs
                .iter()
                .map(|port| port.id.to_string())
                .collect()
        })
        .unwrap_or_default();
    for port in FIXED_INPUTS {
        if !ports.iter().any(|known| known == port) {
            ports.push(port.to_string());
        }
    }
    if let Some(group) = variadic_input(kind) {
        ports.extend(variadic_ports(group));
    }
    ports
}

/// Whether a node of `kind` keeps state from one call to the next in its
/// `state` parameter.
pub fn is_stateful(kind: &NodeType) -> bool {
    matches!(
        kind,
        NodeType::Spring
            | NodeType::Damp
            | NodeType::Slew
            | NodeType::UrdfIkPosition
            | NodeType::UrdfIkPose
            | NodeType::TaskRun
            | NodeType::Spawn
    )
}

/// Whether a node of `kind` reads the graph's time.
pub fn reads_time(kind: &NodeType) -> bool {
    matches!(kind, NodeType::Time | NodeType::Oscillator)
}

/// Whether a node of `kind` reads the step's duration.
pub fn reads_dt(kind: &NodeType) -> bool {
    matches!(kind, NodeType::Spring | NodeType::Damp | NodeType::Slew)
}

/// Whether a node of `kind` acts each time it is evaluated, so the tree
/// ticks it at every tick rather than when its inputs change: a call to
/// another module, a task run, a spawn.
pub fn is_action(kind: &NodeType) -> bool {
    matches!(
        kind,
        NodeType::ExternalFunction | NodeType::TaskRun | NodeType::Spawn
    )
}

/// A dynamically typed value: the well-known key-value type, an opaque bag
/// of values.
fn dynamic() -> FrozenTy {
    FrozenTy::FrozenScalar(FrozenScalar {
        reference: FrozenReference {
            id: *arora_types::ty::KEY_VALUE_ID,
            version: Version::parse("1.0.0").expect("a version"),
        },
    })
}

fn optional(element: FrozenTy) -> FrozenTy {
    FrozenTy::FrozenOption(FrozenOption {
        element: Box::new(element),
    })
}

fn describe(id: Uuid, name: &str, parameters: Vec<(Uuid, String, bool)>) -> ModuleFunction {
    ModuleFunction {
        module_id: MODULE_ID,
        function_id: id,
        function_name: name.to_string(),
        function: Function {
            parameter_ordering: parameters.iter().map(|(id, ..)| *id).collect(),
            parameters: parameters
                .into_iter()
                .map(|(id, name, mutable)| {
                    (
                        id,
                        Parameter {
                            name,
                            ty: optional(dynamic()),
                            mutable,
                        },
                    )
                })
                .collect(),
            return_ty: dynamic(),
        },
    }
}

/// The description of `kind`'s function.
pub fn describe_kind(kind: &NodeType) -> ModuleFunction {
    let mut parameters: Vec<(Uuid, String, bool)> = declared_inputs(kind)
        .into_iter()
        .map(|port| (input_param(&port), port, false))
        .collect();
    parameters.extend(
        FIELDS
            .iter()
            .map(|(field, _)| (param_field(field), format!("param.{field}"), false)),
    );
    parameters.push((time_param(), "t".into(), false));
    parameters.push((dt_param(), "dt".into(), false));
    parameters.push((state_param(), "state".into(), true));
    parameters.push((ports_param(), "ports".into(), false));
    parameters.push((output_shapes_param(), "output_shapes".into(), false));
    parameters.push((node_param(), "node".into(), false));
    describe(function_id(kind), &kind_name(kind), parameters)
}

/// The module's functions: one per node kind, [`SELECT`] and [`EXTERNAL`].
pub fn function_index() -> &'static FunctionIndex {
    static INDEX: OnceLock<FunctionIndex> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut index: FunctionIndex = KINDS
            .iter()
            .map(|kind| (function_id(kind), describe_kind(kind)))
            .collect();
        index.insert(
            select_function(),
            describe(
                select_function(),
                SELECT,
                vec![
                    (input_param("in"), "in".into(), false),
                    (selector_param(), "selector".into(), false),
                    (node_param(), "node".into(), false),
                ],
            ),
        );
        index.insert(
            external_function(),
            describe_kind_as(&NodeType::ExternalFunction, EXTERNAL),
        );
        index
    })
}

fn describe_kind_as(kind: &NodeType, name: &str) -> ModuleFunction {
    let mut described = describe_kind(kind);
    described.function_id = external_function();
    described.function_name = name.to_string();
    described
}

// Node parameters as values.
//====================================================================

/// The node parameters of `params` that differ from their default, each as
/// its parameter id and its value: the literal arguments of a node's call.
pub fn encode_params(params: &NodeParams) -> Result<Vec<(Uuid, Value)>, String> {
    let json = serde_json::to_value(params).map_err(|e| e.to_string())?;
    let mut encoded = Vec::new();
    for (field, kind) in FIELDS {
        let Some(value) = json.get(*field).filter(|value| !value.is_null()) else {
            continue;
        };
        let encoded_value = match kind {
            FieldKind::Value => params.value.clone().expect("a non-null value"),
            FieldKind::F32 => {
                let number = value.as_f64().unwrap_or(0.0) as f32;
                if number == 0.0 {
                    continue;
                }
                Value::F32(number)
            }
            FieldKind::OptF32 => Value::F32(value.as_f64().unwrap_or(0.0) as f32),
            FieldKind::OptU32 => Value::U32(value.as_u64().unwrap_or(0) as u32),
            FieldKind::OptBool => Value::Boolean(value.as_bool().unwrap_or(false)),
            FieldKind::OptString | FieldKind::OptPath | FieldKind::OptUuid => {
                Value::String(value.as_str().unwrap_or_default().to_string())
            }
            FieldKind::OptRoundMode => {
                Value::String(value.as_str().unwrap_or_default().to_string())
            }
            FieldKind::OptVecF32 => Value::ArrayF32(
                value
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .map(|item| item.as_f64().unwrap_or(0.0) as f32)
                            .collect()
                    })
                    .unwrap_or_default(),
            ),
            FieldKind::OptVecString | FieldKind::OptVecUuid => Value::ArrayString(
                value
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .map(|item| item.as_str().unwrap_or_default().to_string())
                            .collect()
                    })
                    .unwrap_or_default(),
            ),
            FieldKind::OptJson => Value::String(value.to_string()),
        };
        encoded.push((param_field(field), encoded_value));
    }
    Ok(encoded)
}

fn as_f32(value: &Value) -> Option<f32> {
    match value {
        Value::F32(v) => Some(*v),
        Value::F64(v) => Some(*v as f32),
        Value::I32(v) => Some(*v as f32),
        Value::I64(v) => Some(*v as f32),
        Value::U32(v) => Some(*v as f32),
        Value::U64(v) => Some(*v as f32),
        _ => None,
    }
}

fn as_string(value: &Value) -> Option<&str> {
    match value {
        Value::String(text) => Some(text),
        _ => None,
    }
}

fn as_strings(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::ArrayString(items) => Some(items.clone()),
        Value::ArrayValue(items) => items
            .iter()
            .map(|item| as_string(item).map(str::to_string))
            .collect(),
        _ => None,
    }
}

fn as_floats(value: &Value) -> Option<Vec<f32>> {
    match value {
        Value::ArrayF32(items) => Some(items.clone()),
        Value::ArrayF64(items) => Some(items.iter().map(|v| *v as f32).collect()),
        Value::ArrayValue(items) => items.iter().map(as_f32).collect(),
        _ => None,
    }
}

fn parse_uuid(text: &str) -> Result<Uuid, String> {
    Uuid::parse_str(text).map_err(|e| format!("'{text}' is not a uuid: {e}"))
}

/// Set the field at `at` of `params` from its value.
fn set_field(params: &mut NodeParams, at: usize, value: Value) -> Result<(), String> {
    let (field, kind) = FIELDS[at];
    let wrong = || format!("param.{field} does not take {value:?}");
    let f32_of = || as_f32(&value).ok_or_else(wrong);
    let string_of = || as_string(&value).map(str::to_string).ok_or_else(wrong);
    match (field, kind) {
        ("value", _) => params.value = Some(value.clone()),
        ("min", _) => params.min = f32_of()?,
        ("max", _) => params.max = f32_of()?,
        ("max_iters", _) => params.max_iters = Some(f32_of()? as u32),
        ("clamp", _) => {
            params.clamp = Some(match value {
                Value::Boolean(on) => on,
                _ => return Err(wrong()),
            })
        }
        ("round_mode", _) => {
            params.round_mode = Some(
                serde_json::from_value(serde_json::Value::String(string_of()?))
                    .map_err(|e| e.to_string())?,
            )
        }
        ("joint_defaults", _) => {
            params.joint_defaults =
                Some(serde_json::from_str(&string_of()?).map_err(|e| e.to_string())?)
        }
        ("path", _) => {
            params.path = Some(TypedPath::parse(&string_of()?).map_err(|e| e.to_string())?)
        }
        ("urdf_xml", _) => params.urdf_xml = Some(string_of()?),
        ("root_link", _) => params.root_link = Some(string_of()?),
        ("tip_link", _) => params.tip_link = Some(string_of()?),
        ("keys", _) => params.keys = Some(string_of()?),
        ("seed", _) => params.seed = Some(as_floats(&value).ok_or_else(wrong)?),
        ("weights", _) => params.weights = Some(as_floats(&value).ok_or_else(wrong)?),
        ("sizes", _) => params.sizes = Some(as_floats(&value).ok_or_else(wrong)?),
        ("case_labels", _) => params.case_labels = Some(as_strings(&value).ok_or_else(wrong)?),
        ("record_keys", _) => params.record_keys = Some(as_strings(&value).ok_or_else(wrong)?),
        ("function", _) => params.function = Some(parse_uuid(&string_of()?)?),
        ("key_field", _) => params.key_field = Some(parse_uuid(&string_of()?)?),
        ("value_field", _) => params.value_field = Some(parse_uuid(&string_of()?)?),
        ("module", _) => params.module = Some(parse_uuid(&string_of()?)?),
        ("param_ids", _) => {
            params.param_ids = Some(
                as_strings(&value)
                    .ok_or_else(wrong)?
                    .iter()
                    .map(|id| parse_uuid(id))
                    .collect::<Result<_, _>>()?,
            )
        }
        (field, FieldKind::OptF32) => {
            let number = Some(f32_of()?);
            match field {
                "frequency" => params.frequency = number,
                "phase" => params.phase = number,
                "x" => params.x = number,
                "y" => params.y = number,
                "z" => params.z = number,
                "in_min" => params.in_min = number,
                "in_max" => params.in_max = number,
                "out_min" => params.out_min = number,
                "out_max" => params.out_max = number,
                "in_low" => params.in_low = number,
                "in_anchor" => params.in_anchor = number,
                "in_high" => params.in_high = number,
                "out_low" => params.out_low = number,
                "out_anchor" => params.out_anchor = number,
                "out_high" => params.out_high = number,
                "bone1" => params.bone1 = number,
                "bone2" => params.bone2 = number,
                "bone3" => params.bone3 = number,
                "tol_pos" => params.tol_pos = number,
                "tol_rot" => params.tol_rot = number,
                "index" => params.index = number,
                "noise_seed" => params.noise_seed = number,
                "octaves" => params.octaves = number,
                "lacunarity" => params.lacunarity = number,
                "persistence" => params.persistence = number,
                "stiffness" => params.stiffness = number,
                "damping" => params.damping = number,
                "mass" => params.mass = number,
                "half_life" => params.half_life = number,
                "max_rate" => params.max_rate = number,
                other => return Err(format!("param.{other} is not a number field")),
            }
        }
        (other, _) => return Err(format!("param.{other} has no decoding")),
    }
    Ok(())
}

/// A selector as the value of [`SELECT`]'s `selector` argument.
pub fn encode_selector(selector: &Selector) -> Value {
    Value::ArrayValue(
        selector
            .iter()
            .map(|segment| match segment {
                SelectorSegment::Field(field) => Value::String(field.clone()),
                SelectorSegment::Index(index) => Value::U64(*index as u64),
            })
            .collect(),
    )
}

fn decode_selector(value: &Value) -> Result<Selector, String> {
    let Value::ArrayValue(segments) = value else {
        return Err(format!("a selector is an array of segments, not {value:?}"));
    };
    segments
        .iter()
        .map(|segment| match segment {
            Value::String(field) => Ok(SelectorSegment::Field(field.clone())),
            Value::U64(index) => Ok(SelectorSegment::Index(*index as usize)),
            other => Err(format!(
                "a selector segment is a field or an index, not {other:?}"
            )),
        })
        .collect()
}

/// Seconds from a clock argument: a float in seconds, an unsigned integer in
/// nanoseconds.
fn seconds(value: &Value) -> f32 {
    match value {
        Value::U64(nanos) => (*nanos as f64 / 1e9) as f32,
        Value::U32(nanos) => (*nanos as f64 / 1e9) as f32,
        Value::I64(nanos) => (*nanos as f64 / 1e9) as f32,
        Value::F32(seconds) => *seconds,
        Value::F64(seconds) => *seconds as f32,
        _ => 0.0,
    }
}

/// The value an argument carries: an optional's content, `None` for an
/// absent one or `()`.
fn present(value: Value) -> Option<Value> {
    match value {
        Value::Option(Some(value)) => present(*value),
        Value::Option(None) | Value::Unit => None,
        value => Some(value),
    }
}

// State as values.
//====================================================================

fn some(value: Option<Value>) -> Value {
    Value::Option(value.map(Box::new))
}

/// A node's runtime state as the value of its `state` parameter: the
/// smoothing nodes' buffers as values of their input's shape, an IK node's
/// joints and last solve, a task run's latched status and outputs, a spawn's
/// last arguments.
fn encode_state(state: NodeRuntimeState) -> Value {
    match state {
        NodeRuntimeState::Spring(state) => Value::ArrayValue(vec![
            state.layout.reconstruct(&state.position),
            state.layout.reconstruct(&state.velocity),
            state.layout.reconstruct(&state.target),
        ]),
        NodeRuntimeState::Damp(state) => state.layout.reconstruct(&state.value),
        NodeRuntimeState::Slew(state) => state.layout.reconstruct(&state.value),
        #[cfg(feature = "urdf_ik")]
        NodeRuntimeState::UrdfKinematics(state) => crate::eval::urdfik::encode_state(&state),
        NodeRuntimeState::TaskRun(state) => {
            Value::ArrayValue(vec![some(state.latched), some(state.outputs)])
        }
        NodeRuntimeState::Spawn(state) => Value::ArrayValue(vec![
            Value::Structure(Structure {
                id: outputs_structure(),
                fields: state
                    .args
                    .into_iter()
                    .map(|(id, value)| StructureField {
                        id,
                        value: Box::new(value),
                    })
                    .collect(),
            }),
            some(state.when),
        ]),
    }
}

/// The runtime state `value` encodes for a node of `kind`, `None` for one it
/// does not encode (a node's first call).
fn decode_state(kind: &NodeType, value: Value) -> Option<NodeRuntimeState> {
    let flat = |value: &Value| flatten_numeric(value);
    match (kind, value) {
        (NodeType::Spring, Value::ArrayValue(parts)) if parts.len() == 3 => {
            let (position, velocity, target) =
                (flat(&parts[0])?, flat(&parts[1])?, flat(&parts[2])?);
            Some(NodeRuntimeState::Spring(SpringState {
                layout: position.layout,
                position: position.data,
                velocity: velocity.data,
                target: target.data,
            }))
        }
        (NodeType::Damp, value) => {
            let value = flat(&value)?;
            Some(NodeRuntimeState::Damp(DampState {
                layout: value.layout,
                value: value.data,
            }))
        }
        (NodeType::Slew, value) => {
            let value = flat(&value)?;
            Some(NodeRuntimeState::Slew(SlewState {
                layout: value.layout,
                value: value.data,
            }))
        }
        (NodeType::TaskRun, Value::ArrayValue(parts)) if parts.len() == 2 => {
            let mut parts = parts.into_iter();
            Some(NodeRuntimeState::TaskRun(TaskRunState {
                latched: parts.next().and_then(present),
                outputs: parts.next().and_then(present),
            }))
        }
        (NodeType::Spawn, Value::ArrayValue(parts)) if parts.len() == 2 => {
            let mut parts = parts.into_iter();
            let args = match parts.next()? {
                Value::Structure(args) => args
                    .fields
                    .into_iter()
                    .map(|field| (field.id, *field.value))
                    .collect(),
                _ => return None,
            };
            Some(NodeRuntimeState::Spawn(SpawnState {
                args,
                when: parts.next().and_then(present),
            }))
        }
        _ => None,
    }
}

// The module.
//====================================================================

/// The functions of [`MODULE_NAME`], run in-process, and what they keep
/// between calls that is no node's state: each URDF chain built, by its
/// configuration, and the runs `spawn` nodes requested.
#[derive(Default)]
pub struct NodeGraphModule {
    /// The module each external function belongs to, for `external` and
    /// `taskrun` calls naming no module.
    function_modules: HashMap<Uuid, Uuid>,
    /// The module of a function `function_modules` does not route.
    default_module: Option<Uuid>,
    /// The URDF chains built, by the hash of their configuration.
    #[cfg(feature = "urdf_ik")]
    pub(crate) chains: crate::eval::urdfik::Chains,
    /// The runs `spawn` nodes requested since they were last taken.
    spawns: Vec<SpawnRequest>,
    /// The input layouts built, by function and the ports a call passes.
    layouts: HashMap<(Uuid, Vec<Uuid>), PortLayout>,
    /// The callables registered through a [`Bridge`], by id: a tree's node
    /// ticks, which call back through the bridge that runs them.
    callables: HashMap<u64, std::rc::Rc<dyn Callable>>,
    /// The id the next callable registered gets. Ids start at
    /// [`CALLABLE_IDS`], apart from the wrapped bridge's own.
    next_callable: u64,
}

/// Where the ids of the callables a [`Bridge`] registers start.
const CALLABLE_IDS: u64 = 1 << 62;

impl NodeGraphModule {
    /// A module with no function routed to another module.
    pub fn new() -> Self {
        Self::default()
    }

    /// Route each function of `function_modules` to its module, for the
    /// `external` and `taskrun` calls that name no module.
    pub fn set_function_modules(&mut self, function_modules: HashMap<Uuid, Uuid>) {
        self.function_modules = function_modules;
    }

    /// Route every function `function_modules` does not to `module`.
    pub fn set_default_module(&mut self, module: Option<Uuid>) {
        self.default_module = module;
    }

    /// The runs the `spawn` nodes requested since this was last asked, in
    /// the order they requested them.
    pub fn take_spawns(&mut self) -> Vec<SpawnRequest> {
        std::mem::take(&mut self.spawns)
    }

    /// A bridge answering this module's calls and passing every other to
    /// `inner`.
    pub fn bridge<'a>(&'a mut self, inner: &'a mut dyn CallBridge) -> Bridge<'a> {
        Bridge {
            module: self,
            inner,
        }
    }

    /// Answer `call`, a call to one of this module's functions; `inner` runs
    /// the calls a function makes to other modules.
    pub fn call(
        &mut self,
        call: Call,
        inner: &mut dyn CallBridge,
    ) -> Result<CallResult, CallError> {
        self.evaluate(call, inner)
            .map_err(|message| CallError::Generic { message })
    }

    fn evaluate(&mut self, call: Call, inner: &mut dyn CallBridge) -> Result<CallResult, String> {
        let function = call.id;
        let kind = if function == external_function() {
            NodeType::ExternalFunction
        } else if function == select_function() {
            return select(call);
        } else {
            kind_of(function)
                .cloned()
                .ok_or_else(|| format!("{MODULE_NAME} has no function {function}"))?
        };
        let roles = roles();
        let mut inputs: Vec<(String, Value)> = Vec::new();
        let mut params = NodeParams::default();
        let mut state = None;
        let (mut t, mut dt) = (0.0, 0.0);
        let mut ports: Option<Vec<String>> = None;
        let mut output_shapes = HashMap::new();
        let mut node = String::new();
        let mut input_ids = Vec::new();
        for argument in call.args {
            let Some(role) = roles.get(&argument.id) else {
                return Err(format!(
                    "{} has no parameter {}",
                    kind_name(&kind),
                    argument.id
                ));
            };
            let Some(value) = present(*argument.value) else {
                continue;
            };
            match role {
                Role::Input(port) => {
                    input_ids.push(argument.id);
                    inputs.push((port.clone(), value));
                }
                Role::Field(at) => set_field(&mut params, *at, value)?,
                Role::State => state = Some(value),
                Role::Time => t = seconds(&value),
                Role::Dt => dt = seconds(&value),
                Role::Ports => ports = as_strings(&value),
                Role::OutputShapes => {
                    output_shapes = serde_json::from_str(as_string(&value).unwrap_or("{}"))
                        .map_err(|e| format!("output_shapes: {e}"))?
                }
                Role::Node => node = as_string(&value).unwrap_or_default().to_string(),
                Role::Selector => {}
            }
        }
        let spec = NodeSpec {
            id: node,
            kind: kind.clone(),
            params,
            output_shapes: output_shapes
                .into_iter()
                .collect::<hashbrown::HashMap<String, Shape>>(),
            input_defaults: Default::default(),
        };

        // An input node's value is what the host staged on its path.
        let staged = if kind == NodeType::Input {
            let at = inputs.iter().position(|(port, _)| port == "in");
            at.map(|at| {
                input_ids.remove(at);
                inputs.remove(at).1
            })
        } else {
            None
        };

        // The inputs in the layout the plan gives them.
        let layout = self
            .layouts
            .entry((function, input_ids))
            .or_insert_with(|| input_layout(&kind, inputs.iter().map(|(port, _)| port.as_str())))
            .clone();
        let mut slots = vec![PortValue::default(); layout.slots.len()];
        let mut present_slots = vec![false; layout.slots.len()];
        for (port, value) in inputs {
            if let Some(slot) = layout.slot(&port) {
                slots[slot] = PortValue::new(value);
                present_slots[slot] = true;
            }
        }
        let inputs = eval_node::InputSlots::new(&slots, &present_slots, &layout);

        let ports = ports.unwrap_or_else(|| vec!["out".to_string()]);
        let variadics = output_variadics(&kind, &ports);
        let output_layout = layout_of(ports.iter().cloned(), variadics);
        let mut out = Vec::new();
        let mut rt = GraphRuntime {
            t,
            dt,
            ..GraphRuntime::default()
        };
        if let (Some(value), Some(path)) = (staged, &spec.params.path) {
            rt.set_input(path.clone(), value, None);
            rt.advance_epoch();
        }
        #[cfg(feature = "urdf_ik")]
        if matches!(
            kind,
            NodeType::UrdfIkPosition | NodeType::UrdfIkPose | NodeType::UrdfFk
        ) {
            if let Some(prepared) =
                crate::eval::urdfik::prepared_state(&self.chains, &spec.params, state.as_ref())
            {
                rt.node_states
                    .insert(spec.id.clone(), NodeRuntimeState::UrdfKinematics(prepared));
            }
            state = None;
        }
        if let Some(state) = state.and_then(|state| decode_state(&kind, state)) {
            rt.node_states.insert(spec.id.clone(), state);
        }
        {
            let mut outputs = eval_node::OutputSlots::new(&mut out, &output_layout);
            let mut host = ModuleFunctions {
                bridge: inner,
                function_modules: &self.function_modules,
                default_module: self.default_module,
            };
            eval_node::eval_node_inner(&mut rt, &spec, &inputs, &mut outputs, Some(&mut host))?;
        }
        self.spawns.append(&mut rt.spawns);
        #[cfg(feature = "urdf_ik")]
        if let Some(NodeRuntimeState::UrdfKinematics(state)) = rt.node_states.get(&spec.id) {
            crate::eval::urdfik::keep_chain(&mut self.chains, state);
        }

        let ret = if ports.len() == 1 && ports[0] == "out" {
            out.into_iter()
                .next()
                .map(|port| port.value)
                .unwrap_or(Value::Unit)
        } else {
            let mut values = out.into_iter();
            Value::Structure(Structure {
                id: outputs_structure(),
                fields: ports
                    .iter()
                    .map(|port| StructureField {
                        id: output_field(port),
                        value: Box::new(values.next().map_or(Value::Unit, |port| port.value)),
                    })
                    .collect(),
            })
        };
        let mutated = if is_stateful(&kind) {
            rt.node_states
                .remove(&spec.id)
                .map(|state| {
                    vec![StructureField {
                        id: state_param(),
                        value: Box::new(encode_state(state)),
                    }]
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(CallResult { ret, mutated })
    }
}

/// The variadic output groups a node of `kind` with the output ports
/// `ports` has, each with its range of slots: where its evaluation writes
/// them.
fn output_variadics(kind: &NodeType, ports: &[String]) -> Vec<(String, VariadicRange)> {
    let Some(group) = signatures()
        .get(kind)
        .and_then(|signature| signature.variadic_outputs.as_ref())
    else {
        return Vec::new();
    };
    let id = group.id;
    let stem = id.strip_suffix('s').unwrap_or(id);
    let belongs = |port: &str| {
        crate::eval::variadic::parse_variadic_key(port).0 == id
            || (stem != id
                && port
                    .strip_prefix(stem)
                    .is_some_and(|tail| tail.parse::<usize>().is_ok()))
    };
    let Some(start) = ports.iter().position(|port| belongs(port)) else {
        return Vec::new();
    };
    let len = ports[start..]
        .iter()
        .take_while(|port| belongs(port))
        .count();
    let range = VariadicRange { start, len };
    let mut groups = vec![(id.to_string(), range)];
    if stem != id {
        groups.push((stem.to_string(), range));
    }
    groups
}

/// [`SELECT`]: the part of its input its selector names.
fn select(call: Call) -> Result<CallResult, String> {
    let mut input = None;
    let mut selector = None;
    let mut node = String::new();
    for argument in call.args {
        let value = present(*argument.value);
        if argument.id == input_param("in") {
            input = value;
        } else if argument.id == selector_param() {
            selector = value;
        } else if argument.id == node_param() {
            node = value
                .as_ref()
                .and_then(as_string)
                .unwrap_or_default()
                .to_string();
        }
    }
    let selector = decode_selector(&selector.unwrap_or(Value::ArrayValue(Vec::new())))?;
    let input = input.unwrap_or_else(|| arora_shape::value::float(0.0));
    let shape = crate::eval::shape_helpers::infer_shape_id(&input);
    let (value, _) =
        crate::eval::shape_helpers::project_by_selector(&input, Some(&shape), &selector)
            .map_err(|error| format!("selector {selector:?} into node '{node}' failed: {error}"))?;
    Ok(CallResult {
        ret: value,
        mutated: Vec::new(),
    })
}

/// The functions of other modules a node calls, through the bridge: what
/// `externalfunction` and `taskrun` nodes call. A call naming no module goes
/// to the module its function is routed to.
struct ModuleFunctions<'a> {
    bridge: &'a mut dyn CallBridge,
    function_modules: &'a HashMap<Uuid, Uuid>,
    default_module: Option<Uuid>,
}

impl ModuleFunctions<'_> {
    fn module(&self, module: Option<Uuid>, function: Uuid) -> Result<Uuid, String> {
        module
            .or_else(|| self.function_modules.get(&function).copied())
            .or(self.default_module)
            .ok_or_else(|| format!("no module registered for external function {function}"))
    }

    fn dispatch(
        &mut self,
        module_id: Uuid,
        function: Uuid,
        args: &[(Uuid, Value)],
    ) -> Result<CallResult, String> {
        self.bridge
            .arora_call(Call {
                module_id: Some(module_id),
                id: function,
                args: args
                    .iter()
                    .map(|(id, value)| StructureField {
                        id: *id,
                        value: Box::new(value.clone()),
                    })
                    .collect(),
            })
            .map_err(|e| format!("module call failed: {e}"))
    }
}

impl NodeFunctions for ModuleFunctions<'_> {
    fn call(&mut self, function: Uuid, args: &[(Uuid, Value)]) -> Result<Value, String> {
        let module = self.module(None, function)?;
        Ok(self.dispatch(module, function, args)?.ret)
    }

    fn call_module(
        &mut self,
        module: Option<Uuid>,
        function: Uuid,
        args: &[(Uuid, Value)],
    ) -> Result<Value, String> {
        let module = self.module(module, function)?;
        Ok(self.dispatch(module, function, args)?.ret)
    }

    fn call_module_with_outputs(
        &mut self,
        module: Option<Uuid>,
        function: Uuid,
        args: &[(Uuid, Value)],
    ) -> Result<(Value, Vec<(Uuid, Value)>), String> {
        let module = self.module(module, function)?;
        let result = self.dispatch(module, function, args)?;
        Ok((
            result.ret,
            result
                .mutated
                .into_iter()
                .map(|field| (field.id, *field.value))
                .collect(),
        ))
    }
}

/// A [`CallBridge`] answering [`MODULE_NAME`]'s calls in-process and passing
/// every other call, and the callables, to the bridge it wraps.
pub struct Bridge<'a> {
    module: &'a mut NodeGraphModule,
    inner: &'a mut dyn CallBridge,
}

impl CallBridge for Bridge<'_> {
    fn arora_call(&mut self, call: Call) -> Result<CallResult, CallError> {
        if call.module_id == Some(MODULE_ID) {
            self.module.call(call, &mut *self.inner)
        } else {
            self.inner.arora_call(call)
        }
    }

    /// Registers `callable` with the module, so it is called through a
    /// bridge over it, which answers the module's calls. A callable the
    /// wrapped bridge holds would be called through the wrapped bridge, which
    /// does not.
    fn arora_register_callable(&mut self, callable: std::rc::Rc<dyn Callable>) -> CallableId {
        let module = &mut *self.module;
        let id = CALLABLE_IDS + module.next_callable;
        module.next_callable += 1;
        module.callables.insert(id, callable);
        CallableId { id }
    }

    fn arora_unregister_callable(&mut self, callable_id: &CallableId) {
        if self.module.callables.remove(&callable_id.id).is_none() {
            self.inner.arora_unregister_callable(callable_id)
        }
    }

    fn arora_call_indirect(&mut self, callable_id: &CallableId) -> Result<Value, CallError> {
        match self.module.callables.get(&callable_id.id).cloned() {
            Some(callable) => callable.call(self),
            None => self.inner.arora_call_indirect(callable_id),
        }
    }
}
