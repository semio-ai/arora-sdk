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
//! 0. [`sweep_now`] — move everything the seams already hold into the
//!    [`Pending`] buffers, without waiting;
//! 1. [`tick_clock`] + [`publish_clock`] — advance the golden clock and write
//!    it first, so the whole frame sees this frame's time/`dt`;
//! 2. [`apply_sensors`] — the HAL's readings, oldest first;
//! 3. [`apply_events`] — the bridge events, **after** the sensors: a remote
//!    update to a key beats this frame's sensor reading. Commands are
//!    dispatched and replied to here;
//! 4. [`tick_behavior`] — the one interpreter, **last**: its writes win the
//!    frame, and it saw what it overrode;
//! 5. [`flush`] — coalesce the store's change feed into one outbound
//!    [`StateChange`], golden keys filtered out;
//! 6. [`write_hal`] then [`write_bridges`] — fan that change out.
//!
//! Per-key precedence within one frame, total and visible in that order:
//! **behavior ▸ bridge ▸ HAL ▸ previous frame** — and inside each phase,
//! arrival order (the newest write wins). Only one phase touches the state at
//! a time, so there is never concurrent access — and no dedicated engine
//! thread, just a dedicated *step*. Because the phases are free functions over
//! explicit state, each is unit-testable in isolation; `Arora` is only a
//! convenience holder for the engine and its friends.
//!
//! ## One design, two drivers
//!
//! [`step`](Arora::step) is **synchronous and non-blocking**, and [`Arora`]
//! itself spawns no threads, owns no async runtime, and never sleeps. Its I/O
//! seams are owned streams in and non-blocking pushes out: the bridge
//! endpoints' inbound streams (taken once at build and merged), the HAL's
//! sensor feed, and their `try_send` counterparts. Any real async work (a
//! WebSocket, Zenoh, DDS) lives *inside* those implementations; the step never
//! sees it.
//!
//! - **native**: [`run`](Arora::run) paces `step` on a fixed-period metronome
//!   and, between ticks, drains the seams into the [`Pending`] buffers —
//!   buffering only, nothing touches the store outside `step`;
//! - **web / direct drive**: call `step()` yourself (from
//!   `requestAnimationFrame`, or a faster-than-realtime preview loop with a
//!   virtual `dt`) — phase 0's sweep picks the seams up with identical
//!   semantics, no runtime needed.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use arora_behavior::{golden, BehaviorContext, BehaviorInterpreter, BehaviorStatus};
use arora_behavior_tree::ModuleFunction;
use arora_bridge::{Bridge, BridgeCommand, BridgeOp, Inbound};
use arora_hal::Hal;
use arora_types::call::{CallBridge, CallResult};
use arora_types::data::{DataStore, Key, StateChange, Subscription};
use arora_types::value::Value;
use futures::{FutureExt, Stream, StreamExt};
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

/// The frame clock values a step publishes into the golden keys before anything
/// else runs.
pub struct ClockValues {
    /// Monotonic nanoseconds since the device started, after this step's `dt`.
    pub time_ns: u64,
    /// Nanoseconds elapsed since the previous step (this step's `dt`).
    pub dt_ns: u64,
}

/// Everything the seams delivered since the previous step, in arrival order per
/// seam. The driver only buffers here (natively [`run`](Arora::run)'s select
/// between ticks, plus phase 0's [`sweep_now`]); the next `step` applies and
/// drains it. Nothing touches the store outside `step`.
#[derive(Default)]
pub struct Pending {
    /// Sensor readings from the HAL feed.
    pub sensors: Vec<StateChange>,
    /// Inbound bridge events: commands, device-info updates, claim toggles.
    pub events: Vec<Inbound>,
}

// =============================================================================
// The step pipeline — free functions over explicit state.
// =============================================================================

/// Phase 0 — sweep the seams: move everything the streams already hold into
/// `pending`, without blocking or waiting. For an embedder that drives `step`
/// directly (web rAF, a preview loop) this is the whole inbound drain; under
/// [`run`](Arora::run) it just picks up what arrived since the select last
/// yielded, so both drivers see identical semantics.
pub fn sweep_now(
    hal_feed: &mut (impl Stream<Item = StateChange> + Unpin),
    inbound: &mut (impl Stream<Item = Inbound> + Unpin),
    pending: &mut Pending,
) {
    while let Some(Some(reading)) = hal_feed.next().now_or_never() {
        pending.sensors.push(reading);
    }
    while let Some(Some(event)) = inbound.next().now_or_never() {
        pending.events.push(event);
    }
}

