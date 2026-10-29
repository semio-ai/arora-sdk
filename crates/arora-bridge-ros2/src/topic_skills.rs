//! Skills on topics: a profile-declared topic whose every message spawns a
//! device task-run method.
//!
//! A skill that sets a state held until changed — ROS4HRI's `set_expression`,
//! Vizij's `set_facial_action_units` and `set_viseme` — rides a topic, not an
//! action: there is no lifecycle for the client to follow, and the latest
//! command is the one that counts. The bridge serves it with the same spawn
//! path as a bound action ([`actions`](crate::actions)): the message's fields
//! are routed onto the method's parameters and the call is spawned through
//! the interpreter module's SPAWN. What differs is the lifecycle around it:
//!
//! - nothing answers the publisher (fire-and-forget);
//! - the latest message wins. The run the previous message spawned is halted,
//!   if it has not ended, before the new one is spawned; messages that queue
//!   up while a spawn is in flight collapse to the newest, which is the state
//!   the publisher means.
//!
//! The binding is checked against the device's described methods
//! ([`resolve`]) at start and whenever they change, and a message whose
//! fields do not route is dropped with a warning, leaving the running state as
//! it was.

use std::sync::Arc;

use arora_bridge::{BridgeCommand, MethodSignature};
use arora_msgs_ros2::Ros2Registry;
use arora_types::call::Call;
use arora_types::data::{Key, StateChange};
use arora_types::ty::low;
use arora_types::value::Value;
use arora_types::Uuid;
use futures::channel::mpsc as fmpsc;
use futures::{FutureExt, StreamExt};
use log::warn;
use tokio::sync::mpsc as tmpsc;

use crate::actions::{
    resolve_routes, routed_args, run_status_of, runtime_call, spawn_call, spawned_run_of, task_run,
    BoundRoute, RunStatus,
};
use crate::conversions::MessageStream;
use crate::profile::TopicSkillBinding;
use crate::qos::Qos;

/// A [`TopicSkillBinding`] resolved against the device's methods and the
/// message registry: the subscription to make and the call each message
/// becomes.
pub(crate) struct TopicSkill {
    /// The absolute topic.
    pub topic: String,
    /// The registered message name, the subscription's type.
    pub ros_type: String,
    /// The binding's delivery profile.
    pub qos: Option<Qos>,
    /// The module of the method each message spawns.
    pub module_id: Uuid,
    /// The method each message spawns.
    pub function_id: Uuid,
    message_type: low::Type,
    routes: Vec<BoundRoute>,
}

impl TopicSkill {
    /// The call a decoded message spawns: the method, with each routed field
    /// as its argument.
    fn call_of(&self, message: &Value, registry: &Ros2Registry) -> Result<Call, String> {
        Ok(Call {
            module_id: Some(self.module_id),
            id: self.function_id,
            args: routed_args(message, &self.message_type, &self.routes, registry)?,
        })
    }
}

/// Resolve every topic-skill binding against the device's described methods
/// and the message registry. Returns the resolved skills plus one error per
/// binding that does not hold (an unregistered type, a missing or mis-shaped
/// method, an unroutable field), so the caller can refuse it loudly.
pub(crate) fn resolve(
    bindings: &[TopicSkillBinding],
    signatures: &[MethodSignature],
    registry: &Ros2Registry,
) -> (Vec<TopicSkill>, Vec<String>) {
    let mut skills = Vec::new();
    let mut errors = Vec::new();
    for binding in bindings {
        match resolve_binding(binding, signatures, registry) {
            Ok(skill) => skills.push(skill),
            Err(e) => errors.push(format!("skill topic '{}': {e}", binding.topic)),
        }
    }
    (skills, errors)
}

/// Resolve one binding, or say precisely why it does not hold.
fn resolve_binding(
    binding: &TopicSkillBinding,
    signatures: &[MethodSignature],
    registry: &Ros2Registry,
) -> Result<TopicSkill, String> {
    let message_type = registry
        .get_by_name(&binding.ros_type)
        .cloned()
        .ok_or_else(|| format!("unregistered message '{}'", binding.ros_type))?;
    let signature = task_run(signatures, &binding.function)?;
    let routes = resolve_routes(
        &binding.topic,
        &message_type,
        &binding.routes,
        signature,
        registry,
    )?;
    Ok(TopicSkill {
        topic: binding.topic.clone(),
        ros_type: binding.ros_type.clone(),
        qos: binding.qos,
        message_type,
        module_id: signature.module_id,
        function_id: signature.id,
        routes,
    })
}

