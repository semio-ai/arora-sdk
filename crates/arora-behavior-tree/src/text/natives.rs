//! The native nodes as functions: the name the text language and the display
//! write for each, its parameters with their types, and whether it holds
//! children.
//!
//! A native node is dispatched by the tree itself, not by a module, so it is
//! in no device's function index; this table is where its name and signature
//! live. Its `module_id` is the nil id: it belongs to no module.

use std::collections::HashMap;
use std::sync::LazyLock;

use arora_behavior::{ModuleFunction, STATUS_ENUMERATION_ID, STATUS_ENUMERATION_VERSION};
use arora_types::call::Call;
use arora_types::record::module::frozen::{Function, Parameter};
use arora_types::record::ty::{FrozenOption, FrozenScalar, FrozenTy, PrimitiveKind};
use arora_types::record::{FrozenReference, Version};
use arora_types::AroraType;
use uuid::Uuid;

use crate::nodes::*;

/// A native node: its signature, whether it holds children, and the
/// parameters a node must bind.
pub(crate) struct Native {
    pub(crate) function: ModuleFunction,
    pub(crate) children: bool,
    pub(crate) required: Vec<Uuid>,
}

/// Every native, in the order a palette lists them: the control nodes, the
/// status leaves, the data nodes, the string nodes, the task-run nodes.
static NATIVES: LazyLock<Vec<Native>> = LazyLock::new(|| {
    use Kind::*;
    use Use::*;
    let mut natives = vec![
        declare("sequence", SEQ_FUNCTION_ID, true, &[]),
        declare(
            "sequence_star",
            SEQ_STAR_FUNCTION_ID,
            true,
            &[(
                "current_index",
                SEQ_STAR_CURRENT_INDEX_PARAM_ID,
                Primitive(PrimitiveKind::U16),
                Written,
            )],
        ),
        declare("fallback", FALLBACK_FUNCTION_ID, true, &[]),
        declare("parallel", PARALLEL_FUNCTION_ID, true, &[]),
        declare("succeed", SUCCEED_FUNCTION_ID, false, &[]),
        declare("fail", FAIL_FUNCTION_ID, false, &[]),
        declare("running", RUN_FUNCTION_ID, false, &[]),
        declare(
            "equal",
            EQUAL_FUNCTION_ID,
            false,
            &[
                ("a", EQUAL_A_PARAM_ID, Dynamic, Required),
                ("b", EQUAL_B_PARAM_ID, Dynamic, Required),
            ],
        ),
        declare(
            "unpack",
            UNPACK_FUNCTION_ID,
            false,
            &[("values", UNPACK_VALUES_PARAM_ID, Dynamic, Required)],
        ),
        declare(
            "scatter",
            SCATTER_FUNCTION_ID,
            false,
            &[
                ("values", SCATTER_VALUES_PARAM_ID, Dynamic, Required),
                (
                    "keys",
                    SCATTER_KEYS_PARAM_ID,
                    Primitive(PrimitiveKind::ArrayString),
                    Required,
                ),
                ("within", SCATTER_WITHIN_PARAM_ID, OptionalString, Read),
            ],
        ),
        declare(
            "set_string",
            SET_STR_FUNCTION_ID,
            false,
            &[
                (
                    "variable",
                    SET_STR_VARIABLE_PARAM_ID,
                    OptionalString,
                    RequiredWritten,
                ),
                ("value", SET_STR_VALUE_PARAM_ID, OptionalString, Required),
            ],
        ),
        declare(
            "unset_string",
            UNSET_STR_FUNCTION_ID,
            false,
            &[(
                "variable",
                UNSET_STR_VARIABLE_PARAM_ID,
                OptionalString,
                RequiredWritten,
            )],
        ),
        declare(
            "is_string_set",
            IS_STR_SET_FUNCTION_ID,
            false,
            &[("value", IS_STR_SET_VALUE_PARAM_ID, OptionalString, Required)],
        ),
        declare(
            "wait_string_set",
            WAIT_STR_SET_FUNCTION_ID,
            false,
            &[(
                "value",
                WAIT_STR_SET_VALUE_PARAM_ID,
                OptionalString,
                Required,
            )],
        ),
        declare(
            "regex_match",
            REGEX_MATCH_FUNCTION_ID,
            false,
            &[
                (
                    "value",
                    REGEX_MATCH_VALUE_PARAM_ID,
                    OptionalString,
                    Required,
                ),
                (
                    "matcher",
                    REGEX_MATCH_MATCHER_PARAM_ID,
                    OptionalString,
                    Required,
                ),
                (
                    "first_match",
                    REGEX_MATCH_FIRST_MATCH_PARAM_ID,
                    OptionalString,
                    RequiredWritten,
                ),
            ],
        ),
        declare(
            "run_call",
            RUN_CALL_FUNCTION_ID,
            false,
            &[("call", RUN_CALL_PARAM_ID, CallRecord, Required)],
        ),
        declare(
            "run_status",
            RUN_STATUS_FUNCTION_ID,
            true,
            &[
                ("status", RUN_STATUS_OUT_PARAM_ID, Status, RequiredWritten),
                ("latch", RUN_STATUS_LATCH_PARAM_ID, Dynamic, Written),
            ],
        ),
    ];
    // `unpack`'s output ports, `out_0` to `out_63`.
    let unpack = natives
        .iter_mut()
        .find(|native| native.function.function_id == UNPACK_FUNCTION_ID)
        .expect("unpack is native");
    for (index, id) in UNPACK_OUT_PARAM_IDS.iter().enumerate() {
        add_parameter(
            &mut unpack.function.function,
            *id,
            &format!("out_{index}"),
            Dynamic.ty(),
            true,
        );
    }
    natives
});

