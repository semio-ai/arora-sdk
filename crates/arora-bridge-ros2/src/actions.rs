//! Exposing a device's spawnable methods as ROS 2 actions.
//!
//! The method plane ([`services`](crate::services)) mirrors callable methods as
//! services; this plane mirrors the *task-run* methods as **actions** — the ROS
//! shape for long-running, cancellable work. A method is action-shaped when its
//! return type is the behavior-tree `Status` enumeration: such a function is
//! tickable (it reports `Running`/`Success`/`Failure` across repeated
//! invocations), which is exactly what the interpreter's
//! `spawn` hosts as a task run. Conveniently, that is also the one return type
//! the service plane cannot carry (enumerations have no ROS 2 field type), so
//! actions claim precisely the methods services skip — the two planes never
//! overlap.
//!
//! Per action, the ROS protocol decomposes into three services and two topics
//! (see [the design article](https://design.ros2.org/articles/actions.html)).
//! Two of them carry fixed `action_msgs` types (cancel, status) and ride
//! ros2-client's typed endpoints; the other three carry messages synthesised
//! *at runtime* from the method signature, so they ride the raw byte-level
//! endpoints — driven through the shared CDR codec ([`arora_msgs_ros2::cdr`]),
//! like the service plane's requests:
//!
//! - **SendGoal request** = `goal_id: uint8[16]` + one field per method
//!   parameter (the goal), *flattened* — CDR serialises a nested struct's
//!   fields inline, so the flat form is byte-identical and needs no registry
//!   entry for a wrapper type.
//! - **GetResult response** = `status: int8` + the result value, typed lazily
//!   from what the run wrote to its result key ([`type_ref_of_value`]) — the
//!   device function defines the de-facto Result message by what it writes
//!   there.
//! - **Feedback message** = `goal_id: uint8[16]` + the feedback value, typed
//!   lazily the same way.
//!
//! The remaining fixed-type halves (the SendGoal response, the GetResult
//! request, the cancel service, the status topic) are handled inside
//! ros2-client's `RawActionServer` — the bridge never builds those bytes.
//!
//! The goal lifecycle itself lives in the [`GoalBook`]: goals are accepted on
//! `SendGoal` (spawned through `BridgeOp::Call` to the interpreter module's
//! `SPAWN`), advance by observing the run's per-goal status key on the
//! outbound state stream, cancel through the handle's stop call, and resolve
//! `GetResult` at terminal. The book is pure state — the node task drives it.

