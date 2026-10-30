//! A node's parameters, as the node functions read them.
//!
//! Each node kind reads the fields it needs and ignores the rest. A node's
//! call passes the fields that differ from their default as its literal
//! arguments, `param.<field>` ([`crate::functions::param_field`]).

use arora_types::value::Value;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A node's parameters.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Params {
    /// Literal value for constant-like nodes and inline defaults.
    pub value: Option<Value>,
    /// Frequency parameter used by oscillators and noise-style generators.
    pub frequency: Option<f32>,
    /// Phase offset used by oscillators.
    pub phase: Option<f32>,
    /// Lower bound used by range-based nodes such as sliders, clamp, and remap variants.
    pub min: f32,
    /// Upper bound used by range-based nodes such as sliders, clamp, and remap variants.
    pub max: f32,
    /// Optional `x` component seed for vector-construction nodes.
    pub x: Option<f32>,
    /// Optional `y` component seed for vector-construction nodes.
    pub y: Option<f32>,
    /// Optional `z` component seed for vector-construction nodes.
    pub z: Option<f32>,
    /// Source low bound for remap-style nodes.
    pub in_min: Option<f32>,
    /// Source high bound for remap-style nodes.
    pub in_max: Option<f32>,
    /// Destination low bound for remap-style nodes.
    pub out_min: Option<f32>,
    /// Destination high bound for remap-style nodes.
    pub out_max: Option<f32>,
    /// Rounding mode used by [`Kind::Round`](crate::kind::Kind::Round).
    pub round_mode: Option<RoundMode>,
    /// Clamp behavior for [`Kind::PiecewiseRemap`](crate::kind::Kind::PiecewiseRemap).
    pub clamp: Option<bool>,
    /// Input low anchor for [`Kind::CenteredRemap`](crate::kind::Kind::CenteredRemap).
    pub in_low: Option<f32>,
    /// Input center anchor for [`Kind::CenteredRemap`](crate::kind::Kind::CenteredRemap).
    pub in_anchor: Option<f32>,
    /// Input high anchor for [`Kind::CenteredRemap`](crate::kind::Kind::CenteredRemap).
    pub in_high: Option<f32>,
    /// Output low anchor for [`Kind::CenteredRemap`](crate::kind::Kind::CenteredRemap).
    pub out_low: Option<f32>,
    /// Output center anchor for [`Kind::CenteredRemap`](crate::kind::Kind::CenteredRemap).
    pub out_anchor: Option<f32>,
    /// Output high anchor for [`Kind::CenteredRemap`](crate::kind::Kind::CenteredRemap).
    pub out_high: Option<f32>,
    /// Bone length for lightweight analytic IK nodes.
    pub bone1: Option<f32>,
    /// Bone length for lightweight analytic IK nodes.
    pub bone2: Option<f32>,
    /// Bone length for lightweight analytic IK nodes.
    pub bone3: Option<f32>,
    /// Raw URDF XML fed to URDF-backed kinematics nodes.
    pub urdf_xml: Option<String>,
    /// Root link name used when building a URDF chain.
    pub root_link: Option<String>,
    /// Tip/end-effector link name used when building a URDF chain.
    pub tip_link: Option<String>,
    /// Optional seed vector passed into IK solvers.
    pub seed: Option<Vec<f32>>,
    /// Optional per-joint weights passed into IK solvers.
    pub weights: Option<Vec<f32>>,
    /// Maximum solver iterations for IK nodes.
    pub max_iters: Option<u32>,
    /// Positional tolerance for IK convergence.
    pub tol_pos: Option<f32>,
    /// Rotational tolerance for IK convergence.
    pub tol_rot: Option<f32>,
    /// Default joint values keyed by joint name for URDF-backed nodes.
    pub joint_defaults: Option<Vec<(String, f32)>>,
    /// Zero-based component index used by indexing/splitting nodes.
    pub index: Option<f32>,
    /// Segment sizes for [`Kind::Split`](crate::kind::Kind::Split), floored to whole-number widths by the evaluator.
    pub sizes: Option<Vec<f32>>,
    /// Noise seed value passed into procedural noise nodes.
    pub noise_seed: Option<f32>,
    /// Number of noise octaves to accumulate.
    pub octaves: Option<f32>,
    /// Frequency multiplier between octaves for noise nodes.
    pub lacunarity: Option<f32>,
    /// Amplitude falloff between octaves for noise nodes.
    pub persistence: Option<f32>,
    /// Spring stiffness for smoothing nodes.
    pub stiffness: Option<f32>,
    /// Damping coefficient for smoothing nodes.
    pub damping: Option<f32>,
    /// Effective mass for spring-style nodes.
    pub mass: Option<f32>,
    /// Half-life parameter for damp nodes.
    pub half_life: Option<f32>,
    /// Maximum change rate for slew nodes.
    pub max_rate: Option<f32>,
    /// Branch labels for [`Kind::Case`](crate::kind::Kind::Case), matched in declaration order.
    pub case_labels: Option<Vec<String>>,
    /// For BuildRecord/ReadRecord/TaskRun – one key string per variadic slot, in
    /// slot order (a TaskRun key is the id of the out-parameter the slot carries)
    pub record_keys: Option<Vec<String>>,
    /// For SplitRecord – comma-separated field keys to include in `included` output
    pub keys: Option<String>,
    /// Stable id of the node-function invoked by [`Kind::ExternalFunction`](crate::kind::Kind::ExternalFunction).
    ///
    /// The graph attaches no meaning to this id beyond passing it to the host
    /// [`NodeFunctions`](crate::eval::NodeFunctions); the host resolves and invokes it.
    pub function: Option<Uuid>,
    /// Ordered arg-key handles matching the [`Kind::ExternalFunction`](crate::kind::Kind::ExternalFunction) variadic `args` inputs.
    ///
    /// These are opaque to the graph; the host pairs each with the positional value in the call.
    pub param_ids: Option<Vec<Uuid>>,
    /// Module the [`Kind::TaskRun`](crate::kind::Kind::TaskRun) function is dispatched to, when the run
    /// names one. `None` leaves resolution to the host's function→module map,
    /// like an [`Kind::ExternalFunction`](crate::kind::Kind::ExternalFunction) call.
    pub module: Option<Uuid>,
    /// Whether a [`Kind::Spawn`](crate::kind::Kind::Spawn) node spawns on
    /// its first evaluation too, with the arguments it records as loaded:
    /// for a graph whose inputs command a state that holds, which is then set
    /// from them at once as well as on every change. `None` or `false`
    /// spawns on changes only.
    pub spawn_on_load: Option<bool>,
}

/// Rounding strategy for [`Kind::Round`](crate::kind::Kind::Round).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum RoundMode {
    /// Round toward negative infinity.
    #[default]
    Floor,
    /// Round toward positive infinity.
    Ceil,
    /// Round toward zero.
    Trunc,
}
