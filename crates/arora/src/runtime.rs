//! The Arora step loop — the data plane, expressed as a function pipeline.
//!
//! # Portable, step-dispatched, single state owner
//!
//! Several things want to change the blackboard: the **bridge** (commands and
//! state from the remote), the **HAL** (sensor readings), and the **behavior**
//! (intent it writes while ticking). Rather than share the state behind a lock
//! and race, [`Arora`] gives it a single owner and dispatches the others as
//! serialized phases of one loop ([`step`](Arora::step)).
//!
//! Each phase is a **free function** taking explicit arguments and returning the
//! data the next phase needs; [`Arora::step`] is just the wiring that hands the
//! object's own fields to them, top to bottom:
//!
//! 1. [`tick_clock`] — advance the golden clock, yield this frame's time/`dt`;
//! 2. [`read_inbound`] — drain the HAL sensor feed and the bridge(s) into an
//!    [`Inbound`] (sensors, commands, control signals);
//! 3. [`ingest`] — apply sensors, publish the clock, and dispatch commands into
//!    the store;
//! 4. [`tick_behavior`] — tick the one interpreter against the shared store;
//! 5. [`flush`] — coalesce the store's change feed into one outbound
//!    [`StateChange`], golden keys filtered out;
//! 6. [`write_outbound`] — fan that change out to the HAL and every bridge.
//!
//! Only one phase touches the state at a time, so there is never concurrent
//! access — and no dedicated engine thread, just a dedicated *step*. Because
//! the phases are free functions over explicit state, each is unit-testable in
//! isolation; `Arora` is only a convenience holder for the engine and its
//! friends.
//!
//! ## Why it is built this way (web first)
//!
//! [`step`](Arora::step) is **synchronous and non-blocking**, and [`Arora`]
//! itself spawns no threads, owns no async runtime, and never sleeps. It touches
//! its I/O seams — the [`Bridge`] and the [`Hal`] — through their synchronous
//! poll/push surface: `bridge.try_recv()` / `bridge.try_send()` and the HAL's
//! [`updates`](Hal::updates) subscription / [`try_send`](Hal::try_send). Any
//! real async work (a WebSocket, Zenoh, DDS) lives *inside* those
//! implementations, each owning its own task; the step never sees it.
//!
//! The embedder just drives `step`:
//!
//! - **native**: call [`run`](Arora::run) (a thin `step` loop) on a thread;
//! - **web**: drive `step()` from `requestAnimationFrame` — or run the whole
//!   thing inside a Web Worker.
//!
//! Because the loop owns no async runtime and only pokes synchronous seams, it
//! moves into a Web Worker unchanged: the worker boundary is the seam's problem.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use arora_behavior::{golden, BehaviorContext, BehaviorInterpreter, BehaviorStatus};
use arora_behavior_tree::{
    behavior::BehaviorTreeInterpreter, schema_groot, tree_node::TreeNode, BehaviorTree,
    ModuleFunction,
};
use arora_bridge::{Bridge, BridgeCommand, BridgeOp, Inbound as BridgeInbound};
use arora_engine::engine::PinnedEngine;
use arora_hal::Hal;
use arora_types::call::CallResult;
use arora_types::data::{DataStore, Key, StateChange, Subscription};
use arora_types::value::Value;
use uuid::Uuid;

use crate::Arora;

/// What a [`step`](Arora::step) concluded.
#[derive(Debug, PartialEq, Eq)]
pub enum StepOutcome {
    /// The device is live; keep stepping.
    Live,
    /// The device was unregistered from the remote; stop stepping.
    Unregistered,
}

/// Something went wrong running a step.
#[derive(Debug)]
pub enum RuntimeError {
    /// A write to the data store failed.
    Store(String),
    /// A behavior tree failed to build or run.
    BehaviorTree(String),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeError::Store(m) => write!(f, "data store error: {m}"),
            RuntimeError::BehaviorTree(m) => write!(f, "behavior tree error: {m}"),
        }
    }
}

impl std::error::Error for RuntimeError {}

/// A point-in-time copy of the device's live indicators.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TelemetrySnapshot {
    /// Measured step-loop frequency in Hz. `None` until the embedder's loop
    /// measures it (the native [`run`](Arora::run) does; a custom `step` driver
    /// may not).
    pub loop_hz: Option<f32>,
    /// Whether a remote client currently claims the device (asks for data).
    pub claimed: bool,
    /// Name of the behavior currently installed, when one is set and was given a
    /// name.
    pub behavior: Option<String>,
}

/// Shared, read-only view over the device's live indicators — loop frequency,
/// claim state, current behavior. Cloneable and thread-safe: the step loop
/// writes, observers (a TUI, a GUI, a metrics exporter) read
/// [`snapshot`](Telemetry::snapshot) at their own pace.
#[derive(Clone, Default)]
pub struct Telemetry {
    inner: Arc<std::sync::RwLock<TelemetrySnapshot>>,
}

