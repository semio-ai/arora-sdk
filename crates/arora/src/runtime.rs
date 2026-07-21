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

use arora_behavior::{golden, BehaviorContext, BehaviorInterpreter, BehaviorStatus};
use arora_behavior_tree::ModuleFunction;
use arora_bridge::{Bridge, BridgeCommand, BridgeOp, Inbound};
use arora_hal::Hal;
use arora_types::call::{CallBridge, CallError, CallResult};
use arora_types::data::{DataStore, Key, StateChange, Subscription};
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

/// The golden clock: monotonic nanoseconds since the device started, advanced by
/// each step's `dt`. Zero at build.
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

/// Phase 1a — advance the golden clock by `dt` and return this frame's clock
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

/// Phase 1b — publish the frame clock into the golden keys, before anything
/// else touches the store: the whole frame (sensor applies, command handling,
/// the behavior tick) sees this frame's time. The writes go into the store's
/// change feed like any other, and travel outbound like any other — a remote
/// derives the device's step rate from them.
fn publish_clock(store: &dyn DataStore, clock: &ClockValues) -> Result<(), RuntimeError> {
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
        BridgeOp::Call(call) => call_bridge
            .arora_call(call.clone())
            .map_err(|e| format!("call failed: {e:?}")),
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
    let mut slot = cell.try_borrow_mut().map_err(|_| CallError::Generic {
        message: "the behavior is being ticked; call between steps".to_string(),
    })?;
    let interpreter = slot.as_mut().ok_or_else(|| CallError::Generic {
        message: "no behavior interpreter is installed".to_string(),
    })?;
    operation(interpreter.as_mut()).map_err(|e| CallError::Guest {
        message: e.to_string(),
    })?;
    Ok(CallResult {
        ret: Value::Unit,
        mutated: Vec::new(),
    })
}

/// Phase 4 — tick the one behavior interpreter (a tree, a node graph, …)
/// against the shared store, **last**: its writes win the frame, and it ticks
/// over everything the frame already applied (clock, sensors, remote updates).
/// A no-op when none is installed. When the interpreter reports
/// [`BehaviorStatus::Done`] it is dropped (back to `None`); while it is
/// [`BehaviorStatus::Running`] it stays for the next step.
///
/// A failing tick does not stop the device: the behavior stays installed and
/// ticks again next step, and the rest of the pipeline — HAL writes included —
/// goes on. The failure is sent once per distinct message on the standing
/// error watch (received through [`Arora::behavior_error`]) and logged;
/// the next successful tick clears it.
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
    match behavior.tick(&mut ctx) {
        Ok(status) => {
            if standing_error.borrow().is_some() {
                standing_error.send_replace(None);
            }
            if status == BehaviorStatus::Done {
                *interpreter = None;
            }
        }
        Err(error) => {
            let message = error.to_string();
            if standing_error.borrow().as_deref() != Some(message.as_str()) {
                log::warn!("the behavior failed and will be retried: {message}");
                standing_error.send_replace(Some(message));
            }
        }
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
fn write_hal(hal: &dyn Hal, out: &StateChange, sensor_applied: &StateChange) {
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
    /// the golden keys, so the whole frame reads this frame's time — then the
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
    /// golden keys before anything else runs, so behaviors read timing from the
    /// store rather than as a tick argument.
    pub fn step(&mut self, dt: Duration) -> Result<(), RuntimeError> {
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
        apply_events(
            &*self.store,
            &self.function_index,
            &mut self.engine,
            std::mem::take(&mut self.pending.events),
            &mut self.data_requested,
        )?;
        // 4. behavior — the frame's last writer: its intent wins, and it saw
        //    what it overrode. The cell borrow spans exactly this phase; a
        //    tick-time golden edit through the engine finds it held and fails
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
            Some(_) => self.next_due = Some(now + self.period),
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
    /// Groot tree loaded into it against `store` — the construct-empty → load →
    /// inject flow, ready to hand to [`build_in_with`].
    fn groot_interpreter(xml: &str, store: &dyn DataStore) -> Box<dyn BehaviorInterpreter> {
        let mut interpreter = BehaviorTreeInterpreter::new(Rc::new(HashMap::new()));
        interpreter.load_groot(xml, store).expect("tree loads");
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
        let interpreter = groot_interpreter(xml, &store);
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
        arora.step(Duration::from_millis(16)).expect("step");
        assert_eq!(
            store.read(&[Key::from(golden::DT)]),
            vec![Some(Value::U64(16_000_000))]
        );
        assert_eq!(
            store.read(&[Key::from(golden::TIME)]),
            vec![Some(Value::U64(16_000_000))]
        );

        // Step at 4 ms: dt resets to the latest delta, time accumulates to 20 ms.
        arora.step(Duration::from_millis(4)).expect("step");
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
            forwarded_keys.iter().any(|k| k.as_str() == golden::DT),
            "the clock travels outbound like any other state, got {forwarded_keys:?}"
        );
    }
}