/// Phase 1a — advance the golden clock by `dt` and return this frame's clock
/// values. The monotonic accumulator is exact integer nanoseconds (no float
/// drift over a long run). `dt` is the elapsed time since the previous step,
/// measured (or, for a preview, chosen) by the caller's driver.
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

/// Phase 1b — publish the frame clock into the golden keys, before anything
/// else touches the store: the whole frame (sensor applies, command handling,
/// the behavior tick) sees this frame's time. The writes go into the store's
/// change feed like any other, but [`flush`] filters the golden namespace out
/// of what it forwards outbound.
pub fn publish_clock(store: &dyn DataStore, clock: &ClockValues) -> Result<(), RuntimeError> {
    let mut change = StateChange::new();
    change
        .set
        .insert(Key::from(golden::DT), Some(Value::U64(clock.dt_ns)));
    change
        .set
        .insert(Key::from(golden::TIME), Some(Value::U64(clock.time_ns)));
    store
        .write(change)
        .map_err(|e| RuntimeError::Store(e.to_string()))
}

/// Phase 2 — apply the HAL's sensor readings, oldest first: within the frame,
/// a later reading of the same key wins. Returns the coalesced readings it
/// applied, so phase 6 can keep the hardware's own reports from being written
/// back to it ([`write_hal`]).
pub fn apply_sensors(
    store: &dyn DataStore,
    sensors: Vec<StateChange>,
) -> Result<StateChange, RuntimeError> {
    let mut applied = StateChange::new();
    for change in sensors {
        for (key, value) in &change.set {
            applied.unset.remove(key);
            applied.set.insert(key.clone(), value.clone());
        }
        for key in &change.unset {
            applied.set.remove(key);
            applied.unset.insert(key.clone());
        }
        store
            .write(change)
            .map_err(|e| RuntimeError::Store(e.to_string()))?;
    }
    Ok(applied)
}

/// Phase 3 — apply the bridge events, in arrival order, **after** the sensors:
/// a remote update to a key overwrites this frame's sensor reading
/// (deterministic phase order, not network timing). Commands are dispatched
/// against the store and replied to on their channel; the control signals fold
/// into the step's outcome — an unregistration ends the run, a claim toggle
/// lands in telemetry.
pub fn apply_events(
    store: &dyn DataStore,
    function_index: &HashMap<Uuid, ModuleFunction>,
    call_bridge: &mut dyn CallBridge,
    events: Vec<Inbound>,
    telemetry: &Telemetry,
) -> Result<StepOutcome, RuntimeError> {
    let mut outcome = StepOutcome::Live;
    for event in events {
        match event {
            Inbound::Command(cmd) => apply_command(store, function_index, call_bridge, cmd)?,
            Inbound::DeviceInfo(Ok(None)) => outcome = StepOutcome::Unregistered,
            Inbound::DeviceInfo(Ok(Some(_info))) => { /* TODO: apply device info */ }
            Inbound::DeviceInfo(Err(e)) => {
                // A dropped link is not an unregistration: the device keeps
                // running autonomously (the endpoint may reconnect on its own).
                log::warn!("bridge endpoint error: {e}");
            }
            Inbound::DataRequested(requested) => {
                telemetry.update(|t| t.claimed = requested);
            }
        }
    }
    Ok(outcome)
}