impl Telemetry {
    /// Copy the current indicator values.
    pub fn snapshot(&self) -> TelemetrySnapshot {
        self.inner.read().expect("telemetry lock poisoned").clone()
    }

    fn update(&self, apply: impl FnOnce(&mut TelemetrySnapshot)) {
        apply(&mut self.inner.write().expect("telemetry lock poisoned"));
    }
}

/// The golden clock: monotonic nanoseconds since the device started, advanced by
/// each step's `dt`. Zero at build; [`tick_clock`] moves it forward.
#[derive(Default)]
pub struct Clock {
    time_ns: u64,
}

/// The frame clock values a step publishes into the golden keys before ticking.
pub struct ClockValues {
    /// Monotonic nanoseconds since the device started, after this step's `dt`.
    pub time_ns: u64,
    /// Nanoseconds elapsed since the previous step (this step's `dt`).
    pub dt_ns: u64,
}

/// Everything drained from the I/O seams in one step, before it touches the
/// store: HAL sensor readings, remote commands, and the two control signals a
/// step reacts to (unregistration and claim state).
pub struct Inbound {
    /// Sensor readings drained from the HAL feed this step.
    pub sensors: Vec<StateChange>,
    /// Commands from the remote(s), each carrying its reply channel.
    pub commands: Vec<BridgeCommand>,
    /// A bridge reported the device unregistered (stop stepping).
    pub unregistered: bool,
    /// The latest claim toggle a bridge reported this step, if any.
    pub claim: Option<bool>,
}

// =============================================================================
// The step pipeline — free functions over explicit state.
// =============================================================================

/// Advance the golden clock by `dt` and return this frame's clock values. The
/// monotonic accumulator is exact integer nanoseconds (no float drift over a
/// long run). `dt` is the elapsed wall time since the previous step, measured
/// by the caller's driver.
pub fn tick_clock(clock: &mut Clock, dt: Duration) -> ClockValues {
    // A single step's delta is far under u64 nanoseconds; the cast is lossless
    // in practice, and `saturating_add` keeps a pathological run from wrapping.
    let dt_ns = dt.as_nanos() as u64;
    clock.time_ns = clock.time_ns.saturating_add(dt_ns);
    ClockValues {
        time_ns: clock.time_ns,
        dt_ns,
    }
}

/// Drain the HAL sensor feed and every bridge into one [`Inbound`]. Reads fan in
/// across all bridges. Non-blocking: each seam hands over what it has buffered
/// off its own transport task and yields when empty.
pub fn read_inbound(hal_updates: &Subscription, bridges: &[Arc<dyn Bridge>]) -> Inbound {
    let mut inbound = Inbound {
        sensors: Vec::new(),
        commands: Vec::new(),
        unregistered: false,
        claim: None,
    };
    // HAL sensor updates (a synchronous subscription).
    while let Some(change) = hal_updates.try_recv() {
        inbound.sensors.push(change);
    }
    // Bridge events, drained synchronously from each bridge (it buffers them off
    // its own transport task).
    for bridge in bridges {
        while let Some(event) = bridge.try_recv() {
            match event {
                BridgeInbound::Command(cmd) => inbound.commands.push(cmd),
                BridgeInbound::DeviceInfo(Ok(None)) => inbound.unregistered = true,
                BridgeInbound::DeviceInfo(Ok(Some(_info))) => { /* TODO: apply device info */ }
                BridgeInbound::DeviceInfo(Err(_e)) => { /* TODO: surface bridge error */ }
                BridgeInbound::DataRequested(requested) => inbound.claim = Some(requested),
            }
        }
    }
    inbound
}

/// Apply this step's inbound into the store: sensor readings first, then the
/// remote commands (each replied to on its channel), then the golden clock. The
/// clock lands *last* here but still before any behavior ticks, since `ingest`
/// runs before [`tick_behavior`]; [`flush`] keeps the golden namespace off the
/// wire. Drains `inbound.sensors` and `inbound.commands`.
pub fn ingest(
    store: &dyn DataStore,
    clock: &ClockValues,
    inbound: &mut Inbound,
    function_index: &HashMap<Uuid, ModuleFunction>,
) -> Result<(), RuntimeError> {
    for change in inbound.sensors.drain(..) {
        store
            .write(change)
            .map_err(|e| RuntimeError::Store(e.to_string()))?;
    }
    for cmd in inbound.commands.drain(..) {
        apply_command(store, function_index, cmd)?;
    }
    // Publish the frame clock into the golden keys. These writes go into the
    // store's change feed like any other, but `flush` filters the golden
    // namespace out of what it forwards outbound.
    let mut clock_change = StateChange::new();
    clock_change
        .set
        .insert(Key::from(golden::DT), Some(Value::U64(clock.dt_ns)));
    clock_change
        .set
        .insert(Key::from(golden::TIME), Some(Value::U64(clock.time_ns)));
    store
        .write(clock_change)
        .map_err(|e| RuntimeError::Store(e.to_string()))
}

