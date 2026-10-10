//! The Arora step loop: the device's data plane.
//!
//! Several sources want to change the data storage — the bridges (remote state
//! and commands), the HAL (sensor readings), and the behavior (the intent it
//! writes while ticking). Rather than share the state behind a lock and race,
//! [`Arora`] owns it alone and serializes the others into the phases of one
//! [`step`](Arora::step).
//!
//! A device runs either on [`run`](Arora::run), which paces the steps itself,
//! or on an embedder calling [`step`](Arora::step) from its own clock.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use arora_behavior::{built_in, BehaviorContext, BehaviorInterpreter, BehaviorStatus};
use arora_behavior_tree::ModuleFunction;
use arora_bridge::{Bridge, BridgeCommand, BridgeOp, Inbound, MethodSignature};
use arora_hal::Hal;
use arora_types::call::{CallBridge, CallError, CallResult};
use arora_types::data::{DataStore, Key, KeyMeta, StateChange, Subscription};
use arora_types::value::Value;
use futures::{FutureExt, Stream, StreamExt};
use tokio::sync::watch;
use uuid::Uuid;
use web_time::Instant;

use crate::Arora;

/// Self-pacing: drive [`step`](Arora::step) to completion at a fixed cadence,
/// draining the I/O seams between ticks.
///
/// On the web it awaits between steps like any other future, so it shares its
/// thread rather than holding it; what changes with the thread is where the
/// cadence survives. On the main thread the steps compete with rendering and
/// input, and the browser throttles the timers the cadence sleeps on once the
/// page is hidden — seconds, then a minute apart — so a backgrounded device
/// slows to a crawl, which is the right behavior for an app that should idle
/// with its page. A **dedicated Web Worker** is exempt from that throttling and
/// keeps stepping at full rate while the page is hidden but its process lives.
/// Neither survives process-level suspension: macOS App Nap occluding a
/// WKWebView, iOS backgrounding, and Android's cached-app freezer stop the
/// whole renderer, workers included, and riding those out takes native-side
/// measures outside this crate.
///
/// When stepping is render-coupled, drive [`step`](Arora::step) from
/// `requestAnimationFrame` instead — one step per painted frame, no cadence of
/// its own.
impl Arora {
    /// The default inter-step period for [`run`](Arora::run): ~100 Hz.
    pub const DEFAULT_STEP_PERIOD: Duration = Duration::from_millis(10);

