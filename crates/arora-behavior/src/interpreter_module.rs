//! The behavior interpreter as a module: the well-known ids and call
//! conventions under which the runtime exposes its one
//! [`BehaviorInterpreter`](crate::BehaviorInterpreter) on the engine.
//!
//! The runtime assembles a host module under [`ID`] (the engine's generic
//! module builder) with functions attached to the interpreter's entry points:
//!
//! - [`LOAD`] → [`BehaviorInterpreter::load`](crate::BehaviorInterpreter::load):
//!   replace the running behavior with a whole [`Graph`];
//! - [`EDIT`] → [`BehaviorInterpreter::apply`](crate::BehaviorInterpreter::apply):
//!   apply a [`GraphDiff`] to it;
//! - [`SPAWN`] → [`BehaviorInterpreter::spawn`](crate::BehaviorInterpreter::spawn):
//!   start a task run from a [`Call`] under a [`RunPolicy`], returning a
//!   [`TaskHandle`];
//! - [`SPAWN_GRAPH`] →
//!   [`BehaviorInterpreter::spawn_graph`](crate::BehaviorInterpreter::spawn_graph):
//!   start a task run whose program is a [`Graph`] of a [`GraphType`], under
//!   a [`RunPolicy`], returning a [`TaskHandle`];
//! - [`HALT`] → [`BehaviorInterpreter::halt`](crate::BehaviorInterpreter::halt):
//!   stop the run named by a [`TaskId`].
//!
//! A primitive argument travels as its own [`Value`] — a task id as a uuid, a
//! graph type and version as strings. A compound payload (a graph, a diff, a
//! call, a policy) is converted generically through
//! [`arora_types::value_serde`], and the [`TaskHandle`] that [`SPAWN`] and
//! [`SPAWN_GRAPH`] answer with returns the same way. A
//! `Call{module_id: ID, id: LOAD|EDIT|SPAWN|SPAWN_GRAPH|HALT}` — a remote's
//! `BridgeOp::Call`, or a behavior's own call bridge — reaches the interpreter
//! through the engine's normal dispatch, like any module function.

use arora_types::call::Call;
use arora_types::value::{StructureField, Value};
use arora_types::value_serde;
use serde::de::DeserializeOwned;
use serde::Serialize;
use uuid::{uuid, Uuid};

use crate::graph::{Graph, GraphDiff, GraphType};
use crate::task::{RunPolicy, TaskHandle, TaskId};

/// Module id under which the runtime registers the behavior interpreter on
/// the engine. Self-identifying like the vizij type ids: the ASCII bytes of
/// "arora" lead the UUID, a small offset tails it.
pub const ID: Uuid = uuid!("61726f72-6100-0000-0000-000000000001");

/// Function id of **edit**: apply a [`GraphDiff`] to the running behavior.
pub const EDIT: Uuid = uuid!("61726f72-6100-0000-0000-000000000002");

/// Argument id of [`EDIT`]'s one argument: the [`GraphDiff`], as a structured
/// [`Value`](arora_types::value::Value).
pub const EDIT_ARG: Uuid = uuid!("61726f72-6100-0000-0000-000000000003");

/// Function id of **load**: replace the running behavior with a [`Graph`].
pub const LOAD: Uuid = uuid!("61726f72-6100-0000-0000-000000000004");

/// Argument id of [`LOAD`]'s one argument: the [`Graph`], as a structured
/// [`Value`](arora_types::value::Value).
pub const LOAD_ARG: Uuid = uuid!("61726f72-6100-0000-0000-000000000005");

/// Function id of **spawn**: start a task run from a [`Call`] under a
/// [`RunPolicy`], returning a [`TaskHandle`].
pub const SPAWN: Uuid = uuid!("61726f72-6100-0000-0000-000000000006");

/// Argument id of [`SPAWN`]'s first argument: the [`Call`] to run, as a
/// structured [`Value`].
pub const SPAWN_CALL_ARG: Uuid = uuid!("61726f72-6100-0000-0000-000000000007");

/// Argument id of [`SPAWN`]'s second argument: the [`RunPolicy`].
pub const SPAWN_POLICY_ARG: Uuid = uuid!("61726f72-6100-0000-0000-000000000008");

/// Function id of **halt**: stop the run named by a [`TaskId`].
pub const HALT: Uuid = uuid!("61726f72-6100-0000-0000-000000000009");

/// Argument id of [`HALT`]'s one argument: the [`TaskId`] of the run to stop.
pub const HALT_ARG: Uuid = uuid!("61726f72-6100-0000-0000-00000000000a");