/// Handle one command from the remote against the store / function index, then
/// reply on its channel.
fn apply_command(
    store: &dyn DataStore,
    function_index: &HashMap<Uuid, ModuleFunction>,
    cmd: BridgeCommand,
) -> Result<(), RuntimeError> {
    let result = match &cmd.op {
        BridgeOp::Get(keys) => {
            let values = store.read(keys);
            let array = values
                .into_iter()
                .map(|v| Value::Option(v.map(Box::new)))
                .collect();
            Ok(CallResult {
                ret: Value::ArrayValue(array),
                mutated: Vec::new(),
            })
        }
        BridgeOp::Update(change) => match store.write(change.clone()) {
            Ok(()) => Ok(CallResult {
                ret: Value::Unit,
                mutated: Vec::new(),
            }),
            Err(e) => Err(e.to_string()),
        },
        BridgeOp::Call(_call) => {
            // TODO(PR 5): dispatch the call through the engine (the golden
            // behavior-edit functions plug in exactly here).
            Err("call handling is not yet wired".to_string())
        }
        BridgeOp::ListKeys { prefix } => {
            // Introspection: enumerate the live (set) key paths, optionally
            // filtered by prefix, sorted for a deterministic reply.
            let snapshot = store.snapshot();
            let mut paths: Vec<String> = snapshot
                .storage
                .iter()
                .filter(|(_, value)| value.is_some())
                .map(|(key, _)| key.path.clone())
                .filter(|path| prefix.as_ref().is_none_or(|p| path.starts_with(p.as_str())))
                .collect();
            paths.sort();
            Ok(CallResult {
                ret: Value::ArrayValue(paths.into_iter().map(Value::String).collect()),
                mutated: Vec::new(),
            })
        }
        BridgeOp::ListMethods { prefix } => {
            // Introspection: enumerate registered module method names, optionally
            // filtered by prefix, sorted and deduped.
            let mut names: Vec<String> = function_index
                .values()
                .map(|f| f.function_name.clone())
                .filter(|name| prefix.as_ref().is_none_or(|p| name.starts_with(p.as_str())))
                .collect();
            names.sort();
            names.dedup();
            Ok(CallResult {
                ret: Value::ArrayValue(names.into_iter().map(Value::String).collect()),
                mutated: Vec::new(),
            })
        }
    };
    cmd.reply(result);
    Ok(())
}

/// Tick the one behavior interpreter (a tree, a node graph, …) against the
/// shared store. A no-op when none is installed. When the interpreter reports
/// [`BehaviorStatus::Done`] it is dropped (back to `None`) and cleared from
/// telemetry; while it is [`BehaviorStatus::Running`] it stays for the next
/// step.
pub fn tick_behavior(
    interpreter: &mut Option<Box<dyn BehaviorInterpreter>>,
    store: &dyn DataStore,
    engine: &mut PinnedEngine,
    telemetry: &Telemetry,
) -> Result<(), RuntimeError> {
    let Some(behavior) = interpreter.as_mut() else {
        return Ok(());
    };
    let mut ctx = BehaviorContext {
        store,
        caller: engine,
    };
    let status = behavior
        .tick(&mut ctx)
        .map_err(|e| RuntimeError::BehaviorTree(e.to_string()))?;
    if status == BehaviorStatus::Done {
        *interpreter = None;
        telemetry.update(|t| t.behavior = None);
    }
    Ok(())
}

/// Coalesce everything drained from the store's change feed this step into ONE
/// [`StateChange`], so the remote/hardware see a single, consistent update per
/// step. Changes are drained in order, so later ones win: a set overrides an
/// earlier unset of the same key (and vice versa). The golden clock keys are
/// runtime-local and dropped, so the wall-clock churning every frame never
/// reaches the wire.
pub fn flush(changes: &Subscription) -> StateChange {
    let mut merged = StateChange::new();
    while let Some(change) = changes.try_recv() {
        for (key, value) in change.set {
            if golden::is_golden(key.get_path()) {
                continue;
            }
            merged.unset.remove(&key);
            merged.set.insert(key, value);
        }
        for key in change.unset {
            if golden::is_golden(key.get_path()) {
                continue;
            }
            merged.set.remove(&key);
            merged.unset.insert(key);
        }
    }
    merged
}