    /// Drive `step` to completion at a fixed cadence.
    ///
    /// `run` is `async`: each `step` stays fully synchronous, but between steps
    /// the loop `.await`s the next tick *and* the device's inbound seams in one
    /// select — whichever is ready first. A tick runs the next step; an inbound
    /// arrival is only **buffered** into [`Pending`] (nothing touches the store
    /// outside `step`), so the seams' channels stay drained without extra
    /// tasks, queues, or locks. The select is biased toward the tick: an event
    /// flood cannot starve the cadence.
    ///
    /// The caller brings the executor; `step` itself owns none. Natively that
    /// means a Tokio runtime in scope (the binary drives it from
    /// `#[tokio::main]`; the metronome sleeps on Tokio's timer). On the web it
    /// is whatever polls the future — e.g. `wasm_bindgen_futures::spawn_local`
    /// inside a dedicated worker; the metronome sleeps on a JS timer.
    ///
    /// `period` is the target time between steps — pass
    /// [`DEFAULT_STEP_PERIOD`](Arora::DEFAULT_STEP_PERIOD) for the ~100 Hz
    /// default. A step that overruns the period shifts the next tick out rather
    /// than firing a burst of catch-up ticks. The `dt` handed to `step` is the
    /// **actual** measured time since the previous step, not `period`.
    ///
    /// To **stop** a run, drop its future (e.g. lose a `select!` against a
    /// stop signal): each `step` runs inside a single poll, so cancellation
    /// only ever lands between steps — the device stays consistent, and
    /// `&mut self` is the caller's again to resume, inspect, or drop.
    pub async fn run(&mut self, period: Duration) -> Result<(), RuntimeError> {
        let mut metronome = Metronome::new(period);
        // Wall-clock delta between steps, fed to `step` as the frame `dt`.
        let mut last_step = Instant::now();
        loop {
            // Wait out the period, buffering what the seams deliver meanwhile —
            // in arrival order per seam, applied by the next step. The select
            // polls the tick first (biased); the seams' `next()` futures are
            // fused, so an ended stream's branch is simply never taken again.
            {
                let tick = metronome.tick().fuse();
                futures::pin_mut!(tick);
                loop {
                    futures::select_biased! {
                        _ = tick => break,
                        reading = self.hal_feed.next() => {
                            if let Some(reading) = reading {
                                self.pending.sensors.push(reading);
                            }
                        }
                        event = self.inbound.next() => {
                            if let Some(event) = event {
                                self.pending.events.push(event);
                            }
                        }
                    }
                }
            }
            let now = Instant::now();
            let dt = now.duration_since(last_step);
            last_step = now;
            self.step(dt)?;
        }
    }
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

/// The built-in clock: monotonic nanoseconds on the device's timeline, advanced
/// by each step's `dt`. It starts where the builder set it
/// ([`with_start_time`](crate::AroraBuilder::with_start_time)), zero by default,
/// and only a step moves it afterwards.
#[derive(Default)]
pub struct Clock {
    time_ns: u64,
}

impl Clock {
    /// A clock that reads `start` before its first step. A start past `u64`
    /// nanoseconds (~584 years) saturates, as the clock itself does.
    pub(crate) fn starting_at(start: Duration) -> Self {
        Self {
            time_ns: u64::try_from(start.as_nanos()).unwrap_or(u64::MAX),
        }
    }
}

/// The frame clock values a step publishes into the built-in keys before anything
/// else runs.
pub struct ClockValues {
    /// Monotonic nanoseconds on the device's timeline, after this step's `dt`.
    pub time_ns: u64,
    /// Nanoseconds elapsed since the previous step (this step's `dt`).
    pub dt_ns: u64,
}

/// One inbound event stream, tagged with the bridge endpoint it belongs to so
/// the device can answer each remote on its own terms — `None` for events from
/// an in-process [`Caller`](crate::Caller), which is no remote.
pub type EndpointInbound = futures::stream::BoxStream<'static, (Option<usize>, Inbound)>;

/// Everything the seams delivered since the previous step, in arrival order per
/// seam. The driver only buffers here ([`run`](Arora::run)'s select between
/// ticks, plus the step's own opening sweep); the next `step` applies and
/// drains it. Nothing touches the store outside `step`.
#[derive(Default)]
pub struct Pending {
    /// Sensor readings from the HAL feed.
    pub sensors: Vec<StateChange>,
    /// Inbound events — commands, device-info updates, data-request toggles —
    /// each with the bridge endpoint it arrived on (`None`: in-process).
    pub events: Vec<(Option<usize>, Inbound)>,
}

// =============================================================================
// The step pipeline — free functions over explicit state.
// =============================================================================

/// Phase 0 — sweep the seams: move everything the streams already hold into
/// `pending`, without blocking or waiting. For an embedder that drives `step`
/// directly (web rAF, a preview loop) this is the whole inbound drain; under
/// [`run`](Arora::run) it just picks up what arrived since the select last
/// yielded, so both drivers see identical semantics.
fn sweep_now(
    hal_feed: &mut (impl Stream<Item = StateChange> + Unpin),
    inbound: &mut (impl Stream<Item = (Option<usize>, Inbound)> + Unpin),
    pending: &mut Pending,
) {
    while let Some(Some(reading)) = hal_feed.next().now_or_never() {
        pending.sensors.push(reading);
    }
    while let Some(Some(event)) = inbound.next().now_or_never() {
        pending.events.push(event);
    }
}

/// Phase 1a — advance the built-in clock by `dt` and return this frame's clock
/// values. The monotonic accumulator is exact integer nanoseconds (no float
/// drift over a long run). `dt` is the elapsed time since the previous step,
/// measured (or, for a preview, chosen) by the caller's driver.
fn tick_clock(clock: &mut Clock, dt: Duration) -> ClockValues {
    // A single step's delta is far under u64 nanoseconds; the cast is lossless
    // in practice, and `saturating_add` keeps a pathological run from wrapping.
    let dt_ns = dt.as_nanos() as u64;
    clock.time_ns = clock.time_ns.saturating_add(dt_ns);
    ClockValues {
        time_ns: clock.time_ns,
        dt_ns,
    }
}

/// Phase 1b — publish the frame clock into the built-in keys, before anything
/// else touches the store: the whole frame (sensor applies, command handling,
/// the behavior tick) sees this frame's time. The writes go into the store's
/// change feed like any other, and travel outbound like any other — a remote
/// derives the device's step rate from them.
fn publish_clock(store: &dyn DataStore, clock: &ClockValues) -> Result<(), RuntimeError> {
    let mut change = StateChange::new();
    change
        .set
        .insert(Key::from(built_in::DT), Some(Value::U64(clock.dt_ns)));
    change
        .set
        .insert(Key::from(built_in::TIME), Some(Value::U64(clock.time_ns)));
    store
        .write(change)
        .map_err(|e| RuntimeError::Store(e.to_string()))
}

/// Phase 2 — apply the HAL's sensor readings, oldest first: within the frame,
/// a later reading of the same key wins. Returns the coalesced readings it
/// applied, so phase 6 can keep the hardware's own reports from being written
/// back to it ([`write_hal`]).
fn apply_sensors(
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
/// against the store and replied to on their channel; a claim toggle lands in
/// telemetry.
fn apply_events(
    store: &dyn DataStore,
    function_index: &HashMap<Uuid, ModuleFunction>,
    call_bridge: &mut dyn CallBridge,
    events: Vec<(Option<usize>, Inbound)>,
    data_requested: &mut [bool],
) -> Result<(), RuntimeError> {
    for (endpoint, event) in events {
        match event {
            Inbound::Command(cmd) => apply_command(store, function_index, call_bridge, cmd)?,
            // A remote that no longer knows this device says nothing about the
            // device itself: it keeps running for whoever else it serves.
            Inbound::DeviceInfo(Ok(None)) => {}
            Inbound::DeviceInfo(Ok(Some(_info))) => { /* TODO: apply device info */ }
            Inbound::DeviceInfo(Err(e)) => {
                // A dropped link is not an unregistration: the device keeps
                // running autonomously (the endpoint may reconnect on its own).
                log::warn!("bridge endpoint error: {e}");
            }
            Inbound::DataRequested(requested) => {
                if let Some(asked) = endpoint.and_then(|endpoint| data_requested.get_mut(endpoint))
                {
                    *asked = requested;
                }
            }
        }
    }
    Ok(())
}

/// Handle one command from the remote against the store / function index, then
/// reply on its channel.
// `ListMethods` is deprecated in favour of `DescribeMethods`, but this handler
// still serves it so existing callers keep working.
#[allow(deprecated)]
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
        BridgeOp::Update(change) => {
            // Every bridge's inbound write passes here, so this is where it is
            // checked against the store's meta — once, for all of them, and for
            // the whole change: a change with any refused key writes nothing.
            // A key is closed unless its meta says it is an input, and a key
            // whose meta states a type takes only values of that type, so every
            // reader of a typed key can rely on its type. An unset, or a set to
            // no value, holds no value to check.
            // Each key once, though a change may both set and unset it.
            let keys: Vec<Key> = change
                .set
                .keys()
                .chain(
                    change
                        .unset
                        .iter()
                        .filter(|key| !change.set.contains_key(*key)),
                )
                .cloned()
                .collect();
            let metas = store.meta(&keys);
            let refused: Vec<String> = metas
                .iter()
                .zip(&keys)
                .filter(|(meta, _)| !meta.as_ref().is_some_and(|meta| meta.editable))
                .map(|(_, key)| key.path.clone())
                .collect();
            let mut ill_typed: Vec<String> = metas
                .iter()
                .zip(&keys)
                .filter_map(|(meta, key)| {
                    let expected = meta.as_ref()?.ty.as_ref()?;
                    let value = change.set.get(key)?.as_ref()?;
                    (!value.conforms_to(expected)).then(|| {
                        format!("{} (expected {expected}, got {})", key.path, value.kind())
                    })
                })
                .collect();
            ill_typed.sort();
            if !refused.is_empty() {
                Err(format!(
                    "not an input of this device: {}",
                    refused.join(", ")
                ))
            } else if !ill_typed.is_empty() {
                Err(format!(
                    "not of the key's declared type: {}",
                    ill_typed.join(", ")
                ))
            } else {
                match store.write(change.clone()) {
                    Ok(()) => Ok(CallResult {
                        ret: Value::Unit,
                        mutated: Vec::new(),
                    }),
                    Err(e) => Err(e.to_string()),
                }
            }
        }
        BridgeOp::Call(call) => call_bridge
            .arora_call(call.clone())
            .map_err(|e| format!("call failed: {e:?}")),
        BridgeOp::ListKeys { prefix } => {
            // Introspection: every key the device holds a value for, plus every
            // key the store has meta for — a key can be described before
            // anything writes it. Sorted for a deterministic reply.
            let snapshot = store.snapshot();
            let mut listed: Vec<(Key, Option<Value>)> = snapshot
                .storage
                .into_iter()
                .filter(|(_, value)| value.is_some())
                .collect();
            // A key can be described before anything writes it.
            let held: std::collections::HashSet<Key> =
                listed.iter().map(|(key, _)| key.clone()).collect();
            listed.extend(
                store
                    .all_meta()
                    .into_keys()
                    .filter(|key| !held.contains(key))
                    .map(|key| (key, None)),
            );
            listed.retain(|(key, _)| {
                prefix
                    .as_ref()
                    .is_none_or(|p| key.path.starts_with(p.as_str()))
            });
            let paths: Vec<Key> = listed.iter().map(|(key, _)| key.clone()).collect();
            let mut keys: Vec<(String, KeyMeta)> = listed
                .into_iter()
                .zip(store.meta(&paths))
                .map(|((key, value), meta)| {
                    let mut meta = meta.unwrap_or_default();
                    // The value shows its own shape; the store says the rest.
                    if meta.ty.is_none() {
                        meta.ty = value.as_ref().map(|value| value.kind());
                    }
                    (key.path, meta)
                })
                .collect();
            keys.sort_by(|left, right| left.0.cmp(&right.0));
            match arora_types::value_serde::to_value(&keys) {
                Ok(ret) => Ok(CallResult {
                    ret,
                    mutated: Vec::new(),
                }),
                Err(e) => Err(format!("list_keys: encode failed: {e}")),
            }
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
        BridgeOp::DescribeMethods { prefix } => {
            // Introspection with full signatures: each registered method's
            // parameters (name, type, order) and return type, filtered by name
            // prefix and sorted for a deterministic reply. Encoded as JSON over
            // the value plane (a `Value::String`) so a remote can deserialise the
            // `FrozenTy`s and build a typed call — or, like arora-bridge-ros2,
            // synthesise a typed service from them.
            let mut signatures: Vec<MethodSignature> = function_index
                .values()
                .filter(|f| {
                    prefix
                        .as_ref()
                        .is_none_or(|p| f.function_name.starts_with(p.as_str()))
                })
                .map(|f| MethodSignature {
                    module_id: f.module_id,
                    id: f.function_id,
                    name: f.function_name.clone(),
                    function: f.function.clone(),
                })
                .collect();
            signatures.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
            // Encode over the value plane with Arora's own type system (each
            // signature becomes a `Value::Structure`), not an ad-hoc format.
            match arora_types::value_serde::to_value(&signatures) {
                Ok(ret) => Ok(CallResult {
                    ret,
                    mutated: Vec::new(),
                }),
                Err(e) => Err(format!("describe_methods: encode failed: {e}")),
            }
        }
    };
    cmd.reply(result);
    Ok(())
}

/// The shared cell holding the device's one behavior interpreter: the step
/// loop ticks it (phase 4) and the interpreter module the builder registered
/// on the engine loads/edits it (phase 3). The phases are sequential on one
/// thread, so the cell is uncontended; `RefCell` (not a lock) enforces exactly
/// that. A behavior calling its own interpreter *from inside its tick* finds
/// the cell borrowed and gets a clean error instead of a race.
pub type InterpreterCell = Rc<RefCell<Option<Box<dyn BehaviorInterpreter>>>>;

/// Run `operation` on the interpreter behind `cell` — the body of the
/// interpreter module's functions ([`arora_behavior::interpreter_module`]).
/// A behavior calling its own interpreter *from inside its tick* finds the
/// cell borrowed by phase 4 and gets a clean error instead of aborting; an
/// empty cell (no interpreter installed) errors likewise.
pub(crate) fn with_interpreter(
    cell: &InterpreterCell,
    operation: impl FnOnce(&mut dyn BehaviorInterpreter) -> Result<(), arora_behavior::BehaviorError>,
) -> Result<CallResult, CallError> {
    with_interpreter_value(cell, |interpreter| {
        operation(interpreter).map(|()| Value::Unit)
    })
}

/// Like [`with_interpreter`], but the operation produces the call's return
/// [`Value`] instead of the unit result — for a function whose result carries
/// data back, e.g. SPAWN returning a [`TaskHandle`](arora_behavior::TaskHandle).
pub(crate) fn with_interpreter_value(
    cell: &InterpreterCell,
    operation: impl FnOnce(&mut dyn BehaviorInterpreter) -> Result<Value, arora_behavior::BehaviorError>,
) -> Result<CallResult, CallError> {
    let mut slot = cell.try_borrow_mut().map_err(|_| CallError::Generic {
        message: "the behavior is being ticked; call between steps".to_string(),
    })?;
    let interpreter = slot.as_mut().ok_or_else(|| CallError::Generic {
        message: "no behavior interpreter is installed".to_string(),
    })?;
    let ret = operation(interpreter.as_mut()).map_err(|e| CallError::Guest {
        message: e.to_string(),
    })?;
    Ok(CallResult {
        ret,
        mutated: Vec::new(),
    })
}

/// Phase 4 — tick the one behavior interpreter (a tree, a node graph, …)
/// against the shared store, **last**: its writes win the frame, and it ticks
/// over everything the frame already applied (clock, sensors, remote updates).
/// A no-op when none is installed. When the interpreter reports
/// [`BehaviorStatus::Done`] — `Ok(())` (clean) or `Err(_)` (a terminal fault) —
/// it is dropped (back to `None`); while it is [`BehaviorStatus::Running`] it
/// stays for the next step.
///
/// A *transient* failing tick (an `Err`) does not stop the device: the behavior
/// stays installed and ticks again next step, and the rest of the pipeline — HAL
/// writes included — goes on. A terminal `Done(Err(_))` fault drops it instead:
/// dropped, not retried. Either way the reason is sent once per distinct message
/// on the standing error watch (received through [`Arora::behavior_error`]) and
/// logged; the next successful tick clears it.
fn tick_behavior(
    interpreter: &mut Option<Box<dyn BehaviorInterpreter>>,
    store: &dyn DataStore,
    engine: &mut arora_engine::engine::PinnedEngine,
    standing_error: &watch::Sender<Option<String>>,
) {
    let Some(behavior) = interpreter.as_mut() else {
        return;
    };
    let mut ctx = BehaviorContext {
        store,
        call_bridge: engine,
    };
    // Raise a fault on the standing watch, deduplicated by message; `retried`
    // only changes the log line (the interpreter's fate is decided by the caller).
    let raise = |message: String, retried: bool| {
        if standing_error.borrow().as_deref() != Some(message.as_str()) {
            if retried {
                log::warn!("the behavior failed and will be retried: {message}");
            } else {
                log::warn!("the behavior errored and was dropped: {message}");
            }
            standing_error.send_replace(Some(message));
        }
    };
    let clear = || {
        if standing_error.borrow().is_some() {
            standing_error.send_replace(None);
        }
    };
    match behavior.tick(&mut ctx) {
        // Still running: this tick was clean, so clear any standing fault.
        Ok(BehaviorStatus::Running) => clear(),
        // Terminal, clean: clear the fault and drop the interpreter.
        Ok(BehaviorStatus::Done(Ok(()))) => {
            clear();
            *interpreter = None;
        }
        // Terminal, faulted: surface the reason and drop the interpreter — a
        // terminal error is not retried (unlike the transient `Err` below).
        Ok(BehaviorStatus::Done(Err(error))) => {
            raise(error.to_string(), false);
            *interpreter = None;
        }
        // Transient failure: keep the interpreter installed and retry next step.
        Err(error) => raise(error.to_string(), true),
    }
}

/// Phase 5 — coalesce everything drained from the store's change feed this step
/// into ONE [`StateChange`], so the remote/hardware see a single, consistent
/// update per step. Changes are drained in order, so later ones win: a set
/// overrides an earlier unset of the same key (and vice versa).
fn flush(changes: &Subscription) -> StateChange {
    let mut merged = StateChange::new();
    while let Some(change) = changes.try_recv() {
        for (key, value) in change.set {
            merged.unset.remove(&key);
            merged.set.insert(key, value);
        }
        for key in change.unset {
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
///
/// The HAL is lent `out` itself when no reading is a frame-final value — always
/// the case for a HAL that reports nothing — so such a frame copies nothing;
/// only a frame with keys to leave out builds the HAL a change of its own.
/// Telling the two apart costs a lookup per reading, not per outbound key.
fn write_hal(hal: &dyn Hal, out: &StateChange, sensor_applied: &StateChange) {
    let reported_back = sensor_applied
        .set
        .iter()
        .any(|(key, value)| out.set.get(key) == Some(value))
        || sensor_applied
            .unset
            .iter()
            .any(|key| out.unset.contains(key));
    if !reported_back {
        hal.try_send(out);
        return;
    }
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
fn write_bridges(bridges: &mut [Box<dyn Bridge>], asked: &[bool], out: &StateChange) {
    for (endpoint, bridge) in bridges.iter_mut().enumerate() {
        if asked.get(endpoint).copied().unwrap_or(false) {
            bridge.try_send(out);
        }
    }
}

// =============================================================================
// `Arora` — the object that holds the state and wires the pipeline.
// =============================================================================

impl Arora {
    /// Advance one step, applying everything the seams delivered since the
    /// previous one. Non-blocking; touches the state from this (single) thread
    /// only.
    ///
    /// Writers apply in a fixed order within the step: the clock first — under
    /// the built-in keys, so the whole frame reads this frame's time — then the
    /// HAL's readings, then the bridges' events (commands dispatch and reply
    /// here), then the behavior. Per-key precedence is therefore total:
    /// **behavior ▸ bridge ▸ HAL ▸ previous frame**, and within each, arrival
    /// order — the newest write wins. What changed is coalesced into a single
    /// outbound change and fanned out to the HAL and every bridge.
    ///
    /// `dt` is the elapsed time since the previous step, measured by the
    /// caller's driver ([`run`](Arora::run) natively, `requestAnimationFrame` on
    /// the web) — or chosen freely by a driver with its own idea of time, e.g. a
    /// faster-than-realtime preview stepping a fixed virtual `dt`. It advances
    /// the monotonic clock, published (with the accumulated time) under the
    /// built-in keys before anything else runs, so behaviors read timing from the
    /// store rather than as a tick argument.
    pub fn step(&mut self, dt: Duration) -> Result<(), RuntimeError> {
        // 0. sweep — pick up everything the seams hold right now.
        sweep_now(&mut self.hal_feed, &mut self.inbound, &mut self.pending);
        // 1. time — built-in keys first: the whole frame sees this clock.
        let clock = tick_clock(&mut self.clock, dt);
        publish_clock(&*self.store, &clock)?;
        // 2. HAL readings — oldest first; per key, the newest wins.
        let sensor_applied =
            apply_sensors(&*self.store, std::mem::take(&mut self.pending.sensors))?;
        // 3. bridge readings — after the HAL: a remote update beats this
        //    frame's sensor value. Commands dispatch and reply here.
        apply_events(
            &*self.store,
            &self.function_index,
            &mut self.engine,
            std::mem::take(&mut self.pending.events),
            &mut self.data_requested,
        )?;
        // 4. behavior — the frame's last writer: its intent wins, and it saw
        //    what it overrode. The cell borrow spans exactly this phase; a
        //    tick-time built-in edit through the engine finds it held and fails
        //    cleanly rather than racing the tick.
        tick_behavior(
            &mut self.interpreter.borrow_mut(),
            &*self.store,
            &mut self.engine,
            &self.behavior_error,
        );
        // 5. flush — everything this frame changed, as one change.
        let out = flush(&self.store_changes);
        // 6. writings — the hardware first (its own readings subtracted), then
        //    every remote.
        if !out.is_empty() {
            write_hal(&*self.hal, &out, &sensor_applied);
            write_bridges(&mut self.bridges, &self.data_requested, &out);
        }
        Ok(())
    }
}

/// A cadence: one [`tick`](Metronome::tick) completes per `period`, the first
/// one immediately. Ticks are anchored to when they were due rather than when
/// they completed, so a slow caller neither drifts nor gets a burst of
/// catch-up ticks.
///
/// Dropping a `tick()` before it completes leaves the cadence untouched: the
/// next one completes at the instant the dropped one was waiting for.
struct Metronome {
    period: Duration,
    /// When the next tick is due; `None` until the first (immediate) tick.
    next_due: Option<Instant>,
}

impl Metronome {
    fn new(period: Duration) -> Self {
        Self {
            period,
            next_due: None,
        }
    }

    /// Complete when the next tick is due.
    async fn tick(&mut self) {
        let now = Instant::now();
        match self.next_due {
            // First tick: immediate.
            None => self.next_due = Some(now + self.period),
            // On schedule: sleep out the remainder, keep the cadence anchored
            // to the previous due time (no cumulative drift).
            Some(due) if now < due => {
                let duration = due - now;
                // WASM targets inherit their time-based functions from their runtime.
                #[cfg(target_arch = "wasm32")]
                gloo_timers::future::sleep(duration).await;
                #[cfg(not(target_arch = "wasm32"))]
                tokio::time::sleep(duration).await;
                self.next_due = Some(due + self.period);
            }
            // Overrun: the due tick fires now; the next is a full period out.
            // Firing "now" still suspends once — through the same timer the
            // on-schedule arm sleeps on, so control reaches the browser's
            // macrotask queue (timers, rendering) and not just the executor's
            // microtask loop. Without it, a device whose steps always overrun
            // the period would never yield its thread — on the web, freezing
            // the whole page.
            Some(_) => {
                self.next_due = Some(now + self.period);
                #[cfg(target_arch = "wasm32")]
                gloo_timers::future::sleep(Duration::ZERO).await;
                #[cfg(not(target_arch = "wasm32"))]
                tokio::task::yield_now().await;
            }
        }
    }
}

/// The cadence guarantees hold identically on every target, so these run both
/// natively and in a browser (`wasm-pack test --headless --firefox
/// crates/arora --no-default-features --lib`). They assert against real
/// elapsed time with margins wide enough for browser timer granularity.
#[cfg(test)]
mod metronome_tests {
    use super::*;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test;

    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    const PERIOD: Duration = Duration::from_millis(60);

    async fn sleep(duration: Duration) {
        // WASM targets inherit their time-based functions from their runtime.
        #[cfg(target_arch = "wasm32")]
        gloo_timers::future::sleep(duration).await;
        #[cfg(not(target_arch = "wasm32"))]
        tokio::time::sleep(duration).await;
    }

    /// An overrun tick fires now but still suspends: a step loop that always
    /// overruns its period keeps sharing its thread instead of freezing it.
    #[cfg_attr(not(target_arch = "wasm32"), tokio::test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    async fn an_overrun_tick_still_suspends() {
        let mut metronome = Metronome::new(Duration::ZERO);
        metronome.tick().await;
        let tick = metronome.tick();
        futures::pin_mut!(tick);
        assert!(
            futures::poll!(tick.as_mut()).is_pending(),
            "the overrun arm must yield before completing"
        );
        tick.await;
    }

    #[cfg_attr(not(target_arch = "wasm32"), tokio::test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    async fn the_first_tick_completes_immediately() {
        let mut metronome = Metronome::new(PERIOD);
        let start = Instant::now();
        metronome.tick().await;
        assert!(
            start.elapsed() < PERIOD / 2,
            "first tick waited {:?}",
            start.elapsed()
        );
    }

    /// Four ticks with half a period of work between them land at ~3.5 periods
    /// when anchored to when the ticks were due; restarting the wait after each
    /// delay would take ~5.
    #[cfg_attr(not(target_arch = "wasm32"), tokio::test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    async fn a_slow_caller_does_not_drift() {
        let mut metronome = Metronome::new(PERIOD);
        let start = Instant::now();
        for _ in 0..4 {
            metronome.tick().await;
            sleep(PERIOD / 2).await;
        }
        assert!(
            start.elapsed() < PERIOD * 17 / 4,
            "four ticks took {:?}",
            start.elapsed()
        );
    }

    #[cfg_attr(not(target_arch = "wasm32"), tokio::test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    async fn an_overrun_delays_the_cadence_instead_of_bursting() {
        let mut metronome = Metronome::new(PERIOD);
        metronome.tick().await;
        sleep(PERIOD * 5 / 2).await;

        let overrun_end = Instant::now();
        metronome.tick().await;
        assert!(
            overrun_end.elapsed() < PERIOD / 2,
            "the due tick waited {:?}",
            overrun_end.elapsed()
        );

        let late_tick = Instant::now();
        metronome.tick().await;
        assert!(
            late_tick.elapsed() > PERIOD * 3 / 4,
            "the missed periods fired as a burst, {:?} apart",
            late_tick.elapsed()
        );
    }

    #[cfg_attr(not(target_arch = "wasm32"), tokio::test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    async fn a_dropped_tick_keeps_its_deadline() {
        let mut metronome = Metronome::new(PERIOD);
        metronome.tick().await;

        let start = Instant::now();
        {
            let tick = metronome.tick().fuse();
            futures::pin_mut!(tick);
            let abandon = sleep(PERIOD * 9 / 10).fuse();
            futures::pin_mut!(abandon);
            futures::select_biased! {
                _ = abandon => {}
                _ = tick => panic!("the tick completed before its deadline"),
            }
        }
        metronome.tick().await;
        assert!(
            start.elapsed() < PERIOD * 3 / 2,
            "the tick restarted its wait instead of keeping its deadline, {:?}",
            start.elapsed()
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use arora_behavior::interpreter_module;
    use arora_behavior_tree::behavior::BehaviorTreeInterpreter;
    use arora_bridge::{BridgeResult, DeviceInfo, FakeBridge, InboundStream};
    use arora_hal::FakeHal;
    use arora_simple_data_store::{NamespacedStore, SimpleDataStore};
    use arora_types::value::Type;
    use async_trait::async_trait;
    use futures::channel::{mpsc, oneshot};
    use futures::stream;
    use std::rc::Rc;
    use std::sync::Arc;

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
    /// Groot tree loaded into it — the construct-empty → load → inject flow,
    /// ready to hand to [`build_in_with`]. The tree binds to the device's store
    /// at its first tick (the scaffold lowers against the tick's context).
    fn groot_interpreter(xml: &str) -> Box<dyn BehaviorInterpreter> {
        let mut interpreter = BehaviorTreeInterpreter::new(Rc::new(HashMap::new()));
        interpreter.load_groot(xml).expect("tree loads");
        Box::new(interpreter)
    }

    #[test]
    fn builder_defaults_to_a_self_contained_device() {
        // No seams named: fake HAL, no bridge (a standalone device is legal —
        // e.g. a preview), a private store, and the default executor — an
        // empty, idle behavior-tree interpreter.
        let arora = Arora::builder().build().expect("default device builds");
        assert!(
            arora.interpreter.borrow().is_some(),
            "default installs an (empty) behavior interpreter"
        );
        assert!(arora.bridges.is_empty(), "no bridge unless one is added");
    }

    #[test]
    fn a_bridgeless_device_steps() {
        // The zero-bridge device (a preview, a bench test) steps and stays live.
        let mut arora = Arora::builder().build().expect("builds");
        for _ in 0..5 {
            arora.step(FRAME).expect("step");
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
            arora.interpreter.borrow().is_some(),
            "the empty interpreter idles and stays installed"
        );
    }

    /// One remote forgetting the device says nothing about the device: it
    /// serves whoever else it is attached to, so it keeps stepping.
    #[test]
    fn an_unregistering_remote_does_not_stop_the_device() {
        let mut arora = build(Box::new(UnregisterBridge));
        for _ in 0..5 {
            arora.step(FRAME).expect("step");
        }
    }

    /// `run` outlives the remote that forgot the device: nothing a bridge
    /// reports ends the loop, so the timeout — not the loop — is what returns.
    #[tokio::test]
    async fn run_outlives_an_unregistering_remote() {
        let mut arora = build(Box::new(UnregisterBridge));
        tokio::time::timeout(
            Duration::from_millis(100),
            arora.run(Duration::from_millis(1)),
        )
        .await
        .expect_err("run keeps pacing the device");
    }

    /// A behavior that counts its ticks through a shared counter and stays
    /// `Running`, so a paced run keeps stepping it — one count per step.
    struct CountTicks(Arc<std::sync::atomic::AtomicU32>);

    impl BehaviorInterpreter for CountTicks {
        fn tick(
            &mut self,
            _ctx: &mut BehaviorContext,
        ) -> Result<BehaviorStatus, arora_behavior::BehaviorError> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(BehaviorStatus::Running)
        }
    }

    /// Stopping a healthy run is dropping its future: after losing a select,
    /// the device is intact — steps made before the drop are visible, and the
    /// caller can keep using it.
    #[tokio::test]
    async fn dropping_the_run_future_stops_the_device_cleanly() {
        let ticks = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let mut arora = build_with(
            Box::new(FakeBridge::new()),
            Box::new(CountTicks(ticks.clone())),
        );
        tokio::select! {
            result = arora.run(Duration::from_millis(1)) => {
                panic!("a healthy run does not return: {result:?}")
            }
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
        let stepped = ticks.load(std::sync::atomic::Ordering::SeqCst);
        assert!(stepped > 0, "the device ran until the drop");
        arora.step(Duration::from_millis(1)).expect("still usable");
    }

    /// `run` paces the step at the requested period: over a fixed wall-clock
    /// window the device makes roughly window/period steps — enough that the
    /// loop is really stepping, and no runaway burst (the metronome delays
    /// after an overrun instead of catching up).
    #[tokio::test]
    async fn run_paces_steps_at_the_period() {
        let ticks = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let mut arora = build_with(
            Box::new(FakeBridge::new()),
            Box::new(CountTicks(ticks.clone())),
        );
        // 200 ms at a 10 ms period targets ~20 steps; the bounds leave generous
        // slack for a loaded machine while still catching an unpaced spin (which
        // would run thousands) or a stalled loop.
        let _ = tokio::time::timeout(
            Duration::from_millis(200),
            arora.run(Duration::from_millis(10)),
        )
        .await;
        let stepped = ticks.load(std::sync::atomic::Ordering::SeqCst);
        assert!(
            (5..=60).contains(&stepped),
            "expected ~20 paced steps, got {stepped}"
        );
    }

    #[tokio::test]
    async fn a_call_edits_the_behavior_through_the_engine() {
        // The builder registered the interpreter module over the injected
        // (default, empty) interpreter; a bridge Call to its EDIT id reaches
        // `interpreter.apply` through the engine's normal dispatch. An empty
        // diff is a valid no-op edit, so the call succeeds.
        let mut arora = build(Box::new(UnregisterBridge));
        let (tx, rx) = oneshot::channel();
        apply_command(
            &*arora.store,
            &arora.function_index,
            &mut arora.engine,
            BridgeCommand::new(
                BridgeOp::Call(interpreter_module::encode_edit(
                    &arora_behavior::GraphDiff::default(),
                )),
                tx,
            ),
        )
        .unwrap();
        let result = rx.await.expect("reply").expect("the edit call succeeds");
        assert_eq!(result.ret, Value::Unit);
    }

    #[tokio::test]
    async fn a_call_loads_a_behavior_through_the_engine() {
        // The interpreter module's LOAD function replaces the running behavior
        // with a whole graph — here an empty one, which the tree interpreter
        // accepts and idles on.
        let mut arora = build(Box::new(UnregisterBridge));
        let (tx, rx) = oneshot::channel();
        apply_command(
            &*arora.store,
            &arora.function_index,
            &mut arora.engine,
            BridgeCommand::new(
                BridgeOp::Call(interpreter_module::encode_load(
                    &arora_behavior::Graph::empty(),
                )),
                tx,
            ),
        )
        .unwrap();
        let result = rx.await.expect("reply").expect("the load call succeeds");
        assert_eq!(result.ret, Value::Unit);
    }

    #[tokio::test]
    async fn calls_reach_spawn_and_halt_through_the_engine() {
        use arora_types::call::Call;
        use arora_types::Uuid;
        // The interpreter module's SPAWN/HALT functions dispatch to the
        // interpreter's spawn/halt through the engine. The default tree
        // interpreter hosts task runs, so SPAWN returns a TaskHandle and HALT
        // (via that handle's stop call) succeeds — proving the wiring reaches
        // those methods. Spawn only registers the run; it is not ticked here, so
        // the placeholder call target is never dispatched.
        let mut arora = build(Box::new(UnregisterBridge));

        let (tx, rx) = oneshot::channel();
        let spawn = interpreter_module::encode_spawn(
            &Call {
                module_id: Some(Uuid::from_u128(0xB0)),
                id: Uuid::from_u128(1),
                args: vec![],
            },
            arora_behavior::RunPolicy::Concurrent,
        );
        apply_command(
            &*arora.store,
            &arora.function_index,
            &mut arora.engine,
            BridgeCommand::new(BridgeOp::Call(spawn), tx),
        )
        .unwrap();
        let result = rx
            .await
            .expect("reply")
            .expect("the default tree interpreter accepts spawn");
        let handle = interpreter_module::decode_spawn_result(&result.ret)
            .expect("SPAWN returns a TaskHandle");
        assert!(
            handle.status.path.starts_with("arora/tasks/"),
            "{}",
            handle.status.path
        );

        // Halt the run just spawned, through its own stop call.
        let (tx, rx) = oneshot::channel();
        apply_command(
            &*arora.store,
            &arora.function_index,
            &mut arora.engine,
            BridgeCommand::new(BridgeOp::Call(handle.stop.clone()), tx),
        )
        .unwrap();
        let halted = rx.await.expect("reply").expect("halt succeeds");
        assert_eq!(halted.ret, Value::Unit);
    }

    /// A close-to-real-world task run driven through the whole device: a
    /// `look_at` host function — the LookAt action-behavior in miniature — is
    /// spawned as a run, and stepping the device advances it. Each step it drives
    /// its gaze actuation key and publishes `Running` to its status key (LookAt
    /// tracks indefinitely, never succeeding on its own), until a halt ends it and
    /// the next step publishes the terminal status. This is the engine-side (A)
    /// half of a ROS 2 action, exercised end to end without ROS.
    #[tokio::test]
    async fn a_look_at_run_advances_and_halts_through_the_device() {
        use arora_behavior_tree::arora_generated::behavior_tree::status::Status;
        use arora_types::call::Call;
        use arora_types::Uuid;

        // The store the look_at function and the test share (a SimpleDataStore
        // clone shares its storage), so the run's writes are observable here.
        let store = SimpleDataStore::new();

        let module_id = Uuid::from_u128(0x704A);
        let look_at = Uuid::from_u128(0x6A2E);
        let module = crate::ModuleBuilder::new(module_id)
            .function(look_at, {
                let store = store.clone();
                move |_call| {
                    // Drive the gaze toward the target and keep tracking.
                    store
                        .write(StateChange::set("gaze/target", Value::F64(1.0)))
                        .map_err(|e| arora_types::call::CallError::Generic {
                            message: e.to_string(),
                        })?;
                    let running: Value = Status::Running.into();
                    Ok(arora_types::call::CallResult {
                        ret: running,
                        mutated: Vec::new(),
                    })
                }
            })
            .build();

        let mut arora = Arora::builder()
            .with_hal(Box::new(FakeHal::new()))
            .with_bridge(Box::new(UnregisterBridge))
            .with_data_store(Box::new(store.clone()))
            .with_host_module(module)
            .build()
            .expect("arora builds");

        // Spawn the look_at run through the interpreter module (as a bridge does
        // for a SendGoal).
        let (tx, rx) = oneshot::channel();
        let spawn = interpreter_module::encode_spawn(
            &Call {
                module_id: Some(module_id),
                id: look_at,
                args: vec![],
            },
            arora_behavior::RunPolicy::Concurrent,
        );
        apply_command(
            &*arora.store,
            &arora.function_index,
            &mut arora.engine,
            BridgeCommand::new(BridgeOp::Call(spawn), tx),
        )
        .unwrap();
        let handle = interpreter_module::decode_spawn_result(
            &rx.await.expect("reply").expect("spawn ok").ret,
        )
        .expect("a TaskHandle");

        // Before stepping, spawn has only registered the run: nothing written.
        assert_eq!(
            store.read(std::slice::from_ref(&Key::from("gaze/target"))),
            vec![None]
        );

        // One step advances the run: it drives gaze and publishes Running.
        arora.step(FRAME).expect("step");
        assert_eq!(
            store.read(std::slice::from_ref(&Key::from("gaze/target"))),
            vec![Some(Value::F64(1.0))],
            "the run drove its actuation key"
        );
        let running: Value = Status::Running.into();
        assert_eq!(
            store.read(std::slice::from_ref(&handle.status)),
            vec![Some(running)]
        );

        // It tracks indefinitely: another step keeps it Running.
        arora.step(FRAME).expect("step");
        let running: Value = Status::Running.into();
        assert_eq!(
            store.read(std::slice::from_ref(&handle.status)),
            vec![Some(running)]
        );

        // Halt (the cancel path): the stop call ends the run, and the next step
        // publishes the terminal status.
        let (tx, rx) = oneshot::channel();
        apply_command(
            &*arora.store,
            &arora.function_index,
            &mut arora.engine,
            BridgeCommand::new(BridgeOp::Call(handle.stop.clone()), tx),
        )
        .unwrap();
        rx.await.expect("reply").expect("halt ok");
        arora.step(FRAME).expect("step");
        let failure: Value = Status::Failure.into();
        assert_eq!(
            store.read(std::slice::from_ref(&handle.status)),
            vec![Some(failure)],
            "a halted run ends terminal"
        );
    }

    #[test]
    fn a_tick_time_call_fails_cleanly() {
        // A behavior calling its own interpreter from inside its tick would
        // find the cell borrowed by phase 4: the module function refuses with
        // an error instead of aborting the process.
        let interpreter: InterpreterCell = Rc::new(RefCell::new(Some(Box::new(
            BehaviorTreeInterpreter::new(Rc::new(HashMap::new())),
        )
            as Box<dyn BehaviorInterpreter>)));
        let _phase_4_holds_it = interpreter.borrow_mut();
        let err = with_interpreter(&interpreter, |interpreter| {
            interpreter.apply(arora_behavior::GraphDiff::default())
        })
        .expect_err("a tick-time call is refused");
        assert!(err.to_string().contains("being ticked"), "{err}");
    }

    #[test]
    fn a_call_without_an_interpreter_errors() {
        let empty: InterpreterCell = Rc::new(RefCell::new(None));
        let err = with_interpreter(&empty, |interpreter| {
            interpreter.apply(arora_behavior::GraphDiff::default())
        })
        .expect_err("no interpreter to call");
        assert!(err.to_string().contains("no behavior interpreter"), "{err}");
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
        // device resolve against one data storage.
        let store = SimpleDataStore::new();
        let interpreter = groot_interpreter(xml);
        let mut arora = build_in_with(
            Box::new(UnregisterBridge),
            Box::new(store.clone()),
            interpreter,
        );
        for _ in 0..5 {
            arora.step(FRAME).expect("step");
        }
    }

    #[tokio::test]
    async fn get_and_update_commands_round_trip() {
        let mut arora = build(Box::new(UnregisterBridge));
        let key = Key::from("greeting");
        arora
            .store
            .set_meta(HashMap::from([(key.clone(), KeyMeta::new().editable())]))
            .unwrap();

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
            Ok(BehaviorStatus::Done(Ok(())))
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

    /// A remote write reaches only the keys the device opened: a key nothing
    /// described is refused on the way in, naming it, for every bridge at once;
    /// a key its meta makes an input — its own, or a subtree's — is accepted.
    #[tokio::test]
    async fn a_remote_write_reaches_only_the_devices_inputs() {
        let mut arora = build(Box::new(UnregisterBridge));
        let write = |path: &str| {
            let mut set = HashMap::new();
            set.insert(Key::from(path), Some(Value::F32(1.0)));
            BridgeOp::Update(StateChange {
                set,
                unset: std::collections::HashSet::new(),
            })
        };
        let mut apply = |op| {
            let (tx, rx) = oneshot::channel();
            apply_command(
                &*arora.store,
                &arora.function_index,
                &mut arora.engine,
                BridgeCommand::new(op, tx),
            )
            .unwrap();
            rx
        };

        let error = apply(write("face/mouth"))
            .await
            .unwrap()
            .expect_err("closed until the device says otherwise");
        assert!(error.contains("face/mouth"), "{error}");
        assert_eq!(
            arora.store.read(&[Key::from("face/mouth")])[0],
            None,
            "nothing was written"
        );

        arora
            .store
            .set_prefix_meta(HashMap::from([(
                "face".to_string(),
                KeyMeta::new().editable(),
            )]))
            .expect("the store keeps meta");
        assert!(apply(write("face/mouth")).await.unwrap().is_ok(), "opened");
        assert!(
            apply(write("arora/time")).await.unwrap().is_err(),
            "outside the opened subtree"
        );
    }

    /// A remote write to a typed key takes only values of that type: any other
    /// is refused with the key, the type it states and the type it was sent,
    /// and the change writes nothing — not even its conforming keys. A value of
    /// the key's type, an unset, a set to no value, and any value for a key
    /// whose meta states no type are accepted. A closed key is refused as one,
    /// whatever its value.
    #[tokio::test]
    async fn a_remote_write_takes_only_the_declared_type() {
        let mut arora = build(Box::new(UnregisterBridge));
        let input = |ty: Type| KeyMeta::new().editable().of_type(ty);
        arora
            .store
            .set_meta(HashMap::from([
                (Key::from("face/mouth"), input(Type::F64)),
                (Key::from("face/label"), input(Type::String)),
                (Key::from("face/lids"), input(Type::ArrayF64)),
                (Key::from("face/mood"), input(Type::Option)),
                (Key::from("face/pose"), input(Type::Structure)),
                (Key::from("face/free"), KeyMeta::new().editable()),
                (Key::from("face/time"), KeyMeta::new().of_type(Type::F64)),
            ]))
            .expect("the store keeps meta");
        let set = |pairs: Vec<(&str, Option<Value>)>| {
            BridgeOp::Update(StateChange {
                set: pairs
                    .into_iter()
                    .map(|(path, value)| (Key::from(path), value))
                    .collect(),
                unset: std::collections::HashSet::new(),
            })
        };
        let mut apply = |op| {
            let (tx, mut rx) = oneshot::channel();
            apply_command(
                &*arora.store,
                &arora.function_index,
                &mut arora.engine,
                BridgeCommand::new(op, tx),
            )
            .unwrap();
            rx.try_recv()
                .expect("replied at once")
                .expect("a reply")
                .map(|_| ())
        };
        let f64 = |v: f64| Some(Value::F64(v));

        // Each kind of mismatch is refused, naming the key and both types.
        let mismatches = [
            ("face/mouth", Value::String("0.5".into()), "F64", "String"),
            ("face/mouth", Value::F32(0.5), "F64", "F32"),
            (
                "face/mouth",
                Value::Option(Some(Box::new(Value::F64(0.5)))),
                "F64",
                "Option",
            ),
            (
                "face/lids",
                Value::ArrayValue(vec![Value::F64(0.5)]),
                "ArrayF64",
                "ArrayValue",
            ),
            (
                "face/lids",
                Value::ArrayF32(vec![0.5]),
                "ArrayF64",
                "ArrayF32",
            ),
            (
                "face/mood",
                Value::String("calm".into()),
                "Option",
                "String",
            ),
            (
                "face/pose",
                Value::Enumeration(arora_types::value::Enumeration {
                    id: Uuid::from_u128(1),
                    variant_id: Uuid::from_u128(2),
                    value: Box::new(Value::Unit),
                }),
                "Structure",
                "Enumeration",
            ),
        ];
        for (path, value, expected, got) in mismatches {
            let error = apply(set(vec![(path, Some(value.clone()))]))
                .expect_err(&format!("{value:?} into {path}"));
            assert!(
                error.contains(&format!("{path} (expected {expected}, got {got})")),
                "{error}"
            );
        }

        // One ill-typed key refuses the whole change.
        let error = apply(set(vec![
            ("face/mouth", f64(0.5)),
            ("face/label", Some(Value::Boolean(true))),
        ]))
        .expect_err("one key is ill-typed");
        assert!(error.contains("face/label"), "{error}");
        assert!(!error.contains("face/mouth"), "{error}");
        assert_eq!(arora.store.read(&[Key::from("face/mouth")])[0], None);

        // A key both set and unset is named once.
        let error = apply(BridgeOp::Update(StateChange {
            set: HashMap::from([(Key::from("face/mouth"), Some(Value::Boolean(true)))]),
            unset: std::collections::HashSet::from([Key::from("face/mouth")]),
        }))
        .expect_err("ill-typed");
        assert_eq!(
            error,
            "not of the key's declared type: face/mouth (expected F64, got Boolean)"
        );

        // A closed key is refused as closed, whatever its value.
        let error =
            apply(set(vec![("face/time", Some(Value::Boolean(true)))])).expect_err("closed");
        assert!(error.contains("not an input"), "{error}");

        // Values of the key's type are written, compound ones whatever they hold.
        apply(set(vec![
            ("face/mouth", f64(0.5)),
            ("face/label", Some(Value::String("hi".into()))),
            ("face/lids", Some(Value::ArrayF64(vec![0.1, 0.2]))),
            (
                "face/mood",
                Some(Value::Option(Some(Box::new(Value::String("calm".into()))))),
            ),
            (
                "face/pose",
                Some(Value::Structure(arora_types::value::Structure {
                    id: Uuid::from_u128(3),
                    fields: vec![],
                })),
            ),
            ("face/free", Some(Value::Boolean(true))),
        ]))
        .expect("conforming values are written");
        assert_eq!(arora.store.read(&[Key::from("face/mouth")])[0], f64(0.5));

        // An unset, or a set to no value, holds nothing to check.
        apply(set(vec![("face/label", None)])).expect("a set to no value");
        apply(BridgeOp::Update(StateChange {
            set: HashMap::new(),
            unset: std::collections::HashSet::from([Key::from("face/mouth")]),
        }))
        .expect("an unset");
        assert_eq!(arora.store.read(&[Key::from("face/mouth")])[0], None);
    }

    /// A bridge whose inbound stream delivers one command, then stays open.
    struct OneCommand(Option<BridgeCommand>);

    #[async_trait]
    impl Bridge for OneCommand {
        fn take_inbound(&mut self) -> InboundStream {
            let command = self.0.take().expect("the inbound is taken once");
            Box::pin(stream::once(async { Inbound::Command(command) }).chain(stream::pending()))
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

    /// What a remote receives for an ill-typed write: the reply its bridge
    /// relays, an error naming the key and the type it states, answered by the
    /// step that applies the command.
    #[test]
    fn a_remote_is_answered_with_the_type_its_write_missed() {
        let store = SimpleDataStore::new();
        store
            .set_meta(HashMap::from([(
                Key::from("face/mouth"),
                KeyMeta::new().editable().of_type(Type::F64),
            )]))
            .expect("the store keeps meta");
        let (tx, mut rx) = oneshot::channel();
        let command = BridgeCommand::new(
            BridgeOp::Update(StateChange::set("face/mouth", Value::String("wide".into()))),
            tx,
        );
        let mut arora = build_in(Box::new(OneCommand(Some(command))), Box::new(store.clone()));

        arora.step(FRAME).expect("step");

        let error = rx
            .try_recv()
            .expect("answered by the step")
            .expect("a reply")
            .expect_err("refused");
        assert_eq!(
            error,
            "not of the key's declared type: face/mouth (expected F64, got String)"
        );
        assert_eq!(store.read(&[Key::from("face/mouth")])[0], None);
    }

    // Exercises `ListMethods` (deprecated) alongside `ListKeys`/`DescribeMethods`.
    #[allow(deprecated)]
    #[tokio::test]
    async fn list_keys_enumerates_the_store_by_prefix() {
        let mut arora = build(Box::new(UnregisterBridge));

        // Seed three keys across two prefixes — the device's own writes.
        let mut set = HashMap::new();
        set.insert(Key::from("face/mouth"), Some(Value::F32(0.5)));
        set.insert(Key::from("face/eyes"), Some(Value::F32(0.1)));
        set.insert(Key::from("body/hand"), Some(Value::F32(0.9)));
        arora
            .store
            .write(StateChange {
                set,
                unset: std::collections::HashSet::new(),
            })
            .unwrap();
        // One of them is described: its unit travels with the rest of its meta.
        arora
            .store
            .set_meta(HashMap::from([(
                Key::from("face/mouth"),
                KeyMeta::new().in_unit("fraction"),
            )]))
            .expect("the store keeps meta");

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
        let keys: Vec<(String, KeyMeta)> =
            arora_types::value_serde::from_value(result.ret).expect("the listed keys");
        assert_eq!(
            keys.iter()
                .map(|(path, _)| path.as_str())
                .collect::<Vec<_>>(),
            ["face/eyes", "face/mouth"]
        );
        // Each key carries the shape of the value it holds and stays closed to
        // remote writers: a description that says nothing about either changes
        // neither.
        for (path, meta) in &keys {
            assert_eq!(meta.ty, Some(arora_types::value::Type::F32), "{path}");
            assert!(!meta.editable, "{path}");
            assert_eq!(meta.min, None);
        }
        // The described key carries its unit; the other has none to carry.
        let unit = |path: &str| {
            keys.iter()
                .find(|(listed, _)| listed == path)
                .map(|(_, meta)| meta.unit.clone())
                .unwrap_or_else(|| panic!("{path} is listed"))
        };
        assert_eq!(unit("face/mouth").as_deref(), Some("fraction"));
        assert_eq!(unit("face/eyes"), None);

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

        // DescribeMethods returns the same set with full signatures, encoded as
        // JSON over the value plane — a `Value::String` that deserialises to a
        // `Vec<MethodSignature>`: the HAL module's and the interpreter module's
        // functions.
        let (tx, rx) = oneshot::channel();
        apply_command(
            &*arora.store,
            &arora.function_index,
            &mut arora.engine,
            BridgeCommand::new(BridgeOp::DescribeMethods { prefix: None }, tx),
        )
        .unwrap();
        let described = rx.await.unwrap().expect("describe_methods ok");
        let signatures: Vec<MethodSignature> = arora_types::value_serde::from_value(described.ret)
            .expect("describe_methods reply deserializes to Vec<MethodSignature>");
        assert_eq!(signatures.len(), arora.function_index.len());
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
            Ok(BehaviorStatus::Done(Ok(())))
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
        // The device's input, said under its own name: the meta is namespaced
        // exactly as the value will be.
        arora
            .store
            .set_meta(HashMap::from([(
                Key::from("greeting"),
                KeyMeta::new().editable(),
            )]))
            .unwrap();

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
        arora.step(FRAME).expect("step");
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

    /// The device publishes the frame clock into the built-in keys *before* it
    /// ticks, so a behavior reads `dt`/time from the store. Nanoseconds
    /// accumulate into `time`; `dt` reflects only the latest step.
    #[test]
    fn built_in_clock_is_published_to_the_store_each_step() {
        // The clone shares the same storage, so the test reads what the device
        // writes.
        let store = SimpleDataStore::new();
        let mut arora = build_in(Box::new(FakeBridge::new()), Box::new(store.clone()));

        // Before any step the built-in keys are unset.
        assert_eq!(store.read(&[Key::from(built_in::DT)]), vec![None]);
        assert_eq!(store.read(&[Key::from(built_in::TIME)]), vec![None]);

        // Step at 16 ms: dt and elapsed time both read 16_000_000 ns.
        arora.step(Duration::from_millis(16)).expect("step");
        assert_eq!(
            store.read(&[Key::from(built_in::DT)]),
            vec![Some(Value::U64(16_000_000))]
        );
        assert_eq!(
            store.read(&[Key::from(built_in::TIME)]),
            vec![Some(Value::U64(16_000_000))]
        );

        // Step at 4 ms: dt resets to the latest delta, time accumulates to 20 ms.
        arora.step(Duration::from_millis(4)).expect("step");
        assert_eq!(
            store.read(&[Key::from(built_in::DT)]),
            vec![Some(Value::U64(4_000_000))]
        );
        assert_eq!(
            store.read(&[Key::from(built_in::TIME)]),
            vec![Some(Value::U64(20_000_000))]
        );
    }

    /// Read the built-in clock keys a device published: `(arora/time, arora/dt)`.
    fn published_clock(store: &SimpleDataStore) -> (Option<Value>, Option<Value>) {
        let mut read = store
            .read(&[Key::from(built_in::TIME), Key::from(built_in::DT)])
            .into_iter();
        (read.next().flatten(), read.next().flatten())
    }

    /// A device started at T joins the timeline at T: nothing is published
    /// before its first step, which publishes `T + dt` with an ordinary `dt`,
    /// and the steps after it count on from there.
    #[test]
    fn a_device_started_at_a_time_steps_on_from_it() {
        let start = Duration::from_secs(90);
        let store = SimpleDataStore::new();
        let mut arora = Arora::builder()
            .with_data_store(Box::new(store.clone()))
            .with_start_time(start)
            .build()
            .expect("arora builds");
        assert_eq!(published_clock(&store), (None, None));

        arora.step(FRAME).expect("step");
        let first = (start + FRAME).as_nanos() as u64;
        assert_eq!(
            published_clock(&store),
            (
                Some(Value::U64(first)),
                Some(Value::U64(FRAME.as_nanos() as u64))
            )
        );

        arora.step(Duration::from_millis(4)).expect("step");
        assert_eq!(
            published_clock(&store),
            (
                Some(Value::U64(first + 4_000_000)),
                Some(Value::U64(4_000_000))
            )
        );
    }

    /// A start beyond the clock's `u64` nanoseconds saturates, and the clock
    /// stays there rather than wrapping.
    #[test]
    fn a_start_beyond_the_clock_s_range_saturates() {
        let store = SimpleDataStore::new();
        let mut arora = Arora::builder()
            .with_data_store(Box::new(store.clone()))
            .with_start_time(Duration::MAX)
            .build()
            .expect("arora builds");
        arora.step(FRAME).expect("step");
        assert_eq!(
            published_clock(&store),
            (
                Some(Value::U64(u64::MAX)),
                Some(Value::U64(FRAME.as_nanos() as u64))
            )
        );
    }

    /// Two devices started at the same time and stepped with the same `dt`s
    /// read the same `arora/time` at every step: the timeline a late device
    /// joins is the one its peers are on.
    #[test]
    fn devices_started_at_one_time_step_in_lockstep() {
        let start = Duration::from_millis(12_345);
        let device = || {
            let store = SimpleDataStore::new();
            let arora = Arora::builder()
                .with_data_store(Box::new(store.clone()))
                .with_start_time(start)
                .build()
                .expect("arora builds");
            (arora, store)
        };
        let (mut a, a_store) = device();
        let (mut b, b_store) = device();

        let mut elapsed = start;
        for dt in [
            FRAME,
            Duration::from_millis(3),
            Duration::from_micros(16_667),
            FRAME,
        ] {
            a.step(dt).expect("step");
            b.step(dt).expect("step");
            elapsed += dt;
            let expected = Some(Value::U64(elapsed.as_nanos() as u64));
            assert_eq!(published_clock(&a_store).0, expected);
            assert_eq!(published_clock(&b_store).0, expected);
        }
    }

    /// A bridge that forwards every `try_send` payload down a channel, and is
    /// otherwise silent (never unregisters), so a test can inspect what the
    /// device actually pushes outbound — lock-free, like a real endpoint.
    struct RecordingBridge {
        sent: mpsc::UnboundedSender<StateChange>,
        /// Whether this remote asks for the device's data, as a real one does
        /// when a client attaches.
        requests_data: bool,
    }

    #[async_trait]
    impl Bridge for RecordingBridge {
        fn take_inbound(&mut self) -> InboundStream {
            if self.requests_data {
                Box::pin(
                    stream::once(async { Inbound::DataRequested(true) }).chain(stream::pending()),
                )
            } else {
                Box::pin(stream::pending())
            }
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

    /// A remote that never asks for the device's data is not written to: the
    /// device steps and keeps its state, it just does not talk to a listener
    /// that is not there.
    #[test]
    fn a_remote_that_asks_for_nothing_is_not_written_to() {
        let (sent_tx, mut sent_rx) = mpsc::unbounded();
        let mut arora = build_in_with(
            Box::new(RecordingBridge {
                sent: sent_tx,
                requests_data: false,
            }),
            Box::new(SimpleDataStore::new()),
            Box::new(WriteKey {
                key: "greeting",
                value: Value::String("hi".into()),
            }),
        );
        for _ in 0..5 {
            arora.step(FRAME).expect("step");
        }
        assert!(
            sent_rx.try_recv().is_err(),
            "nothing should reach a remote that asked for nothing"
        );
    }

    /// What one remote asks for is not what another asks for: a change reaches
    /// the endpoint that asked and no other, even on the same device.
    #[test]
    fn each_endpoint_is_answered_on_its_own_terms() {
        let (asking_tx, mut asking_rx) = mpsc::unbounded();
        let (silent_tx, mut silent_rx) = mpsc::unbounded();
        let mut arora = Arora::builder()
            .with_bridge(Box::new(RecordingBridge {
                sent: asking_tx,
                requests_data: true,
            }))
            .with_bridge(Box::new(RecordingBridge {
                sent: silent_tx,
                requests_data: false,
            }))
            .with_behavior_interpreter(Box::new(WriteKey {
                key: "greeting",
                value: Value::String("hi".into()),
            }))
            .build()
            .expect("builds");
        for _ in 0..5 {
            arora.step(FRAME).expect("step");
        }
        assert!(
            asking_rx.try_recv().is_ok(),
            "the endpoint that asked should be written to"
        );
        assert!(
            silent_rx.try_recv().is_err(),
            "the endpoint that asked for nothing should not be"
        );
    }

    /// Two devices over one shared store, each on its own namespace: what a
    /// device flushes to its remote is its own state under device-relative
    /// names, and never its neighbour's.
    #[test]
    fn devices_sharing_a_store_flush_their_own_namespace_alone() {
        let shared = SimpleDataStore::new();
        let (a_tx, mut a_rx) = mpsc::unbounded();
        let (b_tx, mut b_rx) = mpsc::unbounded();
        let mut a = build_in_with(
            Box::new(RecordingBridge {
                sent: a_tx,
                requests_data: true,
            }),
            Box::new(NamespacedStore::new(Arc::new(shared.clone()), "robotA")),
            Box::new(WriteKey {
                key: "greeting",
                value: Value::String("hi".into()),
            }),
        );
        let mut b = build_in_with(
            Box::new(RecordingBridge {
                sent: b_tx,
                requests_data: true,
            }),
            Box::new(NamespacedStore::new(Arc::new(shared.clone()), "robotB")),
            Box::new(WriteKey {
                key: "mood",
                value: Value::String("calm".into()),
            }),
        );
        for _ in 0..3 {
            a.step(FRAME).expect("step");
            b.step(FRAME).expect("step");
        }

        let forwarded = |sent: &mut mpsc::UnboundedReceiver<StateChange>| {
            let mut keys = std::collections::BTreeSet::new();
            while let Ok(change) = sent.try_recv() {
                keys.extend(change.set.keys().map(|key| key.path.clone()));
                keys.extend(change.unset.iter().map(|key| key.path.clone()));
            }
            keys
        };
        let own = |key: &str| {
            std::collections::BTreeSet::from([
                key.to_string(),
                built_in::DT.to_string(),
                built_in::TIME.to_string(),
            ])
        };
        assert_eq!(forwarded(&mut a_rx), own("greeting"));
        assert_eq!(forwarded(&mut b_rx), own("mood"));
    }

    /// The clock travels outbound with everything else, so a remote can derive
    /// the device's step rate from it; a consumer that does not want it says so
    /// on its own side.
    #[test]
    fn the_clock_is_forwarded_outbound() {
        let (sent_tx, mut sent_rx) = mpsc::unbounded();
        // A behavior that writes one ordinary key; that write must reach the bridge.
        let mut arora = build_with(
            Box::new(RecordingBridge {
                sent: sent_tx,
                requests_data: true,
            }),
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
            forwarded_keys.iter().any(|k| k.as_str() == built_in::DT),
            "the clock travels outbound like any other state, got {forwarded_keys:?}"
        );
    }

    /// A HAL that forwards, for each push, the address of the change it was
    /// handed and a copy of it — so a test can tell a lent change from one
    /// built for the HAL, and read what reached it.
    struct LendingProbe {
        sent: mpsc::UnboundedSender<(usize, StateChange)>,
    }

    #[async_trait]
    impl Hal for LendingProbe {
        async fn describe(&self) -> arora_hal::HalDescription {
            arora_hal::HalDescription::default()
        }
        async fn read(&self, keys: &[Key]) -> arora_hal::HalResult<Vec<Option<Value>>> {
            Ok(vec![None; keys.len()])
        }
        async fn read_all(&self) -> arora_hal::HalResult<arora_types::data::State> {
            Ok(Default::default())
        }
        async fn write(&self, _changes: StateChange) -> arora_hal::HalResult<()> {
            Ok(())
        }
        fn try_send(&self, changes: &StateChange) {
            let address = changes as *const StateChange as usize;
            let _ = self.sent.unbounded_send((address, changes.clone()));
        }
        fn updates(&self) -> arora_hal::UpdatesStream {
            Box::pin(stream::pending())
        }
    }

    fn change(set: &[(&str, f64)], unset: &[&str]) -> StateChange {
        StateChange {
            set: set
                .iter()
                .map(|(key, value)| (Key::from(*key), Some(Value::F64(*value))))
                .collect(),
            unset: unset.iter().map(|key| Key::from(*key)).collect(),
        }
    }

    /// A frame where the HAL reported nothing hands the HAL the outbound change
    /// itself: the HAL receives the very change the frame flushed, not a copy.
    #[test]
    fn a_hal_that_reported_nothing_is_lent_the_outbound_change() {
        let (sent, mut received) = mpsc::unbounded();
        let hal = LendingProbe { sent };
        let out = change(&[("face/mouth", 0.5), ("face/eyes", 0.1)], &["face/brow"]);

        write_hal(&hal, &out, &StateChange::new());

        let (address, got) = received.try_recv().expect("the HAL was written to");
        assert_eq!(
            address, &out as *const StateChange as usize,
            "lent, not copied"
        );
        assert_eq!(got, out);
    }

    /// A reading the behavior overwrote is no longer the HAL's own report, so
    /// there is nothing to leave out and the change is lent as well.
    #[test]
    fn a_reading_overwritten_this_frame_leaves_the_change_lent() {
        let (sent, mut received) = mpsc::unbounded();
        let hal = LendingProbe { sent };
        let out = change(&[("arm/target", 0.8), ("face/mouth", 0.5)], &[]);
        let reported = change(&[("arm/target", 0.2)], &[]);

        write_hal(&hal, &out, &reported);

        let (address, got) = received.try_recv().expect("the HAL was written to");
        assert_eq!(
            address, &out as *const StateChange as usize,
            "lent, not copied"
        );
        assert_eq!(got, out);
    }

    /// The HAL is not told what it just reported: a reading that is still the
    /// frame-final value — set or unset — is left out of what the HAL receives,
    /// and every other key reaches it.
    #[test]
    fn the_hal_is_not_told_what_it_reported() {
        let (sent, mut received) = mpsc::unbounded();
        let hal = LendingProbe { sent };
        let out = change(
            &[("arm/position", 0.2), ("face/mouth", 0.5)],
            &["arm/fault"],
        );
        let reported = change(&[("arm/position", 0.2)], &["arm/fault"]);

        write_hal(&hal, &out, &reported);

        let (_, got) = received.try_recv().expect("the HAL was written to");
        assert_eq!(got, change(&[("face/mouth", 0.5)], &[]));
    }

    /// When the frame changed nothing but the HAL's own reports, the HAL is
    /// not written to at all.
    #[test]
    fn a_frame_of_only_readings_is_not_written_back() {
        let (sent, mut received) = mpsc::unbounded();
        let hal = LendingProbe { sent };
        let out = change(&[("arm/position", 0.2)], &[]);

        write_hal(&hal, &out, &out.clone());

        assert!(received.try_recv().is_err(), "nothing to tell the HAL");
    }
}