use arora_behavior_tree_types::{
    STATUS_ENUMERATION_ID, STATUS_FAILURE_VARIANT_ID, STATUS_RUNNING_VARIANT_ID,
    STATUS_SUCCESS_VARIANT_ID,
};
use arora_bridge::{BridgeCommand, BridgeOp, MethodSignature};
use arora_msgs_ros2::{cdr, ros2_representable, Ros2Registry};
use arora_types::call::{Call, CallResult};
use arora_types::data::{Key, StateChange};
use arora_types::gen_uuid_from_str;
use arora_types::module::low::TypeRef;
use arora_types::record::ty::FrozenTy;
use arora_types::ty::{self, low};
use arora_types::value::{Structure, StructureField, Value};
use arora_types::{value_serde, Uuid};
use futures::channel::{mpsc as fmpsc, oneshot};
use log::warn;
use ros2_client::action_msgs::{
    CancelGoalResponse, CancelGoalResponseEnum, GoalInfo, GoalStatus, GoalStatusArray,
    GoalStatusEnum,
};
use ros2_client::builtin_interfaces::Time;
use ros2_client::unique_identifier_msgs::UUID;
use ros2_client::{ActionTypeName, RawActionServer, RmwRequestId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc as tmpsc;

use crate::services::type_ref_of;

// =============================================================================
// The interpreter-module SPAWN ABI, on the value plane.
// =============================================================================
//
// Task runs are spawned through the engine's interpreter module — the
// well-known ids and call conventions `arora-behavior`'s `interpreter_module`
// defines. The bridge speaks that ABI over the value plane (a `Call` carrying
// serde-encoded values), so it needs the ids and the payload shapes but no code
// dependency on the engine crate. The ids are self-identifying ("arora" in
// ASCII leads the UUID) and stable; `abi_matches_the_interpreter_module` (a
// dev-dependency test) pins them against the defining crate.

/// The interpreter module's id on the engine.
const INTERPRETER_MODULE: Uuid = Uuid::from_u128(0x61726f72_6100_0000_0000_000000000001);
/// Function id of **spawn**: start a task run, returning its handle.
const SPAWN: Uuid = Uuid::from_u128(0x61726f72_6100_0000_0000_000000000006);
/// Argument id of SPAWN's first argument: the [`Call`] to run.
const SPAWN_CALL_ARG: Uuid = Uuid::from_u128(0x61726f72_6100_0000_0000_000000000007);
/// Argument id of SPAWN's second argument: the run policy.
const SPAWN_POLICY_ARG: Uuid = Uuid::from_u128(0x61726f72_6100_0000_0000_000000000008);

/// The engine's `RunPolicy`, mirrored for the value plane (serde encodes by
/// variant name, so the mirror travels identically). v1 spawns everything
/// `Concurrent`.
#[derive(serde::Serialize)]
enum RunPolicy {
    Concurrent,
}

/// Build the SPAWN call that starts `call` as a concurrent task run.
pub(crate) fn spawn_call(call: &Call) -> Call {
    Call {
        module_id: Some(INTERPRETER_MODULE),
        id: SPAWN,
        args: vec![
            StructureField {
                id: SPAWN_CALL_ARG,
                value: Box::new(value_serde::to_value(call).expect("a Call converts to a Value")),
            },
            StructureField {
                id: SPAWN_POLICY_ARG,
                value: Box::new(
                    value_serde::to_value(&RunPolicy::Concurrent)
                        .expect("a RunPolicy converts to a Value"),
                ),
            },
        ],
    }
}

/// The ROS 2 action name a method is exposed on: `/{namespace}/actions/{name}`.
pub(crate) fn action_name(namespace: &str, method: &str) -> String {
    format!("/{namespace}/actions/{method}")
}

/// Whether a method is action-shaped: it returns the behavior-tree `Status`
/// enumeration, the signature of a tickable, spawnable behavior.
pub(crate) fn is_action_shaped(signature: &MethodSignature) -> bool {
    matches!(
        &signature.function.return_ty,
        FrozenTy::FrozenScalar(scalar) if scalar.reference.id == STATUS_ENUMERATION_ID
    )
}

/// A method resolved to a ROS 2 action: the action's identity on the graph and
/// the synthesised request messages. Built by [`resolve`]; the node task creates
/// the action's endpoints from it and drives the lifecycle through a
/// [`GoalBook`].
#[derive(Clone)]
pub(crate) struct MethodAction {
    /// `/{namespace}/actions/{method}` — the action name on the ROS graph.
    pub name: String,
    /// The nominal action type, `arora/{method}`. Like the service plane's
    /// types these are the device's own methods, not standard `.action` files,
    /// so the type names the action; the messages are the synthesised types
    /// here.
    pub action_type: ActionTypeName,
    /// The SendGoal request: `goal_id` + one field per method parameter.
    pub send_goal_request_type: low::Type,
    pub module_id: Uuid,
    pub function_id: Uuid,
}

/// Field id of the `goal_id` field in the synthesised SendGoal/GetResult
/// requests — reserved, distinct from any method parameter id.
pub(crate) fn goal_id_field() -> Uuid {
    gen_uuid_from_str("action/goal_id")
}

/// Resolve every action-shaped method whose parameters ROS 2 can carry to a
/// [`MethodAction`]. Returns the resolved actions plus the names of
/// action-shaped methods that had to be skipped (an unrepresentable parameter)
/// so the caller can log the omission. Non-action-shaped methods are neither —
/// they belong to the service plane.
pub(crate) fn resolve(
    namespace: &str,
    signatures: &[MethodSignature],
    registry: &Ros2Registry,
) -> (Vec<MethodAction>, Vec<String>) {
    let mut actions = Vec::new();
    let mut skipped = Vec::new();
    for signature in signatures {
        if !is_action_shaped(signature) {
            continue;
        }
        let send_goal_request_type = send_goal_request_type(signature);
        if ros2_representable(&send_goal_request_type, registry.types()).is_err() {
            skipped.push(signature.name.clone());
            continue;
        }
        actions.push(MethodAction {
            name: action_name(namespace, &signature.name),
            action_type: ActionTypeName::new("arora", &signature.name),
            send_goal_request_type,
            module_id: signature.module_id,
            function_id: signature.id,
        });
    }
    (actions, skipped)
}

/// The `goal_id: uint8[16]` field every synthesised request leads with (a
/// flattened `unique_identifier_msgs/UUID` — the nested struct adds no bytes).
fn goal_id_low_field() -> (Uuid, low::StructureField) {
    (
        goal_id_field(),
        low::StructureField {
            name: "goal_id".to_string(),
            type_ref: TypeRef::FixedArray {
                id: *ty::U8_ID,
                len: 16,
            },
        },
    )
}

/// The SendGoal request type for a method: `goal_id` then one field per
/// parameter in declared order, each keeping the **parameter's id** — so a
/// decoded request's non-goal-id fields are the spawn call's arguments
/// verbatim.
fn send_goal_request_type(signature: &MethodSignature) -> low::Type {
    let function = &signature.function;
    let params = function.parameter_ordering.iter().filter_map(|id| {
        let parameter = function.parameters.get(id)?;
        Some((
            *id,
            low::StructureField {
                name: parameter.name.clone(),
                type_ref: type_ref_of(&parameter.ty),
            },
        ))
    });
    let fields = std::iter::once(goal_id_low_field()).chain(params);
    let name = format!("{}_SendGoal_Request", signature.name);
    low::Type {
        id: gen_uuid_from_str(&name),
        name,
        description: String::new(),
        kind: low::TypeKind::Structure(low::Structure::from_fields(fields)),
    }
}

/// The GetResult **response** type for a given terminal result value: `status:
/// int8` + the result wrapped in a single `result` field typed from the value —
/// or `status` alone when the run wrote no result. The device function defines
/// the de-facto Result message by what it writes to its result key.
pub(crate) fn get_result_response_type(result: Option<&Value>) -> Option<low::Type> {
    let status = (
        gen_uuid_from_str("action/status"),
        low::StructureField {
            name: "status".to_string(),
            type_ref: TypeRef::Scalar { id: *ty::I8_ID },
        },
    );
    let mut fields = vec![status];
    if let Some(value) = result {
        fields.push((
            gen_uuid_from_str("action/result"),
            low::StructureField {
                name: "result".to_string(),
                type_ref: type_ref_of_value(value)?,
            },
        ));
    }
    Some(low::Type {
        id: gen_uuid_from_str("action/GetResult_Response"),
        name: "GetResult_Response".to_string(),
        description: String::new(),
        kind: low::TypeKind::Structure(low::Structure::from_fields(fields)),
    })
}

/// The GetResult response value for [`get_result_response_type`]. `status` is
/// the ROS goal status (`action_msgs/GoalStatus`, an `int8`).
pub(crate) fn get_result_response_value(status: GoalStatusEnum, result: Option<Value>) -> Value {
    let mut fields = vec![StructureField {
        id: gen_uuid_from_str("action/status"),
        value: Box::new(Value::I8(status as i8)),
    }];
    if let Some(result) = result {
        fields.push(StructureField {
            id: gen_uuid_from_str("action/result"),
            value: Box::new(result),
        });
    }
    Value::Structure(Structure {
        id: gen_uuid_from_str("action/GetResult_Response"),
        fields,
    })
}

/// The Feedback message type for a given feedback value: `goal_id: uint8[16]` +
/// the feedback wrapped in a single `feedback` field typed from the value.
pub(crate) fn feedback_message_type(feedback: &Value) -> Option<low::Type> {
    let fields = [
        goal_id_low_field(),
        (
            gen_uuid_from_str("action/feedback"),
            low::StructureField {
                name: "feedback".to_string(),
                type_ref: type_ref_of_value(feedback)?,
            },
        ),
    ];
    Some(low::Type {
        id: gen_uuid_from_str("action/FeedbackMessage"),
        name: "FeedbackMessage".to_string(),
        description: String::new(),
        kind: low::TypeKind::Structure(low::Structure::from_fields(fields)),
    })
}

/// The Feedback message value for [`feedback_message_type`].
pub(crate) fn feedback_message_value(goal_id: [u8; 16], feedback: Value) -> Value {
    Value::Structure(Structure {
        id: gen_uuid_from_str("action/FeedbackMessage"),
        fields: vec![
            StructureField {
                id: goal_id_field(),
                value: Box::new(Value::ArrayU8(goal_id.to_vec())),
            },
            StructureField {
                id: gen_uuid_from_str("action/feedback"),
                value: Box::new(feedback),
            },
        ],
    })
}

/// The [`TypeRef`] a runtime [`Value`] carries on the wire — the lazy typing
/// used for feedback and result messages, whose types are defined by what the
/// run actually writes. `None` for values ROS 2 cannot carry as a field
/// (structures, enumerations, unit, options, maps): the caller logs and skips.
pub(crate) fn type_ref_of_value(value: &Value) -> Option<TypeRef> {
    let scalar = |id: &Uuid| Some(TypeRef::Scalar { id: *id });
    let array = |id: &Uuid| Some(TypeRef::Array { id: *id });
    match value {
        Value::Boolean(_) => scalar(&ty::BOOLEAN_ID),
        Value::U8(_) => scalar(&ty::U8_ID),
        Value::U16(_) => scalar(&ty::U16_ID),
        Value::U32(_) => scalar(&ty::U32_ID),
        Value::U64(_) => scalar(&ty::U64_ID),
        Value::I8(_) => scalar(&ty::I8_ID),
        Value::I16(_) => scalar(&ty::I16_ID),
        Value::I32(_) => scalar(&ty::I32_ID),
        Value::I64(_) => scalar(&ty::I64_ID),
        Value::F32(_) => scalar(&ty::F32_ID),
        Value::F64(_) => scalar(&ty::F64_ID),
        Value::String(_) => scalar(&ty::STRING_ID),
        Value::ArrayBoolean(_) => array(&ty::BOOLEAN_ID),
        Value::ArrayU8(_) => array(&ty::U8_ID),
        Value::ArrayU16(_) => array(&ty::U16_ID),
        Value::ArrayU32(_) => array(&ty::U32_ID),
        Value::ArrayU64(_) => array(&ty::U64_ID),
        Value::ArrayI8(_) => array(&ty::I8_ID),
        Value::ArrayI16(_) => array(&ty::I16_ID),
        Value::ArrayI32(_) => array(&ty::I32_ID),
        Value::ArrayI64(_) => array(&ty::I64_ID),
        Value::ArrayF32(_) => array(&ty::F32_ID),
        Value::ArrayF64(_) => array(&ty::F64_ID),
        Value::ArrayString(_) => array(&ty::STRING_ID),
        _ => None,
    }
}

/// The spawned run's handle, decoded off the **value plane**: the interpreter
/// module's SPAWN returns the engine's `TaskHandle` as a serde-encoded
/// [`Value`], and this mirror deserialises it by field name — the bridge needs
/// no code dependency on the engine crate that defines the type. Field names
/// match the engine's `TaskHandle` (`arora-behavior`).
#[derive(Debug, serde::Deserialize)]
pub(crate) struct SpawnedRun {
    /// The run's identity (mirrors `TaskId`, a bare uuid on the value plane).
    /// The book keys goals by ROS goal id; the run id documents the contract.
    #[allow(dead_code)]
    pub id: Uuid,
    /// The call that stops the run (reaches the interpreter's `halt`).
    pub stop: Call,
    /// The run's lifecycle status key.
    pub status: Key,
    /// Keys carrying the run's progress feedback.
    pub feedback: Vec<Key>,
    /// Keys carrying the run's result, written at terminal.
    pub result: Vec<Key>,
    /// Keys an observer may write to steer a live run.
    #[allow(dead_code)] // Live-goal updates are a follow-up; the field documents the contract.
    pub update: Vec<Key>,
}

/// Decode the [`SpawnedRun`] out of a SPAWN call's return [`Value`].
pub(crate) fn spawned_run_of(value: Value) -> Result<SpawnedRun, String> {
    arora_types::value_serde::from_value(value).map_err(|e| format!("malformed task handle: {e}"))
}

/// Build the spawn's inner [`Call`] from a decoded SendGoal request: the
/// request's fields minus `goal_id` are the call arguments verbatim (their
/// field ids are the parameter ids — see [`send_goal_request_type`]). Returns
/// the goal id alongside.
pub(crate) fn goal_call_of(action: &MethodAction, request: Value) -> Option<([u8; 16], Call)> {
    let Value::Structure(Structure { fields, .. }) = request else {
        return None;
    };
    let mut goal_id = None;
    let mut args = Vec::with_capacity(fields.len().saturating_sub(1));
    for field in fields {
        if field.id == goal_id_field() {
            if let Value::ArrayU8(bytes) = field.value.as_ref() {
                goal_id = <[u8; 16]>::try_from(bytes.as_slice()).ok();
            }
        } else {
            args.push(field);
        }
    }
    Some((
        goal_id?,
        Call {
            module_id: Some(action.module_id),
            id: action.function_id,
            args,
        },
    ))
}

// =============================================================================
// The goal lifecycle — pure state, driven by the node task.
// =============================================================================

/// What the run wrote that the ROS side must act on, produced by
/// [`GoalBook::observe`].
#[derive(Debug, PartialEq)]
pub(crate) enum GoalEvent {
    /// The goal reached a terminal status: publish the status array, resolve a
    /// pending (or future) GetResult.
    Terminal { goal_id: [u8; 16] },
    /// The run wrote fresh feedback: publish a feedback message.
    Feedback { goal_id: [u8; 16], value: Value },
}

/// One accepted goal: the ROS-side view of a task run.
pub(crate) struct Goal {
    /// The acceptance stamp, for the status array and the cancel policy's
    /// accepted-at-or-before matching.
    pub accepted: Time,
    /// The run's per-goal keys, from the spawn's `TaskHandle`.
    pub status_key: Key,
    pub feedback_keys: Vec<Key>,
    pub result_keys: Vec<Key>,
    /// The stop call that reaches the interpreter's `halt`.
    pub stop: Call,
    /// The ROS goal status. `Executing` from acceptance (the run starts on the
    /// next device step); `Canceling` once a cancel was accepted; terminal once
    /// the run's status key reports it.
    pub status: GoalStatusEnum,
    /// The last value seen on a result key, cached from the state stream so
    /// the terminal result needs no read-back round-trip (the run writes its
    /// result at or before the step that ends it, and the step's changes
    /// arrive as one flush).
    pub result: Option<Value>,
}

impl Goal {
    fn terminal(&self) -> bool {
        matches!(
            self.status,
            GoalStatusEnum::Succeeded | GoalStatusEnum::Aborted | GoalStatusEnum::Canceled
        )
    }
}

/// Every goal this action server knows, keyed by goal id — the bridge-side
/// lifecycle state. Pure state: the node task feeds it requests and state
/// changes and acts on the events it returns.
///
/// Terminal goals are kept (the ROS result cache) up to [`Self::CAPACITY`];
/// beyond it the oldest terminal goals are evicted, matching ROS's bounded
/// result caching.
#[derive(Default)]
pub(crate) struct GoalBook {
    goals: HashMap<[u8; 16], Goal>,
    /// Terminal goal ids in completion (eviction) order.
    finished: Vec<[u8; 16]>,
    /// GetResult requests awaiting their goal's terminal state, resolved by
    /// [`take_ready_results`](Self::take_ready_results).
    pending_results: Vec<([u8; 16], RmwRequestId)>,
}

impl GoalBook {
    /// How many terminal goals are kept for late GetResult requests.
    const CAPACITY: usize = 128;

    /// Accept a goal: record the spawned run's handle keys under `goal_id`.
    /// Rejected (`false`) on a duplicate goal id.
    #[allow(clippy::too_many_arguments)]
    pub fn accept(
        &mut self,
        goal_id: [u8; 16],
        accepted: Time,
        status_key: Key,
        feedback_keys: Vec<Key>,
        result_keys: Vec<Key>,
        stop: Call,
    ) -> bool {
        if self.goals.contains_key(&goal_id) {
            return false;
        }
        self.goals.insert(
            goal_id,
            Goal {
                accepted,
                status_key,
                feedback_keys,
                result_keys,
                stop,
                status: GoalStatusEnum::Executing,
                result: None,
            },
        );
        true
    }

    /// Observe one goal — the tests' window into the book.
    #[cfg(test)]
    pub fn get(&self, goal_id: &[u8; 16]) -> Option<&Goal> {
        self.goals.get(goal_id)
    }

    /// Every live and cached goal with its acceptance stamp and status — the
    /// content of the `GoalStatusArray`.
    pub fn statuses(&self) -> impl Iterator<Item = ([u8; 16], Time, GoalStatusEnum)> + '_ {
        self.goals.iter().map(|(id, g)| (*id, g.accepted, g.status))
    }

    /// Queue a GetResult request for `goal_id`; it resolves through
    /// [`take_ready_results`](Self::take_ready_results) once the goal is
    /// terminal (immediately, if it already is).
    pub fn queue_result_request(&mut self, goal_id: [u8; 16], request: RmwRequestId) {
        self.pending_results.push((goal_id, request));
    }

    /// Drain every queued GetResult request whose goal has reached a terminal
    /// state: `(request, terminal status, cached result value)`. Requests for
    /// unknown goals resolve too — as `Unknown` with no result — rather than
    /// dangling forever.
    pub fn take_ready_results(&mut self) -> Vec<(RmwRequestId, GoalStatusEnum, Option<Value>)> {
        let mut ready = Vec::new();
        self.pending_results
            .retain(|(goal_id, request)| match self.goals.get(goal_id) {
                Some(goal) if goal.terminal() => {
                    ready.push((*request, goal.status, goal.result.clone()));
                    false
                }
                Some(_) => true,
                None => {
                    ready.push((*request, GoalStatusEnum::Unknown, None));
                    false
                }
            });
        ready
    }

    /// Observe one outbound state change: advance every goal whose run keys it
    /// touches. A write to a run's status key maps onto the ROS status —
    /// `Running` keeps `Executing`; `Success` → `Succeeded`; `Failure` →
    /// `Aborted`, or `Canceled` when a cancel was accepted for the goal (the
    /// halt path ends the run with `Failure`; the cancel authority is this
    /// side, so it reports the cancel). Result-key writes are cached; a
    /// feedback-key write yields a feedback event.
    pub fn observe(&mut self, change: &StateChange) -> Vec<GoalEvent> {
        let mut events = Vec::new();
        for (goal_id, goal) in self.goals.iter_mut() {
            if goal.terminal() {
                continue;
            }
            for (key, value) in &change.set {
                let Some(value) = value else { continue };
                if goal.result_keys.contains(key) {
                    goal.result = Some(value.clone());
                } else if goal.feedback_keys.contains(key) {
                    events.push(GoalEvent::Feedback {
                        goal_id: *goal_id,
                        value: value.clone(),
                    });
                } else if *key == goal.status_key {
                    if let Some(status) = run_status_of(value) {
                        match status {
                            RunStatus::Running => {}
                            RunStatus::Success => {
                                goal.status = GoalStatusEnum::Succeeded;
                            }
                            RunStatus::Failure => {
                                goal.status = if goal.status == GoalStatusEnum::Canceling {
                                    GoalStatusEnum::Canceled
                                } else {
                                    GoalStatusEnum::Aborted
                                };
                            }
                        }
                        if goal.terminal() {
                            events.push(GoalEvent::Terminal { goal_id: *goal_id });
                        }
                    }
                }
            }
        }
        for event in &events {
            if let GoalEvent::Terminal { goal_id } = event {
                self.finished.push(*goal_id);
            }
        }
        self.evict();
        events
    }

    /// The goals a cancel request selects, per the ROS cancel policy:
    ///
    /// - zero goal id + zero stamp: every non-terminal goal;
    /// - zero goal id + stamp: goals accepted at or before the stamp;
    /// - goal id + zero stamp: that goal;
    /// - goal id + stamp: that goal, plus goals accepted at or before the stamp.
    ///
    /// Only non-terminal, non-canceling goals are selected. Each selected goal
    /// is marked `Canceling`; the caller issues the returned stop calls.
    pub fn cancel(&mut self, goal_id: [u8; 16], stamp: Time) -> Vec<([u8; 16], Time, Call)> {
        let all = goal_id == [0u8; 16];
        let zero_stamp = stamp == Time::ZERO;
        let mut selected = Vec::new();
        for (id, goal) in self.goals.iter_mut() {
            if goal.terminal() || goal.status == GoalStatusEnum::Canceling {
                continue;
            }
            let matches = if all {
                zero_stamp || goal.accepted <= stamp
            } else {
                *id == goal_id || (!zero_stamp && goal.accepted <= stamp)
            };
            if matches {
                goal.status = GoalStatusEnum::Canceling;
                selected.push((*id, goal.accepted, goal.stop.clone()));
            }
        }
        selected
    }

    /// Evict the oldest terminal goals beyond the result-cache capacity.
    fn evict(&mut self) {
        while self.finished.len() > Self::CAPACITY {
            let oldest = self.finished.remove(0);
            self.goals.remove(&oldest);
        }
    }
}

