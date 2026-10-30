//! The node kinds the module's functions compute: one function per kind.

use serde::{Deserialize, Serialize};

/// The kind of a node: the function it calls, named by its serialized name
/// (`add`, `damp`, `urdfikposition`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    // Scalars / arithmetic
    Constant,
    Slider,
    MultiSlider,
    Add,
    Subtract,
    Multiply,
    Divide,
    Power,
    Log,
    Abs,
    Modulo,
    Sqrt,
    Sign,
    Min,
    Max,
    Round,
    Sin,
    Cos,
    Tan,

    // Time & generators
    Time,
    /// Emits `sin(2π f t + phase)` using the node's configured parameters.
    Oscillator, // sin(2π f t + phase)

    // Transition & smoothing
    Spring,
    Damp,
    Slew,

    // Logic
    And,
    Or,
    Not,
    Xor,

    // Conditional
    GreaterThan,
    LessThan,
    Equal,
    NotEqual,
    If,
    /// Control-flow helper that routes one of several branches by label.
    Case,

    // Ranges
    Clamp,
    Remap,
    /// Remap around an anchor point with independent low/high ranges.
    #[serde(rename = "centered_remap")]
    CenteredRemap,
    /// Remap through multiple segments, optionally clamping at the ends.
    #[serde(rename = "piecewise_remap")]
    PiecewiseRemap,

    // 3D-specific utilities
    Vec3Cross,

    // Generic vector utilities
    VectorConstant,
    VectorAdd,
    VectorSubtract,
    VectorMultiply, // component-wise
    VectorScale,    // scalar * vector
    VectorNormalize,
    VectorDot,
    VectorLength,
    VectorIndex,
    Join,
    Split,
    VectorMin,
    VectorMax,
    VectorMean,
    VectorMedian,
    VectorMode,
    ToVector,
    FromVector,

    // Noise generators
    SimpleNoise,
    PerlinNoise,
    SimplexNoise,

    // Blend helpers
    WeightedSumVector,
    /// Host-default blending strategy for structured values.
    #[serde(rename = "default-blend")]
    DefaultBlend,
    BlendWeightedAverage,
    BlendAdditive,
    BlendMultiply,
    BlendWeightedOverlay,
    BlendWeightedAverageOverlay,
    BlendMax,

    // Robotics
    InverseKinematics,
    /// URDF-backed IK solver targeting translation only.
    UrdfIkPosition,
    /// URDF-backed IK solver targeting translation and rotation.
    UrdfIkPose,
    /// URDF-backed forward-kinematics solver.
    UrdfFk,

    // Records
    BuildRecord,
    ReadRecord,
    SwitchRecord,
    MergeRecord,
    SplitRecord,
    MathMultRecord,
    MathAddRecord,
    MathDivRecord,
    MathSubRecord,

    // External function invocation
    /// Invokes an external function (resolved by opaque id) through the host-provided
    /// [`NodeFunctions`](crate::eval::NodeFunctions) interface.
    ExternalFunction,
    /// Hosts one task run: invokes a module function each evaluation and emits
    /// the run's behavior [`Status`](crate::task) value on `out`, latched once
    /// terminal.
    ///
    /// Unlike [`ExternalFunction`](Self::ExternalFunction), the call is fully
    /// self-contained in the params — [`module`](crate::params::Params::module),
    /// [`function`](crate::params::Params::function), and the argument bundle in
    /// [`value`](crate::params::Params::value) — so a run grafts into a live graph without
    /// argument-feeder wiring.
    TaskRun,
    /// Spawns a task run of [`function`](crate::params::Params::function) each time its
    /// arguments change: the keyed variadic `args` inputs (slot `i` carries
    /// the parameter whose id is `record_keys[i]`) over the
    /// [`value`](crate::params::Params::value) bundle's fields. The first evaluation
    /// sets the baseline and spawns nothing, so a graph spawns on a change it
    /// observes — unless [`spawn_on_load`](crate::params::Params::spawn_on_load)
    /// is set, and the first evaluation spawns with the baseline too.
    ///
    /// Where a [`TaskRun`](Self::TaskRun) node *is* one run's call, this node
    /// starts runs: the request is queued on the evaluation's spawns
    /// ([`Context::spawns`](crate::eval::Context)) and served by the host once
    /// the evaluation is done. The graph keeps no handle — a run it
    /// spawns is the host's to end and clean up.
    Spawn,
}