/// Handle one command from the remote against the store / function index, then
/// reply on its channel.
fn apply_command(
    store: &dyn DataStore,
    function_index: &HashMap<Uuid, ModuleFunction>,
    call_bridge: &mut dyn CallBridge,
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
        BridgeOp::Call(call) => {
            // A bridge Call dispatches through the engine's `CallBridge` (the
            // arora-types abstraction over the engine implementation).
            // TODO(PR 5b / edition): a Call to arora-behavior's golden
            // behavior-edit module id will reach `interpreter.apply(GraphDiff)`
            // through this same dispatch, once that id is registered against the
            // interpreter by the builder.
            match call.module_id {
                Some(module) => call_bridge
                    .arora_call(&module, call.clone())
                    .map_err(|e| format!("call failed: {e:?}")),
                None => Err("call is missing its module id".to_string()),
            }
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

/// Phase 4 — tick the one behavior interpreter (a tree, a node graph, …)
/// against the shared store, **last**: its writes win the frame, and it ticks
/// over everything the frame already applied (clock, sensors, remote updates).
/// A no-op when none is installed. When the interpreter reports
/// [`BehaviorStatus::Done`] it is dropped (back to `None`) and cleared from
/// telemetry; while it is [`BehaviorStatus::Running`] it stays for the next
/// step.
pub fn tick_behavior(
    interpreter: &mut Option<Box<dyn BehaviorInterpreter>>,
    store: &dyn DataStore,
    engine: &mut arora_engine::engine::PinnedEngine,
    telemetry: &Telemetry,
) -> Result<(), RuntimeError> {
    let Some(behavior) = interpreter.as_mut() else {
        return Ok(());
    };
    let mut ctx = BehaviorContext {
        store,
        call_bridge: engine,
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

/// Phase 5 — coalesce everything drained from the store's change feed this step
/// into ONE [`StateChange`], so the remote/hardware see a single, consistent
/// update per step. Changes are drained in order, so later ones win: a set
/// overrides an earlier unset of the same key (and vice versa). The golden
/// clock keys are runtime-local and dropped, so the wall-clock churning every
/// frame never reaches the wire.
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

/// Phase 6a — hand the coalesced outbound change to the hardware, through its
/// non-blocking push seam — **minus the keys whose frame-final value came from
/// the hardware itself** (`sensor_applied`, from [`apply_sensors`]): the HAL is
/// not told what it just reported. The bridges are (a remote wants sensor
/// state); and a key the behavior overwrote after the reading goes to the HAL
/// with the behavior's value, since that no longer matches the reading.
pub fn write_hal(hal: &dyn Hal, out: &StateChange, sensor_applied: &StateChange) {
    let mut for_hal = StateChange::new();
    for (key, value) in &out.set {
        if sensor_applied.set.get(key) == Some(value) {
            continue;
        }
        for_hal.set.insert(key.clone(), value.clone());
    }
    for key in &out.unset {
        if sensor_applied.unset.contains(key) {
            continue;
        }
        for_hal.unset.insert(key.clone());
    }
    if !for_hal.is_empty() {
        hal.try_send(&for_hal);
    }
}

/// Phase 6b — fan the same change out to every bridge endpoint. Each buffers
/// onto its own transport; none blocks the step.
pub fn write_bridges(bridges: &mut [Box<dyn Bridge>], out: &StateChange) {
    for bridge in bridges {
        bridge.try_send(out);
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

    /// Advance one step: the [module pipeline](self) top to bottom, wiring this
    /// object's fields into the phase functions. Non-blocking; touches the state
    /// from this (single) thread only.
    ///
    /// `dt` is the elapsed time since the previous step, measured by the
    /// caller's driver ([`run`](Arora::run) natively, `requestAnimationFrame` on
    /// the web) — or chosen freely by a driver with its own idea of time, e.g. a
    /// faster-than-realtime preview stepping a fixed virtual `dt`. It advances
    /// the monotonic clock, published (with the accumulated time) under the
    /// golden keys before anything else runs, so behaviors read timing from the
    /// store rather than as a tick argument.
    pub fn step(&mut self, dt: Duration) -> Result<StepOutcome, RuntimeError> {
        // 0. sweep — pick up everything the seams hold right now.
        sweep_now(&mut self.hal_feed, &mut self.inbound, &mut self.pending);
        // 1. time — golden keys first: the whole frame sees this clock.
        let clock = tick_clock(&mut self.clock, dt);
        publish_clock(&*self.store, &clock)?;
        // 2. HAL readings — oldest first; per key, the newest wins.
        let sensor_applied =
            apply_sensors(&*self.store, std::mem::take(&mut self.pending.sensors))?;
        // 3. bridge readings — after the HAL: a remote update beats this
        //    frame's sensor value. Commands dispatch and reply here.
        let outcome = apply_events(
            &*self.store,
            &self.function_index,
            &mut self.engine,
            std::mem::take(&mut self.pending.events),
            &self.telemetry,
        )?;
        // 4. behavior — the frame's last writer: its intent wins, and it saw
        //    what it overrode.
        tick_behavior(
            &mut self.interpreter,
            &*self.store,
            &mut self.engine,
            &self.telemetry,
        )?;
        // 5. flush — one coalesced change; golden keys stay local.
        let out = flush(&self.store_changes);
        // 6. writings — the hardware first (its own readings subtracted), then
        //    every remote.
        if !out.is_empty() {
            write_hal(&*self.hal, &out, &sensor_applied);
            write_bridges(&mut self.bridges, &out);
        }
        Ok(outcome)
    }
}

/// Native pacing: drive [`step`](Arora::step) to completion at a fixed cadence,
/// draining the I/O seams between ticks. On the web, drive `step` from
/// `requestAnimationFrame` instead (this method would monopolise the loop).
#[cfg(all(not(target_arch = "wasm32"), feature = "native"))]
impl Arora {
    /// The default inter-step period for [`run`](Arora::run): ~100 Hz.
    pub const DEFAULT_STEP_PERIOD: Duration = Duration::from_millis(10);

    /// Drive `step` to completion at a fixed cadence.
    ///
    /// `run` is `async`: each `step` stays fully synchronous, but between steps
    /// the loop `.await`s a [`tokio::time::interval`] metronome *and* the
    /// device's inbound seams in one select — whichever is ready first. A tick
    /// runs the next step; an inbound arrival is only **buffered** into
    /// [`Pending`] (nothing touches the store outside `step`), so the seams'
    /// channels stay drained without extra tasks, queues, or locks. The select
    /// is `biased` toward the tick: an event flood cannot starve the cadence.
    ///
    /// It therefore needs a Tokio runtime in scope (the binary drives it from
    /// `#[tokio::main]`); `step` itself owns none.
    ///
    /// `period` is the target time between steps — pass
    /// [`DEFAULT_STEP_PERIOD`](Arora::DEFAULT_STEP_PERIOD) for the ~100 Hz
    /// default. The metronome uses
    /// [`MissedTickBehavior::Delay`](tokio::time::MissedTickBehavior::Delay),
    /// so a step that overruns the period simply shifts the next tick out rather
    /// than firing a burst of catch-up ticks. The `dt` handed to `step` is the
    /// **actual** measured time since the previous step, not `period`.
    pub async fn run(&mut self, period: Duration) -> Result<(), RuntimeError> {
        let mut ticker = tokio::time::interval(period);
        // A slow step delays the next tick instead of bursting to catch up.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Measure the achieved step frequency over ~1 s windows and publish it
        // through the telemetry handle.
        let mut window_start = std::time::Instant::now();
        let mut steps_in_window: u32 = 0;
        // Wall-clock delta between steps, fed to `step` as the frame `dt`.
        let mut last_step = std::time::Instant::now();
        loop {
            tokio::select! {
                biased;
                // Paces at the fixed period; the first tick completes
                // immediately. Falls through to the step below.
                _ = ticker.tick() => {}
                // Between ticks, buffer what the seams deliver — in arrival
                // order per seam, applied by the next step.
                Some(reading) = self.hal_feed.next() => {
                    self.pending.sensors.push(reading);
                    continue;
                }
                Some(event) = self.inbound.next() => {
                    self.pending.events.push(event);
                    continue;
                }
            }
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_behavior_tree::behavior::BehaviorTreeInterpreter;
    use arora_bridge::{BridgeResult, DeviceInfo, FakeBridge, InboundStream};
    use arora_hal::FakeHal;
    use arora_simple_data_store::{NamespacedStore, SimpleDataStore};
    use async_trait::async_trait;
    use futures::channel::{mpsc, oneshot};
    use futures::stream;
    use std::rc::Rc;

    /// 16 ms as a step `dt` — a typical frame at ~60 Hz.
    const FRAME: Duration = Duration::from_millis(16);

    /// A bridge whose inbound stream reports the device unregistered, then
    /// ends.
    struct UnregisterBridge;

    #[async_trait]
    impl Bridge for UnregisterBridge {
        fn take_inbound(&mut self) -> InboundStream {
            Box::pin(stream::once(async { Inbound::DeviceInfo(Ok(None)) }))
        }
        fn try_send(&mut self, _change: &StateChange) {}
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
    fn build(bridge: Box<dyn Bridge>) -> Arora {
        Arora::builder()
            .with_hal(Box::new(FakeHal::new()))
            .with_bridge(bridge)
            .build()
            .expect("arora builds")
    }

    /// Like [`build`], but over a caller-provided store.
    fn build_in(bridge: Box<dyn Bridge>, store: Box<dyn DataStore>) -> Arora {
        Arora::builder()
            .with_hal(Box::new(FakeHal::new()))
            .with_bridge(bridge)
            .with_data_store(store)
            .build()
            .expect("arora builds")
    }

    /// Like [`build`], but injecting a behavior interpreter at build. Interpreters
    /// are executors set once at construction, not swapped afterwards, so a test
    /// that ticks a specific behavior hands it in here.
    fn build_with(bridge: Box<dyn Bridge>, interpreter: Box<dyn BehaviorInterpreter>) -> Arora {
        Arora::builder()
            .with_hal(Box::new(FakeHal::new()))
            .with_bridge(bridge)
            .with_behavior_interpreter(interpreter)
            .build()
            .expect("arora builds")
    }

    /// Like [`build_with`], but over a caller-provided store (so the injected
    /// interpreter can resolve against the same store the device ticks).
    fn build_in_with(
        bridge: Box<dyn Bridge>,
        store: Box<dyn DataStore>,
        interpreter: Box<dyn BehaviorInterpreter>,
    ) -> Arora {
        Arora::builder()
            .with_hal(Box::new(FakeHal::new()))
            .with_bridge(bridge)
            .with_data_store(store)
            .with_behavior_interpreter(interpreter)
            .build()
            .expect("arora builds")
    }

    /// Construct an empty behavior-tree interpreter (no module functions) with a
    /// Groot tree loaded into it against `store` — the construct-empty → load →
    /// inject flow, ready to hand to [`build_in_with`].
    fn groot_interpreter(xml: &str, store: &dyn DataStore) -> Box<dyn BehaviorInterpreter> {
        let mut interpreter = BehaviorTreeInterpreter::new(Rc::new(HashMap::new()));
        interpreter.load_groot(xml, store).expect("tree loads");
        Box::new(interpreter)
    }

    /// Drive `step` until the device reports unregistered, with a safety bound.
    /// Fully synchronous: phase 0's sweep picks the unregister event up from the
    /// bridge's stream on the very next step.
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
        // No seams named: fake HAL, no bridge (a standalone device is legal —
        // e.g. a preview), a private store, and the default executor — an
        // empty, idle behavior-tree interpreter.
        let arora = Arora::builder().build().expect("default device builds");
        assert!(
            arora.interpreter.is_some(),
            "default installs an (empty) behavior interpreter"
        );
        assert!(arora.bridges.is_empty(), "no bridge unless one is added");
    }

    #[test]
    fn a_bridgeless_device_steps() {
        // The zero-bridge device (a preview, a bench test) steps and stays live.
        let mut arora = Arora::builder().build().expect("builds");
        for _ in 0..5 {
            assert_eq!(arora.step(FRAME).expect("step"), StepOutcome::Live);
        }
    }

    #[test]
    fn a_default_devices_empty_interpreter_idles() {
        // The default empty interpreter ticks a no-op (Running), so it is never
        // dropped: it stays installed step after step, waiting for a behavior.
        let mut arora = build(Box::new(FakeBridge::new()));
        for _ in 0..5 {
            arora.step(FRAME).expect("step");
        }
        assert!(
            arora.interpreter.is_some(),
            "the empty interpreter idles and stays installed"
        );
    }

    #[test]
    fn stops_when_unregistered() {
        let mut arora = build(Box::new(UnregisterBridge));
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
        // Construct an empty interpreter, load the tree into it, inject at
        // build. The clone shares the same storage, so the tree's slots and the
        // device resolve against one blackboard.
        let store = SimpleDataStore::new();
        let interpreter = groot_interpreter(xml, &store);
        let arora = build_in_with(
            Box::new(UnregisterBridge),
            Box::new(store.clone()),
            interpreter,
        );
        drive_until_unregistered(arora);
    }

    #[tokio::test]
    async fn get_and_update_commands_round_trip() {
        let mut arora = build(Box::new(UnregisterBridge));
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
            &mut arora.engine,
            BridgeCommand::new(BridgeOp::Update(change), tx),
        )
        .unwrap();
        assert!(rx.await.unwrap().is_ok(), "update should succeed");

        // Get reads it back, wrapped as Option inside an ArrayValue.
        let (tx, rx) = oneshot::channel();
        apply_command(
            &*arora.store,
            &arora.function_index,
            &mut arora.engine,
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

    /// The device ticks a non-tree behavior just like a tree: injecting the
    /// interpreter at build is all it takes.
    #[tokio::test]
    async fn runs_an_installed_non_tree_behavior() {
        let mut arora = build_with(Box::new(FakeBridge::new()), Box::new(WriteOnce));

        // One step ticks the behavior, which writes through the shared store.
        arora.step(FRAME).expect("step");
        let (tx, rx) = oneshot::channel();
        apply_command(
            &*arora.store,
            &arora.function_index,
            &mut arora.engine,
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
        let mut arora = build(Box::new(UnregisterBridge));

        // Seed three keys across two prefixes.
        let mut set = HashMap::new();
        set.insert(Key::from("face/mouth"), Some(Value::F32(0.5)));
        set.insert(Key::from("face/eyes"), Some(Value::F32(0.1)));
        set.insert(Key::from("body/hand"), Some(Value::F32(0.9)));
        let (tx, _rx) = oneshot::channel();
        apply_command(
            &*arora.store,
            &arora.function_index,
            &mut arora.engine,
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
            &mut arora.engine,
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
            &mut arora.engine,
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
    /// store-writing behavior, key/value-parameterized so a test can vary what it
    /// writes.
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
        let store = NamespacedStore::new(Arc::new(shared.clone()), "robotA");
        let mut arora = build_in(Box::new(FakeBridge::new()), Box::new(store));

        // Drive a write through the store pipeline.
        let (tx, rx) = oneshot::channel();
        let mut set = HashMap::new();
        set.insert(Key::from("greeting"), Some(Value::String("hi".into())));
        apply_command(
            &*arora.store,
            &arora.function_index,
            &mut arora.engine,
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

    /// ARORA-39 acceptance, end to end through `step()`: the installed behavior's
    /// writes land under the device namespace in the shared backend, never under
    /// the bare key. The interpreter is injected once at build — it is an
    /// executor, not something the device swaps at runtime.
    #[tokio::test]
    async fn behavior_writes_land_in_the_namespaced_store() {
        let shared = SimpleDataStore::new();
        let store = NamespacedStore::new(Arc::new(shared.clone()), "robotA");
        // The fake bridge never unregisters, so `step()` stays `Live` and ticks
        // the installed behavior each frame.
        let mut arora = build_in_with(
            Box::new(FakeBridge::new()),
            Box::new(store),
            Box::new(WriteKey {
                key: "greeting",
                value: Value::String("hi".into()),
            }),
        );

        // The behavior writes greeting = "hi"; one step lands it under the namespace.
        assert_eq!(arora.step(FRAME).expect("step"), StepOutcome::Live);
        assert_eq!(
            shared.read(&[Key::from("robotA/greeting")]),
            vec![Some(Value::String("hi".into()))],
            "the behavior's write landed under the device namespace"
        );
        // …and NOT under the bare key.
        assert_eq!(
            shared.read(&[Key::from("greeting")]),
            vec![None],
            "the bare key must not be set in the shared store"
        );
    }

    /// The device publishes the frame clock into the golden keys *before* it
    /// ticks, so a behavior reads `dt`/time from the store. Nanoseconds
    /// accumulate into `time`; `dt` reflects only the latest step.
    #[test]
    fn golden_clock_is_published_to_the_store_each_step() {
        // The clone shares the same storage, so the test reads what the device
        // writes.
        let store = SimpleDataStore::new();
        let mut arora = build_in(Box::new(FakeBridge::new()), Box::new(store.clone()));

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

    /// A bridge that forwards every `try_send` payload down a channel, and is
    /// otherwise silent (never unregisters), so a test can inspect what the
    /// device actually pushes outbound — lock-free, like a real endpoint.
    struct RecordingBridge {
        sent: mpsc::UnboundedSender<StateChange>,
    }

    #[async_trait]
    impl Bridge for RecordingBridge {
        fn take_inbound(&mut self) -> InboundStream {
            Box::pin(stream::pending())
        }
        fn try_send(&mut self, change: &StateChange) {
            let _ = self.sent.unbounded_send(change.clone());
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
        let (sent_tx, mut sent_rx) = mpsc::unbounded();
        // A behavior that writes one ordinary key; that write must reach the bridge.
        let mut arora = build_with(
            Box::new(RecordingBridge { sent: sent_tx }),
            Box::new(WriteKey {
                key: "greeting",
                value: Value::String("hi".into()),
            }),
        );

        // Step a few times; `try_send` records synchronously, in-line with step.
        for _ in 0..5 {
            arora.step(FRAME).expect("step");
        }

        let mut forwarded_keys: Vec<String> = Vec::new();
        while let Ok(change) = sent_rx.try_recv() {
            forwarded_keys.extend(change.set.keys().map(|k| k.path.clone()));
        }
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
