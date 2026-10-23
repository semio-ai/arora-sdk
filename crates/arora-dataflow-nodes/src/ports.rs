//! The ports each node kind's function declares: its fixed inputs and
//! outputs, in order, and its variadic groups. A node's port layouts start
//! from these, and Vizij's editor registry describes the same ports.

use crate::kind::Kind;

/// A variadic group of ports, `{id}_{N}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Variadic {
    /// The group's name, the prefix of its ports.
    pub id: &'static str,
    /// Whether the node's parameters name its ports' keys (a record's
    /// fields, a task's out-parameters), which numbers them from 0.
    pub keyed: bool,
}

/// A node kind's ports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ports {
    /// The fixed input ports, in order.
    pub inputs: &'static [&'static str],
    /// The variadic input group, which follows the fixed inputs.
    pub variadic_inputs: Option<Variadic>,
    /// The fixed output ports, in order.
    pub outputs: &'static [&'static str],
    /// The variadic output group, which follows the fixed outputs.
    pub variadic_outputs: Option<Variadic>,
}

/// The ports of a node of `kind`.
pub fn ports(kind: &Kind) -> Ports {
    match kind {
        Kind::Constant => Ports {
            inputs: &[],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Slider => Ports {
            inputs: &[],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::MultiSlider => Ports {
            inputs: &[],
            variadic_inputs: None,
            outputs: &["x", "y", "z"],
            variadic_outputs: None,
        },
        Kind::Add => Ports {
            inputs: &[],
            variadic_inputs: Some(Variadic {
                id: "operand",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Subtract => Ports {
            inputs: &["lhs", "rhs"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Multiply => Ports {
            inputs: &[],
            variadic_inputs: Some(Variadic {
                id: "operand",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Divide => Ports {
            inputs: &["lhs", "rhs"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Modulo => Ports {
            inputs: &["lhs", "rhs"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Power => Ports {
            inputs: &["base", "exp"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Log => Ports {
            inputs: &["value", "base"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Abs => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Sqrt => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Sign => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Min => Ports {
            inputs: &[],
            variadic_inputs: Some(Variadic {
                id: "operand",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Max => Ports {
            inputs: &[],
            variadic_inputs: Some(Variadic {
                id: "operand",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Round => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Sin => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Cos => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Tan => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Time => Ports {
            inputs: &[],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Oscillator => Ports {
            inputs: &["frequency", "phase"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Spring => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Damp => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Slew => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::And => Ports {
            inputs: &["lhs", "rhs"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Or => Ports {
            inputs: &["lhs", "rhs"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Xor => Ports {
            inputs: &["lhs", "rhs"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Not => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::GreaterThan => Ports {
            inputs: &["lhs", "rhs"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::LessThan => Ports {
            inputs: &["lhs", "rhs"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Equal => Ports {
            inputs: &["lhs", "rhs"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::NotEqual => Ports {
            inputs: &["lhs", "rhs"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::If => Ports {
            inputs: &["cond", "then", "else"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Clamp => Ports {
            inputs: &["in", "min", "max"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Remap => Ports {
            inputs: &["in", "in_min", "in_max", "out_min", "out_max"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::CenteredRemap => Ports {
            inputs: &[
                "in",
                "in_low",
                "in_anchor",
                "in_high",
                "out_low",
                "out_anchor",
                "out_high",
            ],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::PiecewiseRemap => Ports {
            inputs: &["in", "input_breakpoints", "output_breakpoints"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Vec3Cross => Ports {
            inputs: &["a", "b"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::VectorConstant => Ports {
            inputs: &[],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::VectorAdd => Ports {
            inputs: &["a", "b"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::VectorSubtract => Ports {
            inputs: &["a", "b"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::VectorMultiply => Ports {
            inputs: &["a", "b"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::VectorScale => Ports {
            inputs: &["scalar", "v"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::VectorNormalize => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::VectorDot => Ports {
            inputs: &["a", "b"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::VectorLength => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::VectorIndex => Ports {
            inputs: &["v", "index"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Join => Ports {
            inputs: &[],
            variadic_inputs: Some(Variadic {
                id: "operand",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Split => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &[],
            variadic_outputs: Some(Variadic {
                id: "parts",
                keyed: false,
            }),
        },
        Kind::ToVector => Ports {
            inputs: &[],
            variadic_inputs: Some(Variadic {
                id: "operand",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::FromVector => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &[],
            variadic_outputs: Some(Variadic {
                id: "elements",
                keyed: false,
            }),
        },
        Kind::VectorMin => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::VectorMax => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::VectorMean => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::VectorMedian => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::VectorMode => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::SimpleNoise => Ports {
            inputs: &["x", "y"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::PerlinNoise => Ports {
            inputs: &["x", "y"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::SimplexNoise => Ports {
            inputs: &["x", "y"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::WeightedSumVector => Ports {
            inputs: &["values", "weights", "masks"],
            variadic_inputs: None,
            outputs: &[
                "total_weighted_sum",
                "total_weight",
                "max_effective_weight",
                "input_count",
            ],
            variadic_outputs: None,
        },
        Kind::DefaultBlend => Ports {
            inputs: &["baseline", "offset", "weights"],
            variadic_inputs: Some(Variadic {
                id: "operand",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::BlendWeightedAverage => Ports {
            inputs: &[
                "total_weighted_sum",
                "total_weight",
                "max_effective_weight",
                "fallback",
            ],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::BlendAdditive => Ports {
            inputs: &["total_weighted_sum", "total_weight", "fallback"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::BlendMultiply => Ports {
            inputs: &["values", "weights", "masks"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::BlendWeightedOverlay => Ports {
            inputs: &["total_weighted_sum", "max_effective_weight", "base"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::BlendWeightedAverageOverlay => Ports {
            inputs: &[
                "total_weighted_sum",
                "total_weight",
                "max_effective_weight",
                "base",
            ],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::BlendMax => Ports {
            inputs: &["values", "weights", "masks", "base"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::Case => Ports {
            inputs: &["selector", "default"],
            variadic_inputs: Some(Variadic {
                id: "operand",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::InverseKinematics => Ports {
            inputs: &["bone1", "bone2", "bone3", "theta", "x", "y"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::UrdfIkPosition => Ports {
            inputs: &["target_pos", "seed"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::UrdfIkPose => Ports {
            inputs: &["target_pos", "target_rot", "seed"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::UrdfFk => Ports {
            inputs: &["joints"],
            variadic_inputs: None,
            outputs: &["position", "rotation", "transform"],
            variadic_outputs: None,
        },
        Kind::BuildRecord => Ports {
            inputs: &[],
            variadic_inputs: Some(Variadic {
                id: "field",
                keyed: true,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::ReadRecord => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &[],
            variadic_outputs: Some(Variadic {
                id: "field",
                keyed: true,
            }),
        },
        Kind::SwitchRecord => Ports {
            inputs: &["switch"],
            variadic_inputs: Some(Variadic {
                id: "record",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::MergeRecord => Ports {
            inputs: &[],
            variadic_inputs: Some(Variadic {
                id: "record",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::SplitRecord => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["included", "excluded"],
            variadic_outputs: None,
        },
        Kind::MathMultRecord => Ports {
            inputs: &["in", "value"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::MathAddRecord => Ports {
            inputs: &["in", "value"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::MathDivRecord => Ports {
            inputs: &["in", "value"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::MathSubRecord => Ports {
            inputs: &["in", "value"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::ExternalFunction => Ports {
            inputs: &[],
            variadic_inputs: Some(Variadic {
                id: "args",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        Kind::TaskRun => Ports {
            inputs: &["args"],
            variadic_inputs: None,
            outputs: &["out", "done"],
            variadic_outputs: Some(Variadic {
                id: "mutated",
                keyed: true,
            }),
        },
        Kind::Spawn => Ports {
            inputs: &["when"],
            variadic_inputs: Some(Variadic {
                id: "args",
                keyed: true,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
    }
}