/// The run the latest message spawned, for as long as it has not ended: what
/// the next message halts.
#[derive(Default)]
struct LatestRun {
    /// The live run's status key and the call that stops it.
    run: Option<(Key, Call)>,
}

impl LatestRun {
    /// Record the run just spawned.
    fn replace(&mut self, status: Key, stop: Call) {
        self.run = Some((status, stop));
    }

    /// The stop call of the live run, which is then no longer tracked.
    fn take_live(&mut self) -> Option<Call> {
        self.run.take().map(|(_, stop)| stop)
    }

    /// Forget the run once its status key reports that it ended, so a run
    /// that finished on its own is not halted.
    fn observe(&mut self, change: &StateChange) {
        let Some((status, _)) = &self.run else {
            return;
        };
        let ended = change
            .set
            .get(status)
            .and_then(|value| value.as_ref())
            .and_then(run_status_of)
            .is_some_and(|status| !matches!(status, RunStatus::Running));
        if ended {
            self.run = None;
        }
    }
}

/// Serve one topic skill: the task the node task spawns per resolved
/// [`TopicSkill`]. Owns the run its latest message spawned and selects over
/// the two things that move it: a message on the topic (halt the live run,
/// spawn the new one) and the device's outbound state changes, forwarded by
/// the node task, which say when the live run ended.
pub(crate) async fn topic_skill_task(
    skill: TopicSkill,
    registry: Arc<Ros2Registry>,
    cmd_tx: fmpsc::UnboundedSender<BridgeCommand>,
    messages: MessageStream,
    mut changes: tmpsc::UnboundedReceiver<StateChange>,
) {
    // Fused: draining the queue may meet the stream's end, and the next
    // select then reads that end again rather than polling a finished stream.
    let mut messages = messages.fuse();
    let mut latest = LatestRun::default();
    loop {
        tokio::select! {
            message = messages.next() => {
                let Some(mut message) = message else {
                    warn!("skill topic '{}' stopped receiving messages", skill.topic);
                    break;
                };
                // Latest wins: what queued up while the last spawn was in
                // flight collapses to the newest.
                while let Some(Some(newer)) = messages.next().now_or_never() {
                    message = newer;
                }
                // A run that already ended is not halted.
                while let Ok(change) = changes.try_recv() {
                    latest.observe(&change);
                }
                handle_message(&skill, &registry, &cmd_tx, &mut latest, &message).await;
            }
            change = changes.recv() => match change {
                Some(change) => latest.observe(&change),
                // The node task ended; so does this skill.
                None => break,
            },
        }
    }
}