// =============================================================================
// The action task — one per action, driving its endpoints and its goal book.
// =============================================================================

/// Serve one action: the task the node task spawns per resolved
/// [`MethodAction`]. Owns the action's endpoints and its [`GoalBook`]
/// (single-owner, no locks) and selects over the four things that advance a
/// goal: a SendGoal request (decode → SPAWN → accept), a CancelGoal request
/// (stop calls → `Canceling`), a GetResult request (queued until terminal),
/// and the device's outbound state changes (the run's status/feedback/result
/// keys), forwarded by the node task on `changes`.
pub(crate) async fn action_task(
    server: RawActionServer,
    action: MethodAction,
    registry: Arc<Ros2Registry>,
    cmd_tx: fmpsc::UnboundedSender<BridgeCommand>,
    mut changes: tmpsc::UnboundedReceiver<StateChange>,
) {
    let mut book = GoalBook::default();
    loop {
        tokio::select! {
            goal = server.receive_goal_request() => match goal {
                Ok((request, bytes)) => {
                    handle_goal(&server, &action, &registry, &cmd_tx, &mut book, request, &bytes)
                        .await;
                }
                Err(e) => {
                    warn!("action '{}' stopped receiving goals: {e:?}", action.name);
                    break;
                }
            },
            cancel = server.receive_cancel_request() => match cancel {
                Ok((request, goal_info)) => {
                    handle_cancel(&server, &action, &cmd_tx, &mut book, request, goal_info).await;
                }
                Err(e) => {
                    warn!("action '{}' stopped receiving cancels: {e:?}", action.name);
                    break;
                }
            },
            result = server.receive_result_request() => match result {
                Ok((request, goal_id)) => {
                    book.queue_result_request(goal_id.uuid.into_bytes(), request);
                    flush_ready_results(&server, &action, &registry, &mut book);
                }
                Err(e) => {
                    warn!("action '{}' stopped receiving result requests: {e:?}", action.name);
                    break;
                }
            },
            change = changes.recv() => match change {
                Some(change) => {
                    observe_change(&server, &action, &registry, &mut book, &change);
                }
                // The node task ended; so does this action.
                None => break,
            },
        }
    }
}

