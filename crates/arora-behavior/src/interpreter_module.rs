//! The behavior interpreter as a module: the well-known ids and call
//! conventions under which the runtime exposes its one
//! [`BehaviorInterpreter`](crate::BehaviorInterpreter) on the engine.
//!
//! The runtime assembles a function module under [`ID`] (the engine's generic
//! module builder) with two functions attached to the interpreter's entry
//! points:
//!
//! - [`LOAD`] → [`BehaviorInterpreter::load`](crate::BehaviorInterpreter::load):
//!   replace the running behavior with a whole [`Graph`];
//! - [`EDIT`] → [`BehaviorInterpreter::apply`](crate::BehaviorInterpreter::apply):
//!   apply a [`GraphDiff`] to it.
//!
//! Both payloads travel as one structured [`Value`] argument, converted
//! generically through [`arora_types::value_serde`] — any serde type flows,
//! no bespoke encoding. A `Call{module_id: ID, id: LOAD|EDIT}` — a remote's
//! `BridgeOp::Call`, or a behavior's own call bridge — reaches the
//! interpreter through the engine's normal dispatch, like any module
//! function.

use arora_types::call::Call;
use arora_types::value::StructureField;
use arora_types::value_serde;
use serde::de::DeserializeOwned;
use serde::Serialize;
use uuid::{uuid, Uuid};

use crate::graph::{Graph, GraphDiff};

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
        call.args[0].value = Box::new(arora_types::value::Value::Boolean(true));
        assert!(decode_edit(&call).is_err());
        let mut call = encode_load(&Graph::empty());
        call.args[0].value = Box::new(arora_types::value::Value::String("not a graph".to_string()));
        assert!(decode_load(&call).is_err());
    }
}