/// Fan the coalesced outbound change out to the hardware and every bridge,
/// through their synchronous, non-blocking push seams. Each buffers/flushes on
/// its own task; none blocks the step.
pub fn write_outbound(hal: &dyn Hal, bridges: &[Arc<dyn Bridge>], out: &StateChange) {
    for bridge in bridges {
        bridge.try_send(out);
    }
    hal.try_send(out);
}

/// Derive the step's outcome from its inbound control signals and surface the
/// claim state through telemetry.
pub fn outcome_of(inbound: &Inbound, telemetry: &Telemetry) -> StepOutcome {
    if let Some(claimed) = inbound.claim {
        telemetry.update(|t| t.claimed = claimed);
    }
    if inbound.unregistered {
        StepOutcome::Unregistered
    } else {
        StepOutcome::Live
    }
}

// =============================================================================
// `Arora` — the object that holds the state and wires the pipeline.
// =============================================================================

impl Arora {
    /// A shared handle over the device's live indicators, for observers such as
    /// an operator UI. Clone it freely; it stays readable after the device stops
    /// (values simply freeze).
    pub fn telemetry(&self) -> Telemetry {
        self.telemetry.clone()
    }

    /// Install the behavior interpreter to tick each step, replacing any current
    /// one — a tree interpreter, a node-graph interpreter, or another executor.
    /// The device ticks it while it reports [`BehaviorStatus::Running`] and drops
    /// it once it reports [`BehaviorStatus::Done`].
    pub fn set_behavior_interpreter(&mut self, interpreter: Box<dyn BehaviorInterpreter>) {
        self.interpreter = Some(interpreter);
        self.telemetry.update(|t| t.behavior = None);
    }

    /// Like [`set_behavior_interpreter`](Arora::set_behavior_interpreter), with a
    /// display name the device reports through [`telemetry`](Arora::telemetry)
    /// while the behavior is installed.
    pub fn set_named_behavior_interpreter(
        &mut self,
        name: &str,
        interpreter: Box<dyn BehaviorInterpreter>,
    ) {
        self.interpreter = Some(interpreter);
        let name = name.to_string();
        self.telemetry.update(|t| t.behavior = Some(name));
    }

    /// Install a behavior tree (as Groot XML) as the interpreter, replacing any
    /// current one.
    ///
    /// Each Groot `{var}` is bound to the data store under its own name — the
    /// Direct convention (variable name == store key) — so a behavior reading or
    /// writing `{var}` reads/writes the store directly during the tick. The
    /// step's single-writer guarantee makes that race-free.
    pub fn set_groot_xml(&mut self, xml: &str) -> Result<(), RuntimeError> {
        let behavior = self.build_groot_interpreter(xml)?;
        self.set_behavior_interpreter(behavior);
        Ok(())
    }

    /// Like [`set_groot_xml`](Arora::set_groot_xml), with a display name the
    /// device reports through [`telemetry`](Arora::telemetry) while the tree is
    /// installed (e.g. the tree's file stem or its Groot tree id).
    pub fn set_named_groot_xml(&mut self, name: &str, xml: &str) -> Result<(), RuntimeError> {
        let behavior = self.build_groot_interpreter(xml)?;
        self.set_named_behavior_interpreter(name, behavior);
        Ok(())
    }

    /// Build a [`BehaviorTreeInterpreter`] from Groot XML, resolving variables to
    /// store slots under the Direct convention.
    fn build_groot_interpreter(
        &self,
        xml: &str,
    ) -> Result<Box<dyn BehaviorInterpreter>, RuntimeError> {
        let groot = schema_groot::BehaviorTree::try_from_groot_xml(xml)
            .map_err(|e| RuntimeError::BehaviorTree(format!("parse: {e:?}")))?;
        // `try_into_tree_node` fills `variables` as name → variable id.
        let mut variables = HashMap::new();
        let tree_node: TreeNode = groot
            .root
            .try_into_tree_node(self.function_index.as_ref(), &mut variables)
            .map_err(|e| RuntimeError::BehaviorTree(format!("build: {e:?}")))?;
        // Invert to variable id → name for the BT builder.
        let id_to_name: HashMap<Uuid, String> =
            variables.into_iter().map(|(name, id)| (id, name)).collect();
        // Direct convention: a variable resolves to the store slot under its name.
        let store = self.store.clone();
        let resolver = move |name: &str| Some(store.slot(&Key::from(name)));
        let behavior: BehaviorTree = tree_node
            .into_behavior_tree(&resolver, &id_to_name)
            .map_err(|e| RuntimeError::BehaviorTree(format!("instantiate: {e:?}")))?;
        Ok(Box::new(BehaviorTreeInterpreter::new(
            behavior,
            self.function_index.clone(),
        )))
    }