/// One message: halt the live run, then spawn the message's call. A message
/// whose fields do not route is dropped and the live run kept.
async fn handle_message(
    skill: &TopicSkill,
    registry: &Ros2Registry,
    cmd_tx: &fmpsc::UnboundedSender<BridgeCommand>,
    latest: &mut LatestRun,
    message: &Value,
) {
    let call = match skill.call_of(message, registry) {
        Ok(call) => call,
        Err(e) => {
            warn!("skill topic '{}' drops a message: {e}", skill.topic);
            return;
        }
    };
    if let Some(stop) = latest.take_live() {
        if let Err(e) = runtime_call(cmd_tx, stop).await {
            warn!(
                "skill topic '{}': halting the previous run failed: {e}",
                skill.topic
            );
        }
    }
    match runtime_call(cmd_tx, spawn_call(&call)).await {
        Ok(result) => match spawned_run_of(result.ret) {
            Ok(run) => latest.replace(run.status, run.stop),
            Err(e) => warn!("skill topic '{}': {e}", skill.topic),
        },
        Err(e) => warn!("skill topic '{}': spawn refused: {e}", skill.topic),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::resolve_bound;
    use crate::profile::ExposureProfile;
    use arora_behavior::interpreter_module;
    use arora_behavior::{TaskHandle, TaskId};
    use arora_behavior_tree_types::{
        STATUS_ENUMERATION_ID, STATUS_RUNNING_VARIANT_ID, STATUS_SUCCESS_VARIANT_ID,
    };
    use arora_bridge::BridgeOp;
    use arora_msgs_ros2::{interaction_skills, std_skills, vizij_face_msgs};
    use arora_types::call::CallResult;
    use arora_types::gen_uuid_from_str;
    use arora_types::record::module::frozen::{Function, Parameter};
    use arora_types::record::ty::{FrozenScalar, FrozenTy, PrimitiveKind};
    use arora_types::record::{FrozenReference, Version};
    use arora_types::value::Enumeration;
    use arora_types::value_serde::bridge::to_value_seeded;
    use arora_types::AroraType;
    use std::collections::HashMap;
    use std::time::Duration;

    /// A task-run method `name(params…) -> Status` on the face module.
    fn task_run_signature(name: &str, params: &[(&str, PrimitiveKind)]) -> MethodSignature {
        let mut parameters = HashMap::new();
        let mut parameter_ordering = Vec::new();
        for (param, kind) in params {
            let id = gen_uuid_from_str(&format!("{name}/{param}"));
            parameter_ordering.push(id);
            parameters.insert(
                id,
                Parameter {
                    name: param.to_string(),
                    ty: FrozenTy::from(*kind),
                    mutable: false,
                },
            );
        }
        MethodSignature {
            module_id: gen_uuid_from_str("face-module"),
            id: gen_uuid_from_str(name),
            name: name.to_string(),
            function: Function {
                parameters,
                parameter_ordering,
                return_ty: FrozenTy::FrozenScalar(FrozenScalar {
                    reference: FrozenReference {
                        id: STATUS_ENUMERATION_ID,
                        version: Version::parse("1.0.0").unwrap(),
                    },
                }),
            },
        }
    }

    /// The face's skill contract, exactly as the ROS4HRI preset routes onto
    /// it: the methods, their parameter names and types.
    fn face_contract() -> Vec<MethodSignature> {
        use PrimitiveKind::*;
        vec![
            task_run_signature(
                "set_expression",
                &[("expression", String), ("valence", F32), ("arousal", F32)],
            ),
            task_run_signature(
                "set_facial_action_units",
                &[("codes", ArrayU8), ("intensities", ArrayF32)],
            ),
            task_run_signature("set_viseme", &[("viseme", U8)]),
            task_run_signature(
                "play_visemes",
                &[
                    ("visemes", ArrayU8),
                    ("starts", ArrayF32),
                    ("durations", ArrayF32),
                ],
            ),
            task_run_signature(
                "look_at",
                &[("policy", String), ("target", ArrayF32), ("frame", String)],
            ),
            task_run_signature("say", &[("text", String)]),
        ]
    }

    fn arg(call: &Call, function: &str, param: &str) -> Option<Value> {
        call.args
            .iter()
            .find(|arg| arg.id == gen_uuid_from_str(&format!("{function}/{param}")))
            .map(|arg| arg.value.as_ref().clone())
    }

    /// A generated message as the decoded value a subscription yields.
    fn value_of<M: AroraType + serde::Serialize>(message: &M) -> Value {
        let (ty, reg) = M::arora_type_with_registry();
        to_value_seeded(message, &ty, &reg).expect("message to value")
    }

    fn meta() -> std_skills::Meta {
        std_skills::Meta {
            caller: "test".into(),
            priority: std_skills::Meta::NORMAL_PRIORITY,
        }
    }

    fn action_units(codes: &[u8], intensities: &[f32]) -> Value {
        value_of(&vizij_face_msgs::SetFacialActionUnits {
            meta: meta(),
            facial_action_units: vizij_face_msgs::FacialActionUnits {
                codes: codes.to_vec(),
                intensities: intensities.to_vec(),
            },
        })
    }

    /// The preset's skills — on topics and on actions — all resolve against
    /// a face serving the contract, with no refusal.
    #[test]
    fn the_ros4hri_skills_resolve_against_the_face_contract() {
        let registry = arora_msgs_ros2::registry();
        let profile = ExposureProfile::ros4hri();
        let (skills, errors) = resolve(&profile.topic_skills, &face_contract(), &registry);
        assert!(errors.is_empty(), "{errors:?}");
        let topics: Vec<&str> = skills.iter().map(|s| s.topic.as_str()).collect();
        assert_eq!(
            topics,
            [
                "/skill/set_expression",
                "/skill/set_facial_action_units",
                "/skill/set_viseme"
            ]
        );
        let (actions, errors) = resolve_bound(&profile.actions, &face_contract(), &registry);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(actions.len(), 3);
    }

    /// A binding that does not hold is refused with its reason, never
    /// served half-way.
    #[test]
    fn a_topic_skill_binding_is_refused_with_a_reason() {
        let registry = arora_msgs_ros2::registry();
        let refused = |binding: TopicSkillBinding, signatures: &[MethodSignature]| {
            let (skills, errors) = resolve(&[binding], signatures, &registry);
            assert!(skills.is_empty());
            errors.into_iter().next().expect("one refusal")
        };
        let contract = face_contract();
        let error = refused(
            binding("vizij_face_msgs/Nope", "set_viseme", &[]),
            &contract,
        );
        assert!(error.contains("unregistered message"), "{error}");
        let error = refused(binding("vizij_face_msgs/SetViseme", "set_viseme", &[]), &[]);
        assert!(error.contains("no method 'set_viseme'"), "{error}");
        let mut not_a_task = task_run_signature("set_viseme", &[("viseme", PrimitiveKind::U8)]);
        not_a_task.function.return_ty = FrozenTy::from(PrimitiveKind::Unit);
        let error = refused(
            binding(
                "vizij_face_msgs/SetViseme",
                "set_viseme",
                &[("viseme.value", "viseme")],
            ),
            &[not_a_task],
        );
        assert!(error.contains("not a task run"), "{error}");
        // A u8 code onto a string parameter.
        let error = refused(
            binding(
                "vizij_face_msgs/SetViseme",
                "set_expression",
                &[("viseme.value", "expression")],
            ),
            &contract,
        );
        assert!(error.contains("does not fit"), "{error}");
        // The intensities onto the codes: an f32 array is not a u8 array.
        let error = refused(
            binding(
                "vizij_face_msgs/SetFacialActionUnits",
                "set_facial_action_units",
                &[("facial_action_units.intensities", "codes")],
            ),
            &contract,
        );
        assert!(error.contains("does not fit"), "{error}");
        let error = refused(
            binding(
                "vizij_face_msgs/SetViseme",
                "set_viseme",
                &[("viseme.nope", "viseme")],
            ),
            &contract,
        );
        assert!(error.contains("no field 'nope'"), "{error}");
    }

    /// Each preset topic skill turns its message into the call the face
    /// contract takes.
    #[test]
    fn a_message_becomes_the_call_its_method_takes() {
        let registry = arora_msgs_ros2::registry();
        let (skills, _) = resolve(
            &ExposureProfile::ros4hri().topic_skills,
            &face_contract(),
            &registry,
        );
        let skill = |topic: &str| skills.iter().find(|s| s.topic == topic).unwrap();

        let call = skill("/skill/set_facial_action_units")
            .call_of(&action_units(&[6, 12], &[0.5, 1.0]), &registry)
            .expect("call");
        assert_eq!(call.id, gen_uuid_from_str("set_facial_action_units"));
        assert_eq!(call.module_id, Some(gen_uuid_from_str("face-module")));
        assert_eq!(
            arg(&call, "set_facial_action_units", "codes"),
            Some(Value::ArrayU8(vec![6, 12]))
        );
        assert_eq!(
            arg(&call, "set_facial_action_units", "intensities"),
            Some(Value::ArrayF32(vec![0.5, 1.0]))
        );

        let viseme = value_of(&vizij_face_msgs::SetViseme {
            meta: meta(),
            viseme: vizij_face_msgs::Viseme {
                value: vizij_face_msgs::Viseme::OU,
            },
        });
        let call = skill("/skill/set_viseme")
            .call_of(&viseme, &registry)
            .expect("call");
        assert_eq!(
            arg(&call, "set_viseme", "viseme"),
            Some(Value::U8(vizij_face_msgs::Viseme::OU))
        );

        let expression = value_of(&interaction_skills::SetExpression {
            meta: meta(),
            expression: arora_msgs_ros2::hri_msgs::Expression {
                header: Default::default(),
                expression: "happy".into(),
                valence: 0.5,
                arousal: -0.25,
                confidence: 1.0,
            },
        });
        let call = skill("/skill/set_expression")
            .call_of(&expression, &registry)
            .expect("call");
        assert_eq!(call.args.len(), 3, "confidence is not routed");
        assert_eq!(
            arg(&call, "set_expression", "expression"),
            Some(Value::String("happy".into()))
        );
        assert_eq!(
            arg(&call, "set_expression", "valence"),
            Some(Value::F32(0.5))
        );
        assert_eq!(
            arg(&call, "set_expression", "arousal"),
            Some(Value::F32(-0.25))
        );
    }

    fn status(variant: Uuid) -> Value {
        Value::Enumeration(Enumeration {
            id: STATUS_ENUMERATION_ID,
            variant_id: variant,
            value: Box::new(Value::Unit),
        })
    }

    /// The scripted runtime's side of the command stream: the next command,
    /// within a bound.
    async fn next_command(commands: &mut fmpsc::UnboundedReceiver<BridgeCommand>) -> BridgeCommand {
        tokio::time::timeout(Duration::from_secs(5), commands.next())
            .await
            .expect("a command within 5 s")
            .expect("the command stream is open")
    }

    /// Answer a SPAWN with run `n`'s handle, returning the spawned call.
    fn answer_spawn(command: BridgeCommand, n: u128) -> (Call, TaskHandle) {
        let BridgeOp::Call(call) = &command.op else {
            panic!("expected a call, got {:?}", command.op);
        };
        let (inner, _) = interpreter_module::decode_spawn(call)
            .unwrap_or_else(|e| panic!("expected a SPAWN, got {call:?}: {e}"));
        let task = TaskId(Uuid::from_u128(0x70 + n));
        let prefix = format!("arora/tasks/face/set_facial_action_units/run{n}");
        let handle = TaskHandle {
            id: task,
            stop: interpreter_module::encode_halt(task),
            status: Key::from(format!("{prefix}/status")),
            feedback: vec![],
            result: vec![],
            update: vec![],
        };
        command.reply(Ok(CallResult {
            ret: interpreter_module::encode_spawn_result(&handle),
            mutated: Vec::new(),
        }));
        (inner, handle)
    }

    /// Answer a halt, returning the halted run.
    fn answer_halt(command: BridgeCommand) -> TaskId {
        let BridgeOp::Call(call) = &command.op else {
            panic!("expected a call, got {:?}", command.op);
        };
        let task = interpreter_module::decode_halt(call)
            .unwrap_or_else(|e| panic!("expected a halt, got {call:?}: {e}"));
        command.reply(Ok(CallResult {
            ret: Value::Unit,
            mutated: Vec::new(),
        }));
        task
    }

    /// A topic-skill binding of `ros_type` on `/skill/test`, its message
    /// fields routed onto `function`'s parameters.
    fn binding(ros_type: &str, function: &str, routes: &[(&str, &str)]) -> TopicSkillBinding {
        TopicSkillBinding {
            topic: "/skill/test".into(),
            ros_type: ros_type.into(),
            function: function.into(),
            routes: routes
                .iter()
                .map(|(field, key)| crate::profile::FieldRoute {
                    field: (*field).into(),
                    key: (*key).into(),
                })
                .collect(),
            qos: None,
        }
    }

    fn set_facial_action_units_skill(registry: &Ros2Registry) -> TopicSkill {
        let binding = binding(
            "vizij_face_msgs/SetFacialActionUnits",
            "set_facial_action_units",
            &[
                ("facial_action_units.codes", "codes"),
                ("facial_action_units.intensities", "intensities"),
            ],
        );
        let (mut skills, errors) = resolve(&[binding], &face_contract(), registry);
        assert!(errors.is_empty(), "{errors:?}");
        skills.remove(0)
    }

    /// Latest wins: a new message halts the live run before spawning its
    /// own, and a run that ended on its own is not halted.
    #[tokio::test]
    async fn each_message_halts_the_live_run_then_spawns() {
        let registry = Arc::new(arora_msgs_ros2::registry());
        let skill = set_facial_action_units_skill(&registry);
        let (cmd_tx, mut commands) = fmpsc::unbounded();
        let (message_tx, message_rx) = fmpsc::unbounded::<Value>();
        let (change_tx, change_rx) = tmpsc::unbounded_channel();
        let task = tokio::spawn(topic_skill_task(
            skill,
            registry.clone(),
            cmd_tx,
            Box::pin(message_rx),
            change_rx,
        ));

        message_tx
            .unbounded_send(action_units(&[12], &[0.5]))
            .unwrap();
        let (call, run0) = answer_spawn(next_command(&mut commands).await, 0);
        assert_eq!(call.id, gen_uuid_from_str("set_facial_action_units"));
        assert_eq!(
            arg(&call, "set_facial_action_units", "codes"),
            Some(Value::ArrayU8(vec![12]))
        );

        // The run is still going: the next message halts it, then spawns.
        let mut running = StateChange::new();
        running
            .set
            .insert(run0.status.clone(), Some(status(STATUS_RUNNING_VARIANT_ID)));
        change_tx.send(running).unwrap();
        message_tx
            .unbounded_send(action_units(&[1], &[1.0]))
            .unwrap();
        assert_eq!(answer_halt(next_command(&mut commands).await), run0.id);
        let (call, run1) = answer_spawn(next_command(&mut commands).await, 1);
        assert_eq!(
            arg(&call, "set_facial_action_units", "intensities"),
            Some(Value::ArrayF32(vec![1.0]))
        );

        // The run ended on its own: the next message only spawns.
        let mut ended = StateChange::new();
        ended
            .set
            .insert(run1.status.clone(), Some(status(STATUS_SUCCESS_VARIANT_ID)));
        change_tx.send(ended).unwrap();
        message_tx
            .unbounded_send(action_units(&[2], &[0.25]))
            .unwrap();
        let (call, _run2) = answer_spawn(next_command(&mut commands).await, 2);
        assert_eq!(
            arg(&call, "set_facial_action_units", "codes"),
            Some(Value::ArrayU8(vec![2]))
        );

        // A message that does not route is dropped: no halt, no spawn.
        message_tx
            .unbounded_send(Value::String("not the message".into()))
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(200), commands.next())
                .await
                .is_err(),
            "an unroutable message reaches the runtime"
        );

        drop(message_tx);
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the task ends with its subscription")
            .unwrap();
    }

    /// Messages that queue up before the task reads them collapse to the
    /// newest: one spawn, of the state the publisher means now.
    #[tokio::test]
    async fn queued_messages_collapse_to_the_newest() {
        let registry = Arc::new(arora_msgs_ros2::registry());
        let skill = set_facial_action_units_skill(&registry);
        let (cmd_tx, mut commands) = fmpsc::unbounded();
        let (message_tx, message_rx) = fmpsc::unbounded::<Value>();
        let (_change_tx, change_rx) = tmpsc::unbounded_channel();
        for code in [1u8, 2, 3] {
            message_tx
                .unbounded_send(action_units(&[code], &[1.0]))
                .unwrap();
        }
        let _task = tokio::spawn(topic_skill_task(
            skill,
            registry.clone(),
            cmd_tx,
            Box::pin(message_rx),
            change_rx,
        ));
        let (call, _) = answer_spawn(next_command(&mut commands).await, 0);
        assert_eq!(
            arg(&call, "set_facial_action_units", "codes"),
            Some(Value::ArrayU8(vec![3]))
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(200), commands.next())
                .await
                .is_err(),
            "the superseded messages were spawned too"
        );
    }
}
