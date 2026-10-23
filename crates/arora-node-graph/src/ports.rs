//! The ports each node kind's function declares: its fixed inputs and
//! outputs, in order, and its variadic groups. A node's port layouts start
//! from these, and Vizij's editor registry ([`crate::schema::registry`])
//! describes the same ports.

use crate::types::NodeType;

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
pub fn ports(kind: &NodeType) -> Ports {
    match kind {
        NodeType::Constant => Ports {
            inputs: &[],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Slider => Ports {
            inputs: &[],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::MultiSlider => Ports {
            inputs: &[],
            variadic_inputs: None,
            outputs: &["x", "y", "z"],
            variadic_outputs: None,
        },
        NodeType::Add => Ports {
            inputs: &[],
            variadic_inputs: Some(Variadic {
                id: "operand",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Subtract => Ports {
            inputs: &["lhs", "rhs"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Multiply => Ports {
            inputs: &[],
            variadic_inputs: Some(Variadic {
                id: "operand",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Divide => Ports {
            inputs: &["lhs", "rhs"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Modulo => Ports {
            inputs: &["lhs", "rhs"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Power => Ports {
            inputs: &["base", "exp"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Log => Ports {
            inputs: &["value", "base"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Abs => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Sqrt => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Sign => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Min => Ports {
            inputs: &[],
            variadic_inputs: Some(Variadic {
                id: "operand",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Max => Ports {
            inputs: &[],
            variadic_inputs: Some(Variadic {
                id: "operand",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Round => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Sin => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Cos => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Tan => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Time => Ports {
            inputs: &[],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Oscillator => Ports {
            inputs: &["frequency", "phase"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Spring => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Damp => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Slew => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::And => Ports {
            inputs: &["lhs", "rhs"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Or => Ports {
            inputs: &["lhs", "rhs"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Xor => Ports {
            inputs: &["lhs", "rhs"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Not => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::GreaterThan => Ports {
            inputs: &["lhs", "rhs"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::LessThan => Ports {
            inputs: &["lhs", "rhs"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Equal => Ports {
            inputs: &["lhs", "rhs"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::NotEqual => Ports {
            inputs: &["lhs", "rhs"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::If => Ports {
            inputs: &["cond", "then", "else"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Clamp => Ports {
            inputs: &["in", "min", "max"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Remap => Ports {
            inputs: &["in", "in_min", "in_max", "out_min", "out_max"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::CenteredRemap => Ports {
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
        NodeType::PiecewiseRemap => Ports {
            inputs: &["in", "input_breakpoints", "output_breakpoints"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Vec3Cross => Ports {
            inputs: &["a", "b"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::VectorConstant => Ports {
            inputs: &[],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::VectorAdd => Ports {
            inputs: &["a", "b"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::VectorSubtract => Ports {
            inputs: &["a", "b"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::VectorMultiply => Ports {
            inputs: &["a", "b"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::VectorScale => Ports {
            inputs: &["scalar", "v"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::VectorNormalize => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::VectorDot => Ports {
            inputs: &["a", "b"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::VectorLength => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::VectorIndex => Ports {
            inputs: &["v", "index"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Join => Ports {
            inputs: &[],
            variadic_inputs: Some(Variadic {
                id: "operand",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Split => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &[],
            variadic_outputs: Some(Variadic {
                id: "parts",
                keyed: false,
            }),
        },
        NodeType::ToVector => Ports {
            inputs: &[],
            variadic_inputs: Some(Variadic {
                id: "operand",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::FromVector => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &[],
            variadic_outputs: Some(Variadic {
                id: "elements",
                keyed: false,
            }),
        },
        NodeType::VectorMin => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::VectorMax => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::VectorMean => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::VectorMedian => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::VectorMode => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::SimpleNoise => Ports {
            inputs: &["x", "y"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::PerlinNoise => Ports {
            inputs: &["x", "y"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::SimplexNoise => Ports {
            inputs: &["x", "y"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::WeightedSumVector => Ports {
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
        NodeType::DefaultBlend => Ports {
            inputs: &["baseline", "offset", "weights"],
            variadic_inputs: Some(Variadic {
                id: "operand",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::BlendWeightedAverage => Ports {
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
        NodeType::BlendAdditive => Ports {
            inputs: &["total_weighted_sum", "total_weight", "fallback"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::BlendMultiply => Ports {
            inputs: &["values", "weights", "masks"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::BlendWeightedOverlay => Ports {
            inputs: &["total_weighted_sum", "max_effective_weight", "base"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::BlendWeightedAverageOverlay => Ports {
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
        NodeType::BlendMax => Ports {
            inputs: &["values", "weights", "masks", "base"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Case => Ports {
            inputs: &["selector", "default"],
            variadic_inputs: Some(Variadic {
                id: "operand",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::InverseKinematics => Ports {
            inputs: &["bone1", "bone2", "bone3", "theta", "x", "y"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::UrdfIkPosition => Ports {
            inputs: &["target_pos", "seed"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::UrdfIkPose => Ports {
            inputs: &["target_pos", "target_rot", "seed"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::UrdfFk => Ports {
            inputs: &["joints"],
            variadic_inputs: None,
            outputs: &["position", "rotation", "transform"],
            variadic_outputs: None,
        },
        NodeType::Input => Ports {
            inputs: &[],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::Output => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::BuildRecord => Ports {
            inputs: &[],
            variadic_inputs: Some(Variadic {
                id: "field",
                keyed: true,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::ReadRecord => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &[],
            variadic_outputs: Some(Variadic {
                id: "field",
                keyed: true,
            }),
        },
        NodeType::SwitchRecord => Ports {
            inputs: &["switch"],
            variadic_inputs: Some(Variadic {
                id: "record",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::MergeRecord => Ports {
            inputs: &[],
            variadic_inputs: Some(Variadic {
                id: "record",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::SplitRecord => Ports {
            inputs: &["in"],
            variadic_inputs: None,
            outputs: &["included", "excluded"],
            variadic_outputs: None,
        },
        NodeType::MathMultRecord => Ports {
            inputs: &["in", "value"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::MathAddRecord => Ports {
            inputs: &["in", "value"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::MathDivRecord => Ports {
            inputs: &["in", "value"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::MathSubRecord => Ports {
            inputs: &["in", "value"],
            variadic_inputs: None,
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::ExternalFunction => Ports {
            inputs: &[],
            variadic_inputs: Some(Variadic {
                id: "args",
                keyed: false,
            }),
            outputs: &["out"],
            variadic_outputs: None,
        },
        NodeType::TaskRun => Ports {
            inputs: &["args"],
            variadic_inputs: None,
            outputs: &["out", "done"],
            variadic_outputs: Some(Variadic {
                id: "mutated",
                keyed: true,
            }),
        },
        NodeType::Spawn => Ports {
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