/// The acceptance stamp for a new goal: system time, as the wire `Time`.
fn now() -> Time {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0);
    Time::from_nanos(nanos)
}

/// Dispatch `call` to the runtime as a [`BridgeOp::Call`] and await its result.
async fn runtime_call(
    cmd_tx: &fmpsc::UnboundedSender<BridgeCommand>,
    call: Call,
) -> Result<CallResult, String> {
    let (reply_tx, reply_rx) = oneshot::channel();
    cmd_tx
        .unbounded_send(BridgeCommand::new(BridgeOp::Call(call), reply_tx))
        .map_err(|_| "the runtime dropped its command stream".to_string())?;
    match reply_rx.await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("the runtime dropped the call".to_string()),
    }
}

/// A SendGoal request: decode the goal, spawn its run, accept — or reject when
/// anything on that path fails (a malformed request, a refused spawn, a
/// duplicate goal id).
async fn handle_goal(
    server: &RawActionServer,
    action: &MethodAction,
    registry: &Ros2Registry,
    cmd_tx: &fmpsc::UnboundedSender<BridgeCommand>,
    book: &mut GoalBook,
    request: RmwRequestId,
    bytes: &[u8],
) {
    let reject = |reason: String| {
        warn!("action '{}' rejects a goal: {reason}", action.name);
        let _ = server.respond_goal(request, false, Time::ZERO);
    };
    let decoded = match cdr::decode(&action.send_goal_request_type, registry.types(), bytes) {
        Ok(value) => value,
        Err(e) => return reject(format!("malformed SendGoal request: {e}")),
    };
    let Some((goal_id, call)) = goal_call_of(action, decoded) else {
        return reject("the SendGoal request carries no goal id".to_string());
    };
    let run = match runtime_call(cmd_tx, spawn_call(&call)).await {
        Ok(result) => match spawned_run_of(result.ret) {
            Ok(run) => run,
            Err(e) => return reject(e),
        },
        Err(e) => return reject(format!("spawn refused: {e}")),
    };
    let stamp = now();
    if !book.accept(
        goal_id,
        stamp,
        run.status,
        run.feedback,
        run.result,
        run.stop,
    ) {
        return reject("duplicate goal id".to_string());
    }
    if server.respond_goal(request, true, stamp).is_err() {
        warn!("action '{}' could not answer a goal request", action.name);
    }
    publish_statuses(server, action, book);
}