/// Function id of **spawn_graph**: start a task run whose program is a
/// [`Graph`] of a [`GraphType`], under a [`RunPolicy`], returning a
/// [`TaskHandle`].
pub const SPAWN_GRAPH: Uuid = uuid!("61726f72-6100-0000-0000-00000000000b");

/// Argument id of [`SPAWN_GRAPH`]'s `graph_type`: the name of the graph's
/// language ([`GraphType::name`]), a string.
pub const SPAWN_GRAPH_TYPE_ARG: Uuid = uuid!("61726f72-6100-0000-0000-00000000000c");

/// Argument id of [`SPAWN_GRAPH`]'s `graph_version`: the version of the
/// language's format the graph is written for ([`GraphType::version`]), a
/// string `MAJOR.MINOR` or `MAJOR.MINOR.PATCH`.
pub const SPAWN_GRAPH_VERSION_ARG: Uuid = uuid!("61726f72-6100-0000-0000-00000000000d");

/// Argument id of [`SPAWN_GRAPH`]'s `graph`: the [`Graph`] to run, as a
/// structured [`Value`].
pub const SPAWN_GRAPH_GRAPH_ARG: Uuid = uuid!("61726f72-6100-0000-0000-00000000000e");

/// Argument id of [`SPAWN_GRAPH`]'s `policy`: the [`RunPolicy`].
pub const SPAWN_GRAPH_POLICY_ARG: Uuid = uuid!("61726f72-6100-0000-0000-00000000000f");

/// Build the [`Call`] that applies `diff` to the running behavior.
pub fn encode_edit(diff: &GraphDiff) -> Call {
    encode(EDIT, EDIT_ARG, diff)
}

/// Read the [`GraphDiff`] out of an [`EDIT`] call.
pub fn decode_edit(call: &Call) -> Result<GraphDiff, String> {
    decode(call, EDIT, EDIT_ARG, "GraphDiff")
}

/// Build the [`Call`] that loads `graph` as the running behavior.
pub fn encode_load(graph: &Graph) -> Call {
    encode(LOAD, LOAD_ARG, graph)
}

/// Read the [`Graph`] out of a [`LOAD`] call.
pub fn decode_load(call: &Call) -> Result<Graph, String> {
    decode(call, LOAD, LOAD_ARG, "Graph")
}

/// Build the [`Call`] that spawns `call` as a task run under `policy`.
pub fn encode_spawn(call: &Call, policy: RunPolicy) -> Call {
    Call {
        module_id: Some(ID),
        id: SPAWN,
        args: vec![
            StructureField {
                id: SPAWN_CALL_ARG,
                value: Box::new(value_serde::to_value(call).expect("a Call converts to a Value")),
            },
            StructureField {
                id: SPAWN_POLICY_ARG,
                value: Box::new(
                    value_serde::to_value(&policy).expect("a RunPolicy converts to a Value"),
                ),
            },
        ],
    }
}

/// Read the [`Call`] to run and its [`RunPolicy`] out of a [`SPAWN`] call.
pub fn decode_spawn(call: &Call) -> Result<(Call, RunPolicy), String> {
    if call.id != SPAWN {
        return Err(format!(
            "not the expected interpreter function: {}",
            call.id
        ));
    }
    let inner = decode_arg(call, SPAWN_CALL_ARG, "Call")?;
    let policy = decode_arg(call, SPAWN_POLICY_ARG, "RunPolicy")?;
    Ok((inner, policy))
}

/// Build the [`Call`] that spawns `graph`, of type `graph_type`, as a task run
/// under `policy`.
pub fn encode_spawn_graph(graph_type: &GraphType, graph: &Graph, policy: RunPolicy) -> Call {
    let field = |id, value| StructureField {
        id,
        value: Box::new(value),
    };
    Call {
        module_id: Some(ID),
        id: SPAWN_GRAPH,
        args: vec![
            field(SPAWN_GRAPH_TYPE_ARG, Value::String(graph_type.name.clone())),
            field(
                SPAWN_GRAPH_VERSION_ARG,
                Value::String(graph_type.version.to_string()),
            ),
            field(
                SPAWN_GRAPH_GRAPH_ARG,
                value_serde::to_value(graph).expect("a Graph converts to a Value"),
            ),
            field(
                SPAWN_GRAPH_POLICY_ARG,
                value_serde::to_value(&policy).expect("a RunPolicy converts to a Value"),
            ),
        ],
    }
}