    /// Advance one step: the [module pipeline](self) top to bottom, wiring this
    /// object's fields into the phase functions. Non-blocking; touches the state
    /// from this (single) thread only.
    ///
    /// `dt` is the elapsed wall time since the previous step, measured by the
    /// caller's driver ([`run`](Arora::run) natively, `requestAnimationFrame` on
    /// the web — converted to a [`Duration`] at that boundary). It advances the
    /// monotonic clock, published (with the accumulated time) under the golden
    /// keys before any behavior ticks, so behaviors read timing from the store
    /// rather than as a tick argument.
    pub fn step(&mut self, dt: Duration) -> Result<StepOutcome, RuntimeError> {
        let clock = tick_clock(&mut self.clock, dt);
        let mut inbound = read_inbound(&self.hal_updates, &self.bridges);
        ingest(&*self.store, &clock, &mut inbound, &self.function_index)?;
        tick_behavior(
            &mut self.interpreter,
            &*self.store,
            &mut self.engine,
            &self.telemetry,
        )?;
        let out = flush(&self.store_changes);
        if !out.is_empty() {
            write_outbound(&*self.hal, &self.bridges, &out);
        }
        Ok(outcome_of(&inbound, &self.telemetry))
    }
}

/// Native convenience: drive [`step`](Arora::step) in a loop until the device is
/// unregistered, pacing with a short sleep. On the web, drive `step` from
/// `requestAnimationFrame` instead (this method would block the event loop).
#[cfg(not(target_arch = "wasm32"))]
impl Arora {
    pub fn run(&mut self) -> Result<(), RuntimeError> {
        // Measure the achieved step frequency over ~1 s windows and publish it
        // through the telemetry handle.
        let mut window_start = std::time::Instant::now();
        let mut steps_in_window: u32 = 0;
        // Wall-clock delta between steps, fed to `step` as the frame `dt`.
        let mut last_step = std::time::Instant::now();
        loop {
            let now = std::time::Instant::now();
            let dt = now.duration_since(last_step);
            last_step = now;
            if self.step(dt)? == StepOutcome::Unregistered {
                return Ok(());
            }
            steps_in_window += 1;
            let elapsed = window_start.elapsed();
            if elapsed >= Duration::from_secs(1) {
                let hz = steps_in_window as f32 / elapsed.as_secs_f32();
                self.telemetry.update(|t| t.loop_hz = Some(hz));
                window_start = std::time::Instant::now();
                steps_in_window = 0;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_bridge::{BridgeResult, DeviceInfo};
    use arora_hal::FakeHal;
    use arora_simple_data_store::{NamespacedStore, SimpleDataStore};
    use async_trait::async_trait;
    use futures::channel::oneshot;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// 16 ms as a step `dt` — a typical frame at ~60 Hz.
    const FRAME: Duration = Duration::from_millis(16);

    /// A bridge that reports the device unregistered on its first poll and is
    /// otherwise silent.
    #[derive(Default)]
    struct UnregisterBridge {
        done: AtomicBool,
    }

    #[async_trait]
    impl Bridge for UnregisterBridge {
        fn try_recv(&self) -> Option<BridgeInbound> {
            if !self.done.swap(true, Ordering::Relaxed) {
                Some(BridgeInbound::DeviceInfo(Ok(None)))
            } else {
                None
            }
        }
        fn try_send(&self, _change: &StateChange) {}
        async fn get_device_info(&self) -> BridgeResult<Option<DeviceInfo>> {
            Ok(None)
        }
        async fn update_device_info(
            &self,
            info: Option<DeviceInfo>,
        ) -> BridgeResult<Option<DeviceInfo>> {
            Ok(info)
        }
    }

    /// Build an [`Arora`] over a fresh [`FakeHal`] and the given bridge, with a
    /// fresh private store.
    fn build(bridge: Arc<dyn Bridge>) -> Arora {
        Arora::builder()
            .with_hal(Arc::new(FakeHal::new()))
            .with_bridge(bridge)
            .build()
            .expect("arora builds")
    }

    /// Like [`build`], but over a caller-provided store.
    fn build_in(bridge: Arc<dyn Bridge>, store: Arc<dyn DataStore>) -> Arora {
        Arora::builder()
            .with_hal(Arc::new(FakeHal::new()))
            .with_bridge(bridge)
            .with_data_store(store)
            .build()
            .expect("arora builds")
    }

    /// Drive `step` until the device reports unregistered, with a safety bound.
    /// Fully synchronous: the bridge hands over the unregister event through
    /// `try_recv` on the very next step, no pump to wait on.
    fn drive_until_unregistered(mut arora: Arora) {
        for _ in 0..1000 {
            if arora.step(FRAME).expect("step ok") == StepOutcome::Unregistered {
                return;
            }
        }
        panic!("device never reported unregistered");
    }

    #[test]
    fn builder_defaults_to_a_self_contained_device() {
        // No seams named: fake HAL, in-process bridge, private store, no behavior.
        let arora = Arora::builder().build().expect("default device builds");
        assert!(arora.interpreter.is_none(), "default has no behavior");
        assert_eq!(arora.bridges.len(), 1, "one default in-process bridge");
    }

    #[test]
    fn stops_when_unregistered() {
        let arora = build(Arc::new(UnregisterBridge::default()));
        drive_until_unregistered(arora);
    }

    #[test]
    fn runs_a_set_tree() {
        let xml = r#"<root main_tree_to_execute="MainTree">
  <BehaviorTree ID="MainTree">
    <Sequence name="11111111-1111-4111-8111-111111111111">
      <Succeed name="22222222-2222-4222-8222-222222222222" />
    </Sequence>
  </BehaviorTree>
</root>"#;
        let mut arora = build(Arc::new(UnregisterBridge::default()));
        arora.set_groot_xml(xml).expect("tree installs");
        drive_until_unregistered(arora);
    }

    #[tokio::test]
    async fn get_and_update_commands_round_trip() {
        let arora = build(Arc::new(UnregisterBridge::default()));
        let key = Key::from("greeting");

        // Update writes a value into the store.
        let (tx, rx) = oneshot::channel();
        let mut set = HashMap::new();
        set.insert(key.clone(), Some(Value::String("hi".into())));
        let change = StateChange {
            set,
            unset: std::collections::HashSet::new(),
        };
        apply_command(
            &*arora.store,
            &arora.function_index,
            BridgeCommand::new(BridgeOp::Update(change), tx),
        )
        .unwrap();
        assert!(rx.await.unwrap().is_ok(), "update should succeed");

        // Get reads it back, wrapped as Option inside an ArrayValue.
        let (tx, rx) = oneshot::channel();
        apply_command(
            &*arora.store,
            &arora.function_index,
            BridgeCommand::new(BridgeOp::Get(vec![key]), tx),
        )
        .unwrap();
        let result = rx.await.unwrap().expect("get should succeed");
        assert_eq!(
            result.ret,
            Value::ArrayValue(vec![Value::Option(Some(Box::new(Value::String(
                "hi".into()
            ))))])
        );
    }

    /// A bridge that stays silent and never unregisters, so `step` keeps
    /// returning `Live` and an installed behavior ticks deterministically.
    struct SilentBridge;

    #[async_trait]
    impl Bridge for SilentBridge {
        fn try_recv(&self) -> Option<BridgeInbound> {
            None
        }
        fn try_send(&self, _change: &StateChange) {}
        async fn get_device_info(&self) -> BridgeResult<Option<DeviceInfo>> {
            Ok(None)
        }
        async fn update_device_info(
            &self,
            info: Option<DeviceInfo>,
        ) -> BridgeResult<Option<DeviceInfo>> {
            Ok(info)
        }
    }

    /// A non-tree [`BehaviorInterpreter`]: writes one key through the shared
    /// store, done.
    struct WriteOnce;

    impl BehaviorInterpreter for WriteOnce {
        fn tick(
            &mut self,
            ctx: &mut BehaviorContext,
        ) -> Result<BehaviorStatus, arora_behavior::BehaviorError> {
            ctx.store
                .write(StateChange::set(
                    "from_behavior",
                    Value::String("hi".into()),
                ))
                .map_err(|e| arora_behavior::BehaviorError {
                    message: e.to_string(),
                })?;
            Ok(BehaviorStatus::Done)
        }
    }

    /// The device ticks a non-tree behavior just like a tree: swapping the
    /// interpreter is all it takes.
    #[tokio::test]
    async fn runs_an_installed_non_tree_behavior() {
        let mut arora = build(Arc::new(SilentBridge));
        arora.set_behavior_interpreter(Box::new(WriteOnce));

        // One step ticks the behavior, which writes through the shared store.
        arora.step(FRAME).expect("step");
        let (tx, rx) = oneshot::channel();
        apply_command(
            &*arora.store,
            &arora.function_index,
            BridgeCommand::new(BridgeOp::Get(vec![Key::from("from_behavior")]), tx),
        )
        .unwrap();
        let result = rx.await.unwrap().expect("get ok");
        assert_eq!(
            result.ret,
            Value::ArrayValue(vec![Value::Option(Some(Box::new(Value::String(
                "hi".into()
            ))))])
        );
    }

    #[tokio::test]
    async fn list_keys_enumerates_the_store_by_prefix() {
        let arora = build(Arc::new(UnregisterBridge::default()));

        // Seed three keys across two prefixes.
        let mut set = HashMap::new();
        set.insert(Key::from("face/mouth"), Some(Value::F32(0.5)));
        set.insert(Key::from("face/eyes"), Some(Value::F32(0.1)));
        set.insert(Key::from("body/hand"), Some(Value::F32(0.9)));
        let (tx, _rx) = oneshot::channel();
        apply_command(
            &*arora.store,
            &arora.function_index,
            BridgeCommand::new(
                BridgeOp::Update(StateChange {
                    set,
                    unset: std::collections::HashSet::new(),
                }),
                tx,
            ),
        )
        .unwrap();

        // ListKeys with a prefix returns only that subtree, sorted.
        let (tx, rx) = oneshot::channel();
        apply_command(
            &*arora.store,
            &arora.function_index,
            BridgeCommand::new(
                BridgeOp::ListKeys {
                    prefix: Some("face".into()),
                },
                tx,
            ),
        )
        .unwrap();
        let result = rx.await.unwrap().expect("list_keys ok");
        assert_eq!(
            result.ret,
            Value::ArrayValue(vec![
                Value::String("face/eyes".into()),
                Value::String("face/mouth".into()),
            ])
        );

        // ListMethods returns the registered method names as an array.
        let (tx, rx) = oneshot::channel();
        apply_command(
            &*arora.store,
            &arora.function_index,
            BridgeCommand::new(BridgeOp::ListMethods { prefix: None }, tx),
        )
        .unwrap();
        let methods = rx.await.unwrap().expect("list_methods ok");
        assert!(
            matches!(methods.ret, Value::ArrayValue(_)),
            "list_methods returns an array"
        );
    }

    /// A behavior that writes one key/value and is then `Done` — the minimal
    /// store-writing behavior, value-parameterized so two of them differ.
    struct WriteKey {
        key: &'static str,
        value: Value,
    }

    impl BehaviorInterpreter for WriteKey {
        fn tick(
            &mut self,
            ctx: &mut BehaviorContext,
        ) -> Result<BehaviorStatus, arora_behavior::BehaviorError> {
            ctx.store
                .write(StateChange::set(self.key, self.value.clone()))
                .map_err(|e| arora_behavior::BehaviorError {
                    message: e.to_string(),
                })?;
            Ok(BehaviorStatus::Done)
        }
    }

    /// An [`Arora`] built over a `NamespacedStore` writes through `step()` under
    /// the device namespace: a write driven through the store pipeline (here the
    /// bridge `Update` path) lands as `robotA/<key>` in the shared backend.
    ///
    /// Exercises the `Arc<dyn DataStore>` injection end-to-end: the device holds
    /// the namespaced view and never sees the prefix, while the mutualized
    /// `SimpleDataStore` ends up holding only the namespaced key.
    #[tokio::test]
    async fn device_over_namespaced_store_writes_under_namespace() {
        let shared = SimpleDataStore::new();
        let store: Arc<dyn DataStore> =
            Arc::new(NamespacedStore::new(Arc::new(shared.clone()), "robotA"));
        let arora = build_in(Arc::new(SilentBridge), store.clone());

        // Drive a write through the store pipeline.
        let (tx, rx) = oneshot::channel();
        let mut set = HashMap::new();
        set.insert(Key::from("greeting"), Some(Value::String("hi".into())));
        apply_command(
            &*arora.store,
            &arora.function_index,
            BridgeCommand::new(
                BridgeOp::Update(StateChange {
                    set,
                    unset: std::collections::HashSet::new(),
                }),
                tx,
            ),
        )
        .unwrap();
        assert!(rx.await.unwrap().is_ok(), "update should succeed");

        // In the shared backend the key lives under the device namespace…
        assert_eq!(
            shared.read(&[Key::from("robotA/greeting")]),
            vec![Some(Value::String("hi".into()))],
            "the write landed under the device namespace"
        );
        // …and NOT under the bare key.
        assert_eq!(
            shared.read(&[Key::from("greeting")]),
            vec![None],
            "the bare key must not be set in the shared store"
        );
    }

    /// ARORA-39 acceptance, end to end through `step()`: an installed behavior
    /// writes a key into the device-namespaced store, and *switching* to a
    /// different behavior changes what gets written.
    #[tokio::test]
    async fn behavior_writes_then_switching_changes_the_namespaced_store() {
        let shared = SimpleDataStore::new();
        let store: Arc<dyn DataStore> =
            Arc::new(NamespacedStore::new(Arc::new(shared.clone()), "robotA"));
        // SilentBridge never unregisters, so `step()` stays `Live` and ticks the
        // installed behavior each frame.
        let mut arora = build_in(Arc::new(SilentBridge), store);

        // A behavior writes greeting = "hi"; one step lands it under the namespace.
        arora.set_behavior_interpreter(Box::new(WriteKey {
            key: "greeting",
            value: Value::String("hi".into()),
        }));
        assert_eq!(arora.step(FRAME).expect("step"), StepOutcome::Live);
        assert_eq!(
            shared.read(&[Key::from("robotA/greeting")]),
            vec![Some(Value::String("hi".into()))],
            "the behavior's write landed under the device namespace"
        );

        // Switch to a different behavior; the next step changes the stored value.
        arora.set_behavior_interpreter(Box::new(WriteKey {
            key: "greeting",
            value: Value::String("bye".into()),
        }));
        assert_eq!(arora.step(FRAME).expect("step"), StepOutcome::Live);
        assert_eq!(
            shared.read(&[Key::from("robotA/greeting")]),
            vec![Some(Value::String("bye".into()))],
            "switching the installed behavior changed what was written"
        );
    }

    /// The device publishes the frame clock into the golden keys *before* it
    /// ticks, so a behavior reads `dt`/time from the store. Nanoseconds
    /// accumulate into `time`; `dt` reflects only the latest step.
    #[test]
    fn golden_clock_is_published_to_the_store_each_step() {
        let store = Arc::new(SimpleDataStore::new());
        let mut arora = build_in(Arc::new(SilentBridge), store.clone());

        // Before any step the golden keys are unset.
        assert_eq!(store.read(&[Key::from(golden::DT)]), vec![None]);
        assert_eq!(store.read(&[Key::from(golden::TIME)]), vec![None]);

        // Step at 16 ms: dt and elapsed time both read 16_000_000 ns.
        assert_eq!(
            arora.step(Duration::from_millis(16)).expect("step"),
            StepOutcome::Live
        );
        assert_eq!(
            store.read(&[Key::from(golden::DT)]),
            vec![Some(Value::U64(16_000_000))]
        );
        assert_eq!(
            store.read(&[Key::from(golden::TIME)]),
            vec![Some(Value::U64(16_000_000))]
        );

        // Step at 4 ms: dt resets to the latest delta, time accumulates to 20 ms.
        assert_eq!(
            arora.step(Duration::from_millis(4)).expect("step"),
            StepOutcome::Live
        );
        assert_eq!(
            store.read(&[Key::from(golden::DT)]),
            vec![Some(Value::U64(4_000_000))]
        );
        assert_eq!(
            store.read(&[Key::from(golden::TIME)]),
            vec![Some(Value::U64(20_000_000))]
        );
    }

    /// A bridge that records every `try_send` payload, and otherwise stays
    /// silent (never unregisters), so a test can inspect what the device
    /// actually forwards outbound.
    struct RecordingBridge {
        sent: Arc<std::sync::Mutex<Vec<StateChange>>>,
    }

    #[async_trait]
    impl Bridge for RecordingBridge {
        fn try_recv(&self) -> Option<BridgeInbound> {
            None
        }
        fn try_send(&self, change: &StateChange) {
            self.sent.lock().unwrap().push(change.clone());
        }
        async fn get_device_info(&self) -> BridgeResult<Option<DeviceInfo>> {
            Ok(None)
        }
        async fn update_device_info(
            &self,
            info: Option<DeviceInfo>,
        ) -> BridgeResult<Option<DeviceInfo>> {
            Ok(info)
        }
    }

    /// The golden clock keys stay local: the device never forwards them out to
    /// the bridge, even though an ordinary behavior write on the same step is
    /// forwarded. This is what keeps the wall-clock (which changes every frame)
    /// off the wire.
    #[test]
    fn golden_keys_are_not_forwarded_outbound() {
        let sent = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut arora = build(Arc::new(RecordingBridge { sent: sent.clone() }));

        // A behavior writes one ordinary key; that write must reach the bridge.
        arora.set_behavior_interpreter(Box::new(WriteKey {
            key: "greeting",
            value: Value::String("hi".into()),
        }));

        // Step a few times; `try_send` records synchronously, in-line with step.
        for _ in 0..5 {
            arora.step(FRAME).expect("step");
        }

        let sent = sent.lock().unwrap();
        let forwarded_keys: Vec<String> = sent
            .iter()
            .flat_map(|c| c.set.keys().map(|k| k.path.clone()))
            .collect();
        assert!(
            forwarded_keys.iter().any(|k| k.as_str() == "greeting"),
            "the ordinary behavior write should be forwarded outbound, got {forwarded_keys:?}"
        );
        assert!(
            !forwarded_keys.iter().any(|k| golden::is_golden(k.as_str())),
            "golden keys must never be forwarded outbound, got {forwarded_keys:?}"
        );
    }
}