/// A CancelGoal request: mark the selected goals `Canceling`, issue their stop
/// calls (each reaches the interpreter's halt), and answer with the canceling
/// list. The goals reach `Canceled` when their runs end on the status keys.
async fn handle_cancel(
    server: &RawActionServer,
    action: &MethodAction,
    cmd_tx: &fmpsc::UnboundedSender<BridgeCommand>,
    book: &mut GoalBook,
    request: RmwRequestId,
    goal_info: GoalInfo,
) {
    let selected = book.cancel(goal_info.goal_id.uuid.into_bytes(), goal_info.stamp);
    for (goal_id, _, stop) in &selected {
        if let Err(e) = runtime_call(cmd_tx, stop.clone()).await {
            warn!(
                "action '{}': stopping goal {} failed: {e}",
                action.name,
                Uuid::from_bytes(*goal_id)
            );
        }
    }
    let response = CancelGoalResponse {
        return_code: if selected.is_empty() {
            CancelGoalResponseEnum::Rejected
        } else {
            CancelGoalResponseEnum::None
        },
        goals_canceling: selected
            .iter()
            .map(|(goal_id, accepted, _)| GoalInfo {
                goal_id: UUID {
                    uuid: Uuid::from_bytes(*goal_id),
                },
                stamp: *accepted,
            })
            .collect(),
    };
    if server.respond_cancel(request, response).is_err() {
        warn!("action '{}' could not answer a cancel request", action.name);
    }
    publish_statuses(server, action, book);
}