/// Read the [`GraphType`], the [`Graph`] to run and its [`RunPolicy`] out of a
/// [`SPAWN_GRAPH`] call. A `graph_version` is `MAJOR.MINOR` (patch 0) or a
/// semver `MAJOR.MINOR.PATCH`; anything else is refused.
pub fn decode_spawn_graph(call: &Call) -> Result<(GraphType, Graph, RunPolicy), String> {
    if call.id != SPAWN_GRAPH {
        return Err(format!(
            "not the expected interpreter function: {}",
            call.id
        ));
    }
    let name: String = decode_arg(call, SPAWN_GRAPH_TYPE_ARG, "graph_type")?;
    let version: String = decode_arg(call, SPAWN_GRAPH_VERSION_ARG, "graph_version")?;
    let full = if version.matches('.').count() == 1 {
        format!("{version}.0")
    } else {
        version.clone()
    };
    let version = semver::Version::parse(&full).map_err(|e| {
        format!("graph_version '{version}' is not MAJOR.MINOR or MAJOR.MINOR.PATCH: {e}")
    })?;
    let graph = decode_arg(call, SPAWN_GRAPH_GRAPH_ARG, "Graph")?;
    let policy = decode_arg(call, SPAWN_GRAPH_POLICY_ARG, "RunPolicy")?;
    Ok((GraphType { name, version }, graph, policy))
}

/// Build the [`Call`] that halts the run named by `task`. The task id travels
/// as a uuid ([`Value::Uuid`]), the type `halt`'s parameter is described with.
pub fn encode_halt(task: TaskId) -> Call {
    Call {
        module_id: Some(ID),
        id: HALT,
        args: vec![StructureField {
            id: HALT_ARG,
            value: Box::new(Value::from(task)),
        }],
    }
}

/// Read the [`TaskId`] out of a [`HALT`] call: a uuid, or the uuid's string
/// form.
pub fn decode_halt(call: &Call) -> Result<TaskId, String> {
    decode(call, HALT, HALT_ARG, "TaskId")
}

/// Convert the [`TaskHandle`] that [`SPAWN`] or [`SPAWN_GRAPH`] returns to a
/// [`Value`], to travel back as the call's return.
pub fn encode_spawn_result(handle: &TaskHandle) -> Value {
    value_serde::to_value(handle).expect("a TaskHandle converts to a Value")
}

/// Read the [`TaskHandle`] out of a [`SPAWN`] or [`SPAWN_GRAPH`] call's
/// return [`Value`].
pub fn decode_spawn_result(value: &Value) -> Result<TaskHandle, String> {
    value_serde::from_value(value.clone()).map_err(|e| format!("malformed TaskHandle: {e}"))
}

fn encode<T: Serialize>(function: Uuid, arg: Uuid, payload: &T) -> Call {
    let value = value_serde::to_value(payload).expect("a graph payload converts to a Value");
    Call {
        module_id: Some(ID),
        id: function,
        args: vec![StructureField {
            id: arg,
            value: Box::new(value),
        }],
    }
}

fn decode<T: DeserializeOwned>(
    call: &Call,
    function: Uuid,
    arg: Uuid,
    what: &str,
) -> Result<T, String> {
    if call.id != function {
        return Err(format!(
            "not the expected interpreter function: {}",
            call.id
        ));
    }
    decode_arg(call, arg, what)
}