static BY_ID: LazyLock<HashMap<Uuid, &'static Native>> = LazyLock::new(|| {
    NATIVES
        .iter()
        .map(|native| (native.function.function_id, native))
        .collect()
});

static BY_NAME: LazyLock<HashMap<&'static str, &'static Native>> = LazyLock::new(|| {
    NATIVES
        .iter()
        .map(|native| (native.function.function_name.as_str(), native))
        .collect()
});

/// The native nodes, as the functions a palette lists: each one's name,
/// parameters (name, type, whether the node writes it) and return type,
/// under the nil module id.
pub fn natives() -> impl Iterator<Item = &'static ModuleFunction> {
    NATIVES.iter().map(|native| &native.function)
}

/// The native whose function id is `id`.
pub(crate) fn native(id: Uuid) -> Option<&'static Native> {
    BY_ID.get(&id).copied()
}

/// The native named `name`.
pub(crate) fn native_named(name: &str) -> Option<&'static Native> {
    BY_NAME.get(name).copied()
}

/// What a native's parameter holds.
#[derive(Clone, Copy)]
enum Kind {
    Primitive(PrimitiveKind),
    OptionalString,
    /// Any value.
    Dynamic,
    Status,
    CallRecord,
}

impl Kind {
    fn ty(self) -> FrozenTy {
        let reference = |id: Uuid, version: Version| {
            FrozenTy::FrozenScalar(FrozenScalar {
                reference: FrozenReference { id, version },
            })
        };
        match self {
            Kind::Primitive(kind) => FrozenTy::from(kind),
            Kind::OptionalString => FrozenTy::FrozenOption(FrozenOption {
                element: Box::new(FrozenTy::from(PrimitiveKind::String)),
            }),
            Kind::Dynamic => reference(
                *arora_types::ty::KEY_VALUE_ID,
                Version::from(semver::Version::new(1, 0, 0)),
            ),
            Kind::Status => reference(
                STATUS_ENUMERATION_ID,
                Version::from(STATUS_ENUMERATION_VERSION),
            ),
            Kind::CallRecord => reference(Call::arora_type_id(), Call::arora_type_version()),
        }
    }
}

/// How a node uses a native's parameter.
#[derive(Clone, Copy, PartialEq)]
enum Use {
    /// Read; a node binds it.
    Required,
    /// Read; a node may leave it out.
    Read,
    /// Written, and a node binds it.
    RequiredWritten,
    /// Written: the node's own state, seeded when a node leaves it out.
    Written,
}

fn declare(
    name: &str,
    function_id: Uuid,
    children: bool,
    parameters: &[(&str, Uuid, Kind, Use)],
) -> Native {
    let mut function = Function {
        parameters: HashMap::new(),
        parameter_ordering: Vec::new(),
        return_ty: Kind::Status.ty(),
    };
    for (name, id, kind, usage) in parameters {
        let written = matches!(usage, Use::RequiredWritten | Use::Written);
        add_parameter(&mut function, *id, name, kind.ty(), written);
    }
    Native {
        function: ModuleFunction {
            module_id: Uuid::nil(),
            function_id,
            function_name: name.to_string(),
            function,
        },
        children,
        required: parameters
            .iter()
            .filter(|(_, _, _, usage)| matches!(usage, Use::Required | Use::RequiredWritten))
            .map(|(_, id, _, _)| *id)
            .collect(),
    }
}

fn add_parameter(function: &mut Function, id: Uuid, name: &str, ty: FrozenTy, mutable: bool) {
    function.parameter_ordering.push(id);
    function.parameters.insert(
        id,
        Parameter {
            name: name.to_string(),
            ty,
            mutable,
        },
    );
}

/// The value a native's own state starts from when a node leaves it out:
/// `sequence_star`'s index and `run_status`'s latch.
pub(crate) fn seed(function: Uuid) -> Option<(Uuid, arora_types::value::Value)> {
    use arora_types::value::Value;
    match function {
        SEQ_STAR_FUNCTION_ID => Some((SEQ_STAR_CURRENT_INDEX_PARAM_ID, Value::U16(0))),
        RUN_STATUS_FUNCTION_ID => Some((RUN_STATUS_LATCH_PARAM_ID, Value::Unit)),
        _ => None,
    }
}