/// One outbound state change: advance the book and act on what it reports —
/// publish feedback, publish the status array, resolve ready results.
fn observe_change(
    server: &RawActionServer,
    action: &MethodAction,
    registry: &Ros2Registry,
    book: &mut GoalBook,
    change: &StateChange,
) {
    let events = book.observe(change);
    if events.is_empty() {
        return;
    }
    let mut terminal = false;
    for event in events {
        match event {
            GoalEvent::Terminal { .. } => terminal = true,
            GoalEvent::Feedback { goal_id, value } => {
                publish_feedback(server, action, registry, goal_id, value);
            }
        }
    }
    if terminal {
        publish_statuses(server, action, book);
        flush_ready_results(server, action, registry, book);
    }
}

/// Publish one feedback value, lazily typed. A value ROS cannot carry is
/// logged and dropped (no silent cap).
fn publish_feedback(
    server: &RawActionServer,
    action: &MethodAction,
    registry: &Ros2Registry,
    goal_id: [u8; 16],
    value: Value,
) {
    let Some(message_type) = feedback_message_type(&value) else {
        warn!(
            "action '{}': a feedback value has no ROS 2 field type; not published",
            action.name
        );
        return;
    };
    let message = feedback_message_value(goal_id, value);
    match cdr::encode(&message_type, registry.types(), &message) {
        Ok(bytes) => {
            if server.publish_feedback_raw(&bytes).is_err() {
                warn!("action '{}' could not publish feedback", action.name);
            }
        }
        Err(e) => warn!("action '{}': encoding feedback: {e}", action.name),
    }
}

/// Publish the status of every goal the book knows.
fn publish_statuses(server: &RawActionServer, action: &MethodAction, book: &GoalBook) {
    let statuses = GoalStatusArray {
        status_list: book
            .statuses()
            .map(|(goal_id, stamp, status)| GoalStatus {
                goal_info: GoalInfo {
                    goal_id: UUID {
                        uuid: Uuid::from_bytes(goal_id),
                    },
                    stamp,
                },
                status,
            })
            .collect(),
    };
    if server.publish_statuses(statuses).is_err() {
        warn!(
            "action '{}' could not publish its status array",
            action.name
        );
    }
}

/// Answer every queued GetResult request whose goal is terminal: `status:
/// int8` + the result the run wrote, lazily typed (status alone when the run
/// wrote none, or wrote a value ROS cannot carry — logged).
fn flush_ready_results(
    server: &RawActionServer,
    action: &MethodAction,
    registry: &Ros2Registry,
    book: &mut GoalBook,
) {
    for (request, status, result) in book.take_ready_results() {
        let (response_type, value) = match get_result_response_type(result.as_ref()) {
            Some(ty) => (ty, get_result_response_value(status, result)),
            None => {
                warn!(
                    "action '{}': a result value has no ROS 2 field type; answering with the \
                     status alone",
                    action.name
                );
                (
                    get_result_response_type(None).expect("the status-only response synthesises"),
                    get_result_response_value(status, None),
                )
            }
        };
        match cdr::encode(&response_type, registry.types(), &value) {
            Ok(bytes) => {
                if server.respond_result_raw(request, &bytes).is_err() {
                    warn!("action '{}' could not answer a result request", action.name);
                }
            }
            Err(e) => warn!("action '{}': encoding a result: {e}", action.name),
        }
    }
}

/// The three run outcomes a status key carries.
enum RunStatus {
    Running,
    Success,
    Failure,
}