/// Read one argument value out of `call` by id and convert it to `T` — the arg
/// half of [`decode`], reused by multi-argument calls like [`SPAWN`] and
/// [`SPAWN_GRAPH`].
fn decode_arg<T: DeserializeOwned>(call: &Call, arg: Uuid, what: &str) -> Result<T, String> {
    let field = call
        .args
        .iter()
        .find(|field| field.id == arg)
        .ok_or_else(|| format!("the call is missing its {what} argument"))?;
    value_serde::from_value(field.value.as_ref().clone())
        .map_err(|e| format!("malformed {what}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_call_round_trips_the_diff() {
        let diff = GraphDiff {
            remove_nodes: vec![Uuid::from_u128(7)],
            set_root: Some(Uuid::from_u128(9)),
            ..Default::default()
        };
        let call = encode_edit(&diff);
        assert_eq!(call.module_id, Some(ID));
        assert_eq!(call.id, EDIT);
        assert_eq!(decode_edit(&call).unwrap(), diff);
    }

    #[test]
    fn load_call_round_trips_the_graph() {
        let mut graph = Graph::empty();
        graph.root = Some(Uuid::from_u128(3));
        let call = encode_load(&graph);
        assert_eq!(call.module_id, Some(ID));
        assert_eq!(call.id, LOAD);
        assert_eq!(decode_load(&call).unwrap(), graph);
    }

    #[test]
    fn spawn_call_round_trips_the_call_and_policy() {
        let inner = Call {
            module_id: Some(Uuid::from_u128(1)),
            id: Uuid::from_u128(2),
            args: vec![],
        };
        let call = encode_spawn(&inner, RunPolicy::Concurrent);
        assert_eq!(call.module_id, Some(ID));
        assert_eq!(call.id, SPAWN);
        assert_eq!(decode_spawn(&call).unwrap(), (inner, RunPolicy::Concurrent));

        // A spawn call under the wrong function id is rejected.
        let mut wrong = call;
        wrong.id = Uuid::from_u128(1);
        assert!(decode_spawn(&wrong).is_err());
    }

    #[test]
    fn spawn_graph_call_round_trips_the_type_graph_and_policy() {
        let mut graph = Graph::empty();
        graph.root = Some(Uuid::from_u128(3));
        graph.nodes.insert(
            Uuid::from_u128(3),
            crate::graph::Node {
                id: Uuid::from_u128(3),
                function: Uuid::from_u128(4),
                ..Default::default()
            },
        );
        let graph_type = GraphType {
            name: "behavior-tree".to_string(),
            version: semver::Version::new(1, 0, 0),
        };
        let call = encode_spawn_graph(&graph_type, &graph, RunPolicy::Concurrent);
        assert_eq!(call.module_id, Some(ID));
        assert_eq!(call.id, SPAWN_GRAPH);
        assert_eq!(
            decode_spawn_graph(&call).unwrap(),
            (graph_type, graph, RunPolicy::Concurrent)
        );

        // Refused: another function id, a missing graph, a version that is not
        // semver.
        let mut wrong = call.clone();
        wrong.id = SPAWN;
        assert!(decode_spawn_graph(&wrong).is_err());
        let mut missing = call.clone();
        missing.args.retain(|arg| arg.id != SPAWN_GRAPH_GRAPH_ARG);
        assert!(decode_spawn_graph(&missing).is_err());
        let mut unversioned = call;
        for arg in &mut unversioned.args {
            if arg.id == SPAWN_GRAPH_VERSION_ARG {
                *arg.value = Value::String("one".to_string());
            }
        }
        assert!(decode_spawn_graph(&unversioned).is_err());

        // `MAJOR.MINOR` reads as patch 0.
        for arg in &mut unversioned.args {
            if arg.id == SPAWN_GRAPH_VERSION_ARG {
                *arg.value = Value::String("1.0".to_string());
            }
        }
        let (read, _, _) = decode_spawn_graph(&unversioned).expect("1.0 is a version");
        assert_eq!(read.version, semver::Version::new(1, 0, 0));
    }

    #[test]
    fn halt_call_round_trips_the_task_id() {
        let task = TaskId(Uuid::from_u128(5));
        let call = encode_halt(task);
        assert_eq!(call.module_id, Some(ID));
        assert_eq!(call.id, HALT);
        assert_eq!(
            *call.args[0].value,
            Value::Uuid(task.0),
            "a uuid on the wire"
        );
        assert_eq!(decode_halt(&call).unwrap(), task);

        // The uuid's string form reads too.
        let mut as_string = call;
        *as_string.args[0].value = Value::String(task.0.to_string());
        assert_eq!(decode_halt(&as_string).unwrap(), task);
    }

    #[test]
    fn spawn_result_round_trips_the_handle() {
        use arora_types::data::Key;
        let id = TaskId(Uuid::from_u128(5));
        let handle = TaskHandle {
            id,
            stop: encode_halt(id),
            status: Key::new("arora/tasks/m/f/5/status"),
            feedback: vec![Key::new("arora/tasks/m/f/5/feedback")],
            result: vec![Key::new("arora/tasks/m/f/5/result")],
            update: vec![],
        };
        let value = encode_spawn_result(&handle);
        assert_eq!(decode_spawn_result(&value).unwrap(), handle);
    }

    #[test]
    fn decode_rejects_malformed_calls() {
        // Wrong function id.
        let mut call = encode_edit(&GraphDiff::default());
        call.id = Uuid::from_u128(1);
        assert!(decode_edit(&call).is_err());

        // Missing the payload argument.
        let mut call = encode_edit(&GraphDiff::default());
        call.args.clear();
        assert!(decode_edit(&call).is_err());

        // The argument does not decode into the expected payload.
        let mut call = encode_edit(&GraphDiff::default());
        *call.args[0].value = arora_types::value::Value::Boolean(true);
        assert!(decode_edit(&call).is_err());
        let mut call = encode_load(&Graph::empty());
        *call.args[0].value = arora_types::value::Value::String("not a graph".to_string());
        assert!(decode_load(&call).is_err());
    }
}
