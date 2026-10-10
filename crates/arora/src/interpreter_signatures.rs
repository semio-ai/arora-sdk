//! The signatures under which the method index describes the interpreter
//! module's task-run functions ([`arora_behavior::interpreter_module`]) —
//! `spawn`, `spawn_graph` and `halt` — so that `DescribeMethods` lists them by
//! name like any module's.
//!
//! `load` and `edit` keep their ids without a description: a client reaches the
//! main behavior through the interpreter module's ids, and the names stay free
//! for the typed `load_graph` and `edit_graph` of the composed behavior host.
//!
//! A graph, a call and a task handle travel as dynamic key-values, keyed by
//! their field names, so they are described as the dynamic key-value type. A
//! run policy travels as the name of its variant, a string.

use std::collections::HashMap;

use arora_behavior::interpreter_module::{
    HALT, HALT_ARG, SPAWN, SPAWN_CALL_ARG, SPAWN_GRAPH, SPAWN_GRAPH_GRAPH_ARG,
    SPAWN_GRAPH_POLICY_ARG, SPAWN_GRAPH_TYPE_ARG, SPAWN_GRAPH_VERSION_ARG, SPAWN_POLICY_ARG,
};
use arora_behavior::TaskId;
use arora_types::record::module::frozen::{Function, Parameter};
use arora_types::record::ty::{FrozenScalar, FrozenTy, PrimitiveKind};
use arora_types::record::{FrozenReference, Version};
use arora_types::{AroraType, SemanticVersion, Uuid};

/// The interpreter module's described functions: each one's id, name and
/// signature.
pub(crate) fn all() -> Vec<(Uuid, &'static str, Function)> {
    let string = || FrozenTy::from(PrimitiveKind::String);
    vec![
        (
            SPAWN,
            "spawn",
            signature(
                &[
                    (SPAWN_CALL_ARG, "call", dynamic()),
                    (SPAWN_POLICY_ARG, "policy", string()),
                ],
                dynamic(),
            ),
        ),
        (
            SPAWN_GRAPH,
            "spawn_graph",
            signature(
                &[
                    (SPAWN_GRAPH_TYPE_ARG, "graph_type", string()),
                    (SPAWN_GRAPH_VERSION_ARG, "graph_version", string()),
                    (SPAWN_GRAPH_GRAPH_ARG, "graph", dynamic()),
                    (SPAWN_GRAPH_POLICY_ARG, "policy", string()),
                ],
                dynamic(),
            ),
        ),
        (
            HALT,
            "halt",
            signature(
                &[(HALT_ARG, "task", task_id())],
                FrozenTy::from(PrimitiveKind::Unit),
            ),
        ),
    ]
}

fn signature(parameters: &[(Uuid, &str, FrozenTy)], return_ty: FrozenTy) -> Function {
    Function {
        parameters: parameters
            .iter()
            .map(|(id, name, ty)| {
                (
                    *id,
                    Parameter {
                        name: name.to_string(),
                        ty: ty.clone(),
                        mutable: false,
                    },
                )
            })
            .collect::<HashMap<_, _>>(),
        parameter_ordering: parameters.iter().map(|(id, _, _)| *id).collect(),
        return_ty,
    }
}

/// A task id: the uuid type.
fn task_id() -> FrozenTy {
    FrozenTy::FrozenScalar(FrozenScalar {
        reference: FrozenReference {
            id: TaskId::arora_type_id(),
            version: TaskId::arora_type_version(),
        },
    })
}

/// The dynamic key-value type.
fn dynamic() -> FrozenTy {
    FrozenTy::FrozenScalar(FrozenScalar {
        reference: FrozenReference {
            id: *arora_types::ty::KEY_VALUE_ID,
            version: Version::from(SemanticVersion {
                major: 1,
                minor: 0,
                patch: 0,
            }),
        },
    })
}