/// Map a status-key value (the behavior-tree `Status` enumeration) to the run
/// outcome. `None` for any other value — a foreign write to the key is not a
/// lifecycle transition.
fn run_status_of(value: &Value) -> Option<RunStatus> {
    let Value::Enumeration(enumeration) = value else {
        return None;
    };
    if enumeration.id != STATUS_ENUMERATION_ID {
        return None;
    }
    if enumeration.variant_id == STATUS_RUNNING_VARIANT_ID {
        Some(RunStatus::Running)
    } else if enumeration.variant_id == STATUS_SUCCESS_VARIANT_ID {
        Some(RunStatus::Success)
    } else if enumeration.variant_id == STATUS_FAILURE_VARIANT_ID {
        Some(RunStatus::Failure)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_msgs_ros2::cdr;
    use arora_types::record::module::frozen::{Function, Parameter};
    use arora_types::record::ty::{FrozenScalar, FrozenTy, PrimitiveKind};
    use arora_types::record::{FrozenReference, Version};
    use arora_types::value::Enumeration;

    /// The LookAt-shaped method under test: a gaze policy plus a target point,
    /// returning the behavior-tree `Status` — indefinite tracking, the driving
    /// real-world action (`interaction_skills/action/LookAt`).
    fn look_at_signature() -> MethodSignature {
        let mut parameters = HashMap::new();
        let mut parameter_ordering = Vec::new();
        for (name, kind) in [
            ("policy", PrimitiveKind::String),
            ("x", PrimitiveKind::F64),
            ("y", PrimitiveKind::F64),
            ("z", PrimitiveKind::F64),
        ] {
            let id = gen_uuid_from_str(name);
            parameter_ordering.push(id);
            parameters.insert(
                id,
                Parameter {
                    name: name.to_string(),
                    ty: FrozenTy::from(kind),
                    mutable: false,
                },
            );
        }
        MethodSignature {
            module_id: gen_uuid_from_str("gaze-module"),
            id: gen_uuid_from_str("look_at"),
            name: "look_at".to_string(),
            function: Function {
                parameters,
                parameter_ordering,
                return_ty: status_return(),
            },
        }
    }

    /// The behavior-tree `Status` return type, as a frozen scalar reference —
    /// the action-shape marker.
    fn status_return() -> FrozenTy {
        FrozenTy::FrozenScalar(FrozenScalar {
            reference: FrozenReference {
                id: STATUS_ENUMERATION_ID,
                version: Version::parse("1.0.0").expect("a valid version"),
            },
        })
    }

    fn status_value(variant: Uuid) -> Value {
        Value::Enumeration(Enumeration {
            id: STATUS_ENUMERATION_ID,
            variant_id: variant,
            value: Box::new(Value::Unit),
        })
    }

    /// The value-plane SPAWN ABI this bridge speaks matches the defining crate
    /// (`arora-behavior`'s `interpreter_module`), id for id and byte for byte —
    /// the pin behind speaking the ABI without a runtime engine dependency.
    #[test]
    fn abi_matches_the_interpreter_module() {
        use arora_behavior::interpreter_module;
        let inner = Call {
            module_id: Some(Uuid::from_u128(1)),
            id: Uuid::from_u128(2),
            args: vec![],
        };
        assert_eq!(
            spawn_call(&inner),
            interpreter_module::encode_spawn(&inner, arora_behavior::RunPolicy::Concurrent),
            "the bridge's SPAWN call must be the interpreter module's, byte for byte"
        );

        // The SpawnedRun mirror decodes the engine's TaskHandle encoding.
        let task = arora_behavior::TaskId(Uuid::from_u128(5));
        let handle = arora_behavior::TaskHandle {
            id: task,
            stop: interpreter_module::encode_halt(task),
            status: Key::from("arora/tasks/m/f/5/status"),
            feedback: vec![Key::from("arora/tasks/m/f/5/feedback")],
            result: vec![Key::from("arora/tasks/m/f/5/result")],
            update: vec![Key::from("arora/tasks/m/f/5/update")],
        };
        let run = spawned_run_of(interpreter_module::encode_spawn_result(&handle))
            .expect("the mirror decodes the engine's encoding");
        assert_eq!(run.id, handle.id.0);
        assert_eq!(run.stop, handle.stop);
        assert_eq!(run.status, handle.status);
        assert_eq!(run.feedback, handle.feedback);
        assert_eq!(run.result, handle.result);
        assert_eq!(run.update, handle.update);
    }

    #[test]
    fn action_shape_is_the_status_return() {
        let look_at = look_at_signature();
        assert!(is_action_shaped(&look_at));

        // A plain method (an f64 return) is not an action…
        let mut speak = look_at_signature();
        speak.function.return_ty = FrozenTy::from(PrimitiveKind::F64);
        assert!(!is_action_shaped(&speak));

        // …and resolve picks exactly the action-shaped one.
        let registry = arora_msgs_ros2::registry();
        let (actions, skipped) = resolve("robot", &[look_at, speak], &registry);
        assert_eq!(actions.len(), 1);
        assert!(skipped.is_empty());
        assert_eq!(actions[0].name, "/robot/actions/look_at");
    }

    /// The SendGoal request round-trips through the real CDR codec, and the
    /// decoded request splits into the goal id and the spawn call whose args
    /// are the goal fields verbatim.
    #[test]
    fn send_goal_request_round_trips_and_becomes_the_spawn_call() {
        let registry = arora_msgs_ros2::registry();
        let (actions, _) = resolve("robot", &[look_at_signature()], &registry);
        let action = &actions[0];

        let goal_id = [7u8; 16];
        let request = Value::Structure(Structure {
            id: action.send_goal_request_type.id,
            fields: vec![
                StructureField {
                    id: goal_id_field(),
                    value: Box::new(Value::ArrayU8(goal_id.to_vec())),
                },
                StructureField {
                    id: gen_uuid_from_str("policy"),
                    value: Box::new(Value::String("track".to_string())),
                },
                StructureField {
                    id: gen_uuid_from_str("x"),
                    value: Box::new(Value::F64(1.0)),
                },
                StructureField {
                    id: gen_uuid_from_str("y"),
                    value: Box::new(Value::F64(0.5)),
                },
                StructureField {
                    id: gen_uuid_from_str("z"),
                    value: Box::new(Value::F64(0.25)),
                },
            ],
        });
        let bytes = cdr::encode(&action.send_goal_request_type, registry.types(), &request)
            .expect("encode SendGoal request");
        let decoded = cdr::decode(&action.send_goal_request_type, registry.types(), &bytes)
            .expect("decode SendGoal request");

        let (decoded_goal_id, call) = goal_call_of(action, decoded).expect("goal + call");
        assert_eq!(decoded_goal_id, goal_id);
        assert_eq!(call.module_id, Some(gen_uuid_from_str("gaze-module")));
        assert_eq!(call.id, gen_uuid_from_str("look_at"));
        assert_eq!(call.args.len(), 4, "the goal fields are the call args");
        assert!(call.args.iter().all(|arg| arg.id != goal_id_field()));
    }

    /// The GetResult response round-trips through the codec; the result message
    /// is typed lazily from the value the run wrote (LookAt's errno).
    #[test]
    fn responses_round_trip_with_lazily_typed_result() {
        let registry = arora_msgs_ros2::registry();

        // GetResult response: status + the errno the run wrote.
        let errno = Value::I32(-125); // ROS_ECANCELED
        let ty = get_result_response_type(Some(&errno)).expect("an i32 result is representable");
        let response = get_result_response_value(GoalStatusEnum::Canceled, Some(errno));
        let bytes = cdr::encode(&ty, registry.types(), &response).expect("encode");
        assert_eq!(
            cdr::decode(&ty, registry.types(), &bytes).unwrap(),
            response
        );

        // No result written: the response is the bare status.
        let ty = get_result_response_type(None).unwrap();
        let response = get_result_response_value(GoalStatusEnum::Aborted, None);
        let bytes = cdr::encode(&ty, registry.types(), &response).expect("encode");
        assert_eq!(
            cdr::decode(&ty, registry.types(), &bytes).unwrap(),
            response
        );
    }

    /// A feedback message wraps the goal id and the lazily-typed feedback value;
    /// a structure value has no lazy wire type and is declined.
    #[test]
    fn feedback_messages_type_lazily() {
        let registry = arora_msgs_ros2::registry();
        let feedback = Value::F32(0.75);
        let ty = feedback_message_type(&feedback).expect("an f32 feedback is representable");
        let message = feedback_message_value([3u8; 16], feedback);
        let bytes = cdr::encode(&ty, registry.types(), &message).expect("encode");
        assert_eq!(cdr::decode(&ty, registry.types(), &bytes).unwrap(), message);

        let opaque = Value::Structure(Structure {
            id: Uuid::nil(),
            fields: vec![],
        });
        assert!(feedback_message_type(&opaque).is_none());
    }

    // =========================================================================
    // The goal book.
    // =========================================================================

    fn keys(prefix: &str) -> (Key, Vec<Key>, Vec<Key>) {
        (
            Key::from(format!("{prefix}/status")),
            vec![Key::from(format!("{prefix}/feedback"))],
            vec![Key::from(format!("{prefix}/result"))],
        )
    }

    fn stop_call() -> Call {
        Call {
            module_id: Some(Uuid::from_u128(1)),
            id: Uuid::from_u128(2),
            args: vec![],
        }
    }

    fn accept(book: &mut GoalBook, goal_id: [u8; 16], accepted: Time, prefix: &str) {
        let (status, feedback, result) = keys(prefix);
        assert!(book.accept(goal_id, accepted, status, feedback, result, stop_call()));
    }

    #[test]
    fn a_goal_advances_from_the_status_key_and_caches_its_result() {
        let mut book = GoalBook::default();
        accept(
            &mut book,
            [1u8; 16],
            Time::from_nanos(10),
            "arora/tasks/m/f/run1",
        );

        // A Running status keeps the goal executing.
        let change = StateChange::set(
            "arora/tasks/m/f/run1/status",
            status_value(STATUS_RUNNING_VARIANT_ID),
        );
        assert!(book.observe(&change).is_empty());
        assert_eq!(
            book.get(&[1u8; 16]).unwrap().status,
            GoalStatusEnum::Executing
        );

        // The run writes its result, then succeeds — one flush can carry both;
        // the result is cached and the terminal event fires.
        let mut change = StateChange::set("arora/tasks/m/f/run1/result", Value::I32(0));
        change.set.insert(
            Key::from("arora/tasks/m/f/run1/status"),
            Some(status_value(STATUS_SUCCESS_VARIANT_ID)),
        );
        let events = book.observe(&change);
        assert_eq!(events, vec![GoalEvent::Terminal { goal_id: [1u8; 16] }]);
        let goal = book.get(&[1u8; 16]).unwrap();
        assert_eq!(goal.status, GoalStatusEnum::Succeeded);
        assert_eq!(goal.result, Some(Value::I32(0)));

        // Terminal goals no longer advance.
        let change = StateChange::set(
            "arora/tasks/m/f/run1/status",
            status_value(STATUS_FAILURE_VARIANT_ID),
        );
        assert!(book.observe(&change).is_empty());
        assert_eq!(
            book.get(&[1u8; 16]).unwrap().status,
            GoalStatusEnum::Succeeded
        );
    }

    #[test]
    fn feedback_writes_become_feedback_events() {
        let mut book = GoalBook::default();
        accept(
            &mut book,
            [2u8; 16],
            Time::from_nanos(10),
            "arora/tasks/m/f/run2",
        );
        let change = StateChange::set("arora/tasks/m/f/run2/feedback", Value::F32(0.5));
        assert_eq!(
            book.observe(&change),
            vec![GoalEvent::Feedback {
                goal_id: [2u8; 16],
                value: Value::F32(0.5),
            }]
        );
    }

    /// The LookAt cancel path: canceling marks the goal, and the halt-driven
    /// `Failure` then reads as `Canceled` — this side issued the cancel, so it
    /// reports it (`ROS_ECANCELED` travels in the result).
    #[test]
    fn a_canceled_goal_ends_canceled_not_aborted() {
        let mut book = GoalBook::default();
        accept(
            &mut book,
            [3u8; 16],
            Time::from_nanos(10),
            "arora/tasks/m/f/run3",
        );

        let selected = book.cancel([3u8; 16], Time::ZERO);
        assert_eq!(selected.len(), 1);
        assert_eq!(
            book.get(&[3u8; 16]).unwrap().status,
            GoalStatusEnum::Canceling
        );

        let change = StateChange::set(
            "arora/tasks/m/f/run3/status",
            status_value(STATUS_FAILURE_VARIANT_ID),
        );
        let events = book.observe(&change);
        assert_eq!(events, vec![GoalEvent::Terminal { goal_id: [3u8; 16] }]);
        assert_eq!(
            book.get(&[3u8; 16]).unwrap().status,
            GoalStatusEnum::Canceled
        );

        // A spontaneous Failure on a goal nobody canceled is Aborted.
        accept(
            &mut book,
            [4u8; 16],
            Time::from_nanos(11),
            "arora/tasks/m/f/run4",
        );
        let change = StateChange::set(
            "arora/tasks/m/f/run4/status",
            status_value(STATUS_FAILURE_VARIANT_ID),
        );
        book.observe(&change);
        assert_eq!(
            book.get(&[4u8; 16]).unwrap().status,
            GoalStatusEnum::Aborted
        );
    }

    /// The four cancel-request forms of the ROS cancel policy.
    #[test]
    fn cancel_selects_goals_per_the_ros_policy() {
        let mut book = GoalBook::default();
        accept(
            &mut book,
            [1u8; 16],
            Time::from_nanos(10),
            "arora/tasks/m/f/a",
        );
        accept(
            &mut book,
            [2u8; 16],
            Time::from_nanos(20),
            "arora/tasks/m/f/b",
        );
        accept(
            &mut book,
            [3u8; 16],
            Time::from_nanos(30),
            "arora/tasks/m/f/c",
        );

        // Goal id + zero stamp: that goal only.
        let selected = book.cancel([2u8; 16], Time::ZERO);
        assert_eq!(
            selected.iter().map(|(id, _, _)| *id).collect::<Vec<_>>(),
            vec![[2u8; 16]]
        );

        // Zero id + stamp: everything accepted at or before it (b is already
        // canceling and is not re-selected).
        let mut ids: Vec<_> = book
            .cancel([0u8; 16], Time::from_nanos(10))
            .into_iter()
            .map(|(id, _, _)| id)
            .collect();
        ids.sort();
        assert_eq!(ids, vec![[1u8; 16]]);

        // Zero id + zero stamp: every remaining non-canceling goal.
        let ids: Vec<_> = book
            .cancel([0u8; 16], Time::ZERO)
            .into_iter()
            .map(|(id, _, _)| id)
            .collect();
        assert_eq!(ids, vec![[3u8; 16]]);
    }

    #[test]
    fn duplicate_goal_ids_are_rejected() {
        let mut book = GoalBook::default();
        accept(
            &mut book,
            [5u8; 16],
            Time::from_nanos(10),
            "arora/tasks/m/f/run5",
        );
        let (status, feedback, result) = keys("arora/tasks/m/f/other");
        assert!(!book.accept(
            [5u8; 16],
            Time::from_nanos(11),
            status,
            feedback,
            result,
            stop_call()
        ));
    }
}
