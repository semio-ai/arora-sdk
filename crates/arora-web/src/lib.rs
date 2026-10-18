//! Run an Arora device in the browser.
//!
//! The centerpiece is [`AroraWeb`] — exported to JavaScript as `AroraRuntime`
//! — the `wasm_bindgen` surface over an [`arora::Arora`]: a synchronous
//! [`step`](AroraWeb::step), a self-pacing [`run`](AroraWeb::run), in-process
//! [`call`](AroraWeb::call) dispatch, and the [`store_json`] accessors that
//! move store values across the JS boundary as JSON in the Arora
//! [`Value`] vocabulary. Its JS constructor (and [`AroraWebBuilder`], JS
//! `AroraRuntimeBuilder`) builds the self-contained demo device on the
//! runtime's in-process defaults; a downstream device composes its own
//! [`arora::Arora`] with [`arora::AroraBuilder`] — the HAL, bridge, and store
//! seams are trait objects that cannot cross the JS boundary — and wraps it
//! with `AroraWeb::from`, reusing this whole JS surface.
//!
//! It also carries a lower-level surface: [`Engine`] and [`BehaviorTreeRunner`]
//! load guest modules (header JSON + executable bytes) and run behavior trees
//! directly on the engine, hosting modules via the browser's native
//! `WebAssembly` runtime — see `arora_engine::executor::browser`.
//!
//! This crate only carries non-trivial content when built for `wasm32-*`
//! targets. On the host it is an empty shim (deps are gated to `wasm32`) so it
//! can sit in the workspace and be verified by `cargo package` on publish
//! without pulling wasm-only deps into a host link.

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::collections::HashMap;
use std::pin::Pin;
use std::rc::Rc;

use arora_engine::{
    call::{CallBridge, Callable, CallableId},
    engine::EngineBuilder,
    executor::browser::{BrowserExecutor, SharedLoaderRc},
    load::load_module_from_parts,
};
use arora_types::module::low::Header;
use arora_types::{
    call::Call,
    value::{Enumeration, StructureField, StructureWithoutId, Value},
};
use uuid::Uuid;
use wasm_bindgen::prelude::*;

/// Route Rust panics to the browser console. Called from every entry point
/// (the [`AroraWeb`] constructors, `Engine::new`, `BehaviorTreeRunner::new`)
/// rather than from a `#[wasm_bindgen(start)]`: a reusable library must not
/// claim the module `start`, or every downstream cdylib that also defines one
/// collides on the `_start` symbol at link time. `set_once` makes repeat calls
/// free.
fn install_panic_hook() {
    console_error_panic_hook::set_once();
}

// =============================================================================
// The browser device
// =============================================================================

use arora::{Arora, AroraBuilder, Caller, LocalCaller, MethodSignature, TaskId};
use arora_bridge::client::{self, KeyInfo};
use arora_types::call::CallError;
use arora_types::data::{DataStore, Key, StateChange, Subscription};
use futures::future::Either;
use futures::FutureExt;
use std::cell::Cell;
use std::future::Future;
use std::task::Poll;
use std::time::Duration;
use tokio::sync::watch;

/// Hand `future` to JavaScript as a Promise, having polled it once already: a
/// future that is already done settles the promise at once, and one that is
/// not is polled on from wasm-bindgen-futures' queue.
fn promise(future: impl Future<Output = Result<JsValue, JsValue>> + 'static) -> js_sys::Promise {
    let mut future = Box::pin(future);
    let mut cx = std::task::Context::from_waker(futures::task::noop_waker_ref());
    match future.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(Ok(value)) => js_sys::Promise::resolve(&value),
        std::task::Poll::Ready(Err(error)) => js_sys::Promise::reject(&error),
        std::task::Poll::Pending => wasm_bindgen_futures::future_to_promise(future),
    }
}

/// Serialize to a plain JS value — objects as objects, not `Map`s — the form
/// every structured answer of this surface takes.
fn to_js(value: &impl serde::Serialize) -> Result<JsValue, JsValue> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| JsValue::from_str(&format!("serialize failed: {e}")))
}

/// Serialize an Arora [`Value`] to the plain JS form of its JSON, e.g.
/// `{"f32": 0.75}`.
fn value_to_js(value: &Value) -> Result<JsValue, JsValue> {
    let json = serde_json::to_value(value)
        .map_err(|e| JsValue::from_str(&format!("serialize value failed: {e}")))?;
    to_js(&json)
}

/// A run handle as a client reads it — the plain object `invoke` of a
/// task-shaped method and `spawn` answer with: `arora_bridge::client::Run`.
fn run_to_js(handle: &arora::TaskHandle) -> Result<JsValue, JsValue> {
    let spawned = arora_behavior::interpreter_module::encode_spawn_result(handle);
    let run = client::run_of(&spawned).map_err(|e| JsValue::from_str(&e))?;
    to_js(&run)
}

/// Value↔JSON accessors over a device's [`DataStore`] and change
/// [`Subscription`] — the store surface every browser device shares. Values
/// cross the JS boundary as JSON in the Arora [`Value`] vocabulary, e.g.
/// `{"f32": 0.75}`. [`AroraWeb`] exposes them as methods; a downstream device
/// wrapping its own store reaches them here directly.
pub mod store_json {
    use super::*;

    /// Write one key into the store. `value_json` is an Arora [`Value`] as
    /// JSON, e.g. `{"f32": 0.75}`.
    pub fn set_value(store: &dyn DataStore, path: &str, value_json: &str) -> Result<(), JsValue> {
        let value: Value = serde_json::from_str(value_json)
            .map_err(|e| JsValue::from_str(&format!("invalid value json for {path}: {e}")))?;
        store
            .write(StateChange::set(path, value))
            .map_err(|e| JsValue::from_str(&format!("write {path} failed: {e}")))
    }

    /// Write several keys at once, as one store change. `values_json` is a JSON
    /// object mapping each key path to an Arora [`Value`], e.g.
    /// `{"sensor/x": {"f32": 0.75}}`.
    pub fn write_values(store: &dyn DataStore, values_json: &str) -> Result<(), JsValue> {
        let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(values_json)
            .map_err(|e| JsValue::from_str(&format!("invalid values json: {e}")))?;
        let mut change = StateChange::new();
        for (path, raw) in map {
            let value: Value = serde_json::from_value(raw)
                .map_err(|e| JsValue::from_str(&format!("invalid value for {path}: {e}")))?;
            change.set.insert(Key::new(path), Some(value));
        }
        store
            .write(change)
            .map_err(|e| JsValue::from_str(&format!("write failed: {e}")))
    }

    /// Read keys from the store. `paths` is a JS `string[]`; the result is a JS
    /// object mapping each path to its Arora [`Value`] (or `null` if absent).
    pub fn read_values(store: &dyn DataStore, paths: JsValue) -> Result<JsValue, JsValue> {
        let paths: Vec<String> = serde_wasm_bindgen::from_value(paths)
            .map_err(|e| JsValue::from_str(&format!("paths must be a string[]: {e}")))?;
        let keys: Vec<Key> = paths.iter().map(Key::new).collect();
        let values = store.read(&keys);
        let mut out = serde_json::Map::with_capacity(paths.len());
        for (path, value) in paths.into_iter().zip(values) {
            out.insert(path, value_to_json(value)?);
        }
        to_js_object(out)
    }

    /// A snapshot of every key currently in the store, as a JS object mapping
    /// path → Arora [`Value`].
    pub fn snapshot(store: &dyn DataStore) -> Result<JsValue, JsValue> {
        let state = store.snapshot();
        let mut out = serde_json::Map::with_capacity(state.storage.len());
        for (key, value) in state.storage {
            out.insert(key.path, value_to_json(value)?);
        }
        to_js_object(out)
    }

    /// Drain the changes the subscription holds, as a JS object mapping each
    /// changed path to its latest Arora [`Value`] (or `null` for a cleared
    /// key). Poll-based counterpart to a push subscription:
    /// [`DataStore::subscribe`] delivers over a std channel JavaScript cannot
    /// await, so the channel holds every change until it is drained here. A
    /// subscription opens on the store's whole current state, so the first
    /// drain returns the full picture.
    ///
    /// [`AroraWeb`] does not leave its subscription's channel to grow between
    /// drains: it folds it into the latest value per key on every step.
    pub fn drain_changes(changes: &Subscription) -> Result<JsValue, JsValue> {
        let mut latest = HashMap::new();
        while let Some(change) = changes.try_recv() {
            fold_change(&mut latest, change);
        }
        latest_to_js(latest.drain())
    }

    /// Fold `change` into `latest`, the latest value per key: a key `change`
    /// sets holds its new value, a key it unsets holds `None`.
    pub(crate) fn fold_change(latest: &mut HashMap<Key, Option<Value>>, change: StateChange) {
        latest.extend(change.set);
        latest.extend(change.unset.into_iter().map(|key| (key, None)));
    }

    /// The latest value per key as a JS object mapping path → Arora [`Value`]
    /// (`null` for a cleared key).
    pub(crate) fn latest_to_js(
        latest: impl Iterator<Item = (Key, Option<Value>)>,
    ) -> Result<JsValue, JsValue> {
        let mut out = serde_json::Map::new();
        for (key, value) in latest {
            out.insert(key.path, value_to_json(value)?);
        }
        to_js_object(out)
    }

    /// Serialize a path-keyed JSON map to a **plain JS object**.
    /// serde-wasm-bindgen's default representation for maps is a JS `Map`, but
    /// these accessors promise plain objects (`{"sensor/x": {"f32": 0.5}}`),
    /// so serialize JSON-compatibly.
    fn to_js_object(out: serde_json::Map<String, serde_json::Value>) -> Result<JsValue, JsValue> {
        use serde::Serialize;
        serde_json::Value::Object(out)
            .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
            .map_err(|e| JsValue::from_str(&format!("serialize failed: {e}")))
    }

    /// Serialize an optional Arora value to JSON (`null` when absent).
    fn value_to_json(value: Option<Value>) -> Result<serde_json::Value, JsValue> {
        match value {
            Some(v) => serde_json::to_value(v)
                .map_err(|e| JsValue::from_str(&format!("serialize value failed: {e}"))),
            None => Ok(serde_json::Value::Null),
        }
    }
}

/// The store changes a page has not drained yet, as the latest value per key.
///
/// The subscription's channel takes a copy of every change the store applies;
/// a device changing a few hundred keys per step at 100 steps a second fills
/// it with tens of thousands of entries a second, all held until a drain.
/// Folded on every step and on every write the page makes, the channel holds
/// no more than one step's changes, and `latest` one entry per key changed
/// since the last drain: what a page has not drained grows with the keys that
/// change, not with the steps it goes without draining. A drain hands over the
/// net change since the previous one — a key set twice reads its second value,
/// a key set then unset reads cleared — which is what a page shows.
struct Undrained {
    subscription: Subscription,
    latest: HashMap<Key, Option<Value>>,
}

impl Undrained {
    fn new(subscription: Subscription) -> Self {
        Self {
            subscription,
            latest: HashMap::new(),
        }
    }

    /// Move what the subscription's channel holds into `latest`.
    fn fold(&mut self) {
        while let Some(change) = self.subscription.try_recv() {
            store_json::fold_change(&mut self.latest, change);
        }
    }

    /// Everything not drained yet, as a path → Value JS object; `latest`
    /// keeps its capacity for the changes that follow.
    fn drain(&mut self) -> Result<JsValue, JsValue> {
        self.fold();
        store_json::latest_to_js(self.latest.drain())
    }
}

#[wasm_bindgen]
extern "C" {
    /// The global `queueMicrotask`, in a page, a worker and Node alike.
    #[wasm_bindgen(js_name = queueMicrotask)]
    fn queue_microtask(callback: &js_sys::Function);

    /// The global `setTimeout`, in a page, a worker and Node alike.
    #[wasm_bindgen(js_name = setTimeout)]
    fn set_timeout(callback: &js_sys::Function, ms: i32);

    #[wasm_bindgen(js_namespace = console, js_name = error)]
    fn console_error(message: &str);
}

/// What the device's steps did that a waiting operation acts on, sent to every
/// operation awaiting its reply.
#[derive(Clone, Debug)]
enum Stepped {
    /// Nothing an operation waits on has happened yet.
    Idle,
    /// A step failed with this message, so an operation it left unanswered
    /// rejects with it rather than waiting for a step that may fail the same
    /// way.
    Failed(String),
    /// [`run`](AroraWeb::run) gave the device back. An operation its steps
    /// left unanswered asks for a step at a zero dt, since no run step
    /// will apply it now.
    Parked,
}

/// Steps the device at a zero dt for what the page asked of it while no
/// [`run`](AroraWeb::run) is ongoing.
///
/// The requests of one turn — the page's operations and its writes to the
/// store — share one step, taken in the next microtask. It applies everything
/// queued on the device's inbound channel (the page's operations in the order
/// it issued them, and what bridges delivered), ticks the behavior with no
/// time elapsed, and writes the HAL and the bridges what changed, the page's
/// writes with it. A step that runs first takes the place of the requested
/// one: a page's own [`step`](AroraWeb::step) clears the request, and while
/// `run()` owns the device its next step applies the queue, so the request
/// lapses.
///
/// A request made from inside a step cannot nest one: its step follows the
/// current one as a task (`setTimeout`), not a microtask, so a callback that
/// asks for one on every tick paces a chain of steps on the event loop
/// instead of starving it.
#[derive(Clone)]
struct StepScheduler {
    device: Rc<RefCell<Option<Arora>>>,
    changes: Rc<RefCell<Undrained>>,
    // Whether a step at a zero dt is due; a microtask or a task is queued
    // whenever it is.
    due: Rc<Cell<bool>>,
    stepped: Rc<watch::Sender<Stepped>>,
}

impl StepScheduler {
    fn new(device: Arora, changes: Rc<RefCell<Undrained>>) -> Self {
        Self {
            device: Rc::new(RefCell::new(Some(device))),
            changes,
            due: Rc::new(Cell::new(false)),
            stepped: Rc::new(watch::channel(Stepped::Idle).0),
        }
    }

    /// Whether a step holds the device: a page's own, or one at a zero dt.
    fn mid_step(&self) -> bool {
        self.device.try_borrow_mut().is_err()
    }

    /// Ask for a step at a zero dt: in the next microtask, or after the
    /// current step when asked from inside one. Requests made before it runs
    /// share it.
    fn request(&self) {
        if self.due.replace(true) {
            return;
        }
        let scheduler = self.clone();
        let callback = Closure::once_into_js(move || scheduler.step_if_due());
        if self.mid_step() {
            set_timeout(callback.unchecked_ref(), 0);
        } else {
            queue_microtask(callback.unchecked_ref());
        }
    }

    /// A step is about to apply the queue: the request is met by it.
    fn met(&self) {
        self.due.set(false);
    }

    /// A step failed: what it left unanswered rejects with `message`.
    fn failed(&self, message: String) {
        self.stepped.send_replace(Stepped::Failed(message));
    }

    fn step_if_due(&self) {
        if !self.due.replace(false) {
            return;
        }
        let Ok(mut parked) = self.device.try_borrow_mut() else {
            // A microtask or a task runs between steps, never inside one;
            // ask again after this one all the same.
            self.request();
            return;
        };
        // While `run()` owns the device, the run's next step applies the
        // queue.
        let Some(device) = parked.as_mut() else {
            return;
        };
        let stepped = device.step(Duration::ZERO);
        drop(parked);
        self.changes.borrow_mut().fold();
        if let Err(e) = stepped {
            let message = e.to_string();
            // A page's write has no promise to reject: the console says it.
            console_error(&format!("arora-web: a step at a zero dt failed: {message}"));
            self.failed(message);
        }
    }
}

/// The browser Arora device, exported to JavaScript as `AroraRuntime`.
///
/// The JS constructor builds the self-contained demo device — the runtime's
/// in-process defaults: fake hardware, no bridge, a private store, an empty
/// behavior — and [`AroraWebBuilder`] (JS `AroraRuntimeBuilder`) additionally
/// loads guest modules. A downstream device composes its own [`Arora`] with
/// [`arora::AroraBuilder`] and wraps it with `AroraWeb::from`, reusing this
/// whole JS surface.
///
/// Drive it by calling [`step`](Self::step) from your own clock (e.g.
/// `requestAnimationFrame`) or by awaiting [`run`](Self::run), which hands the
/// device to [`Arora::run`] until [`stop`](AroraWeb::stop) reclaims it.
/// Either way the rest of the surface stays live: `setValue`/`readValues`/
/// `snapshot` work on a sibling handle of the store, `drainChanges` on its
/// subscription — folded into the latest value per key on every step, so what
/// a page has not drained is one entry per changed key however many steps it
/// goes without draining — and the client operations are the ones a remote
/// client has over a bridge.
///
/// The operations that change the device — `call`, `invoke`, `spawn`, `halt` —
/// are queued on its in-process [`LocalCaller`] before the method returns,
/// and settle on the reply of the step that applies them:
/// - while no `run()` is ongoing, a step at a zero dt the operation asks for:
///   the operations issued in one turn share one such step, applied in the
///   order they were issued, so a page that never steps the device has them
///   applied all the same. A page that steps in the same turn applies them
///   with its own step, and none at a zero dt follows;
/// - while `run()` owns the device, the run's next step: the cadence is the
///   run's alone.
///
/// A step at a zero dt is an ordinary step with no time elapsed: it publishes
/// `arora/dt` as zero and ticks the behavior once, so what runs on time holds
/// while a node that counts ticks advances one. A step that fails rejects each
/// operation it left unanswered with its error. That operation is not
/// withdrawn: if the step failed before applying the queue, a later step still
/// applies it.
///
/// A page's writes to the store — `setValue`, `writeValues` — hold in the store
/// when the method returns, and reach the HAL and the bridges by the same
/// steps: a step at a zero dt they share with the turn's operations while no
/// `run()` is ongoing, the run's next step while one is.
///
/// The operations that only read — `listKeys`, `describeMethods` — answer
/// from the device's current state, with no step, in both modes: the keys
/// from the store's sibling handle, the methods from the method index read
/// when the device was wrapped (it changes only through `Arora`'s module
/// methods, which this wrapper does not call). A read does not see an
/// operation still queued, so a page awaits that operation's promise first.
///
/// A call into this surface from a callback the step itself invokes is an
/// ordinary call: a step cannot nest, so an operation it issues is queued, and
/// applied by a step at a zero dt taken as a task once the current step has
/// ended (or by the run's next step). A write it makes is carried out by the
/// current step, or by the next one if the current step has already flushed.
/// A page should not rely on such calls: what a step needs done belongs in a
/// behavior, where it is part of the step.
///
/// Every method is an ordinary Rust method too, for a Rust crate that wraps
/// this device in its own `wasm_bindgen` type; [`caller`](Self::caller) hands
/// such a crate the typed operations, and [`complete`](Self::complete) has
/// them settle as this surface's do.
#[wasm_bindgen(js_name = AroraRuntime)]
pub struct AroraWeb {
    // The device and the zero-dt steps taken for the page. The device parks
    // in the scheduler between steps; `run` takes it out and the run
    // future puts it back.
    steps: StepScheduler,
    // Ends the current `run` loop at its next step boundary; `None` while no
    // loop runs (the sender is consumed by `stop`).
    stop: RefCell<Option<futures::channel::oneshot::Sender<()>>>,
    caller: LocalCaller,
    store: Box<dyn DataStore>,
    // The device's method index, read when it was wrapped: `describeMethods`
    // and `invoke` answer from it without a step.
    methods: Vec<MethodSignature>,
    // Shared with the run future, which folds the subscription after every
    // step it takes.
    changes: Rc<RefCell<Undrained>>,
    // The device's standing-error watch, taken before `run` can own the
    // device, so the reading stays available while it self-paces.
    behavior_error: watch::Receiver<Option<String>>,
    // The awaitable end of the same watch: taken out for the duration of one
    // `behaviorErrorChanged` await, so sequential awaits share one seen
    // cursor and no change is missed between them.
    behavior_error_changes: Rc<RefCell<Option<watch::Receiver<Option<String>>>>>,
}

impl From<Arora> for AroraWeb {
    /// Wrap a composed device: keep its in-process [`LocalCaller`], a sibling
    /// handle of its store, its method index, and a subscription (opening on
    /// the whole current state), then expose it all to JavaScript.
    fn from(arora: Arora) -> Self {
        install_panic_hook();
        let caller = arora.caller();
        let store = arora.store().clone_box();
        let methods = arora.describe_methods(None);
        let changes = Rc::new(RefCell::new(Undrained::new(store.subscribe())));
        let behavior_error = arora.behavior_error();
        let behavior_error_changes = Rc::new(RefCell::new(Some(arora.behavior_error())));
        Self {
            steps: StepScheduler::new(arora, changes.clone()),
            stop: RefCell::new(None),
            caller,
            store,
            methods,
            changes,
            behavior_error,
            behavior_error_changes,
        }
    }
}

#[wasm_bindgen(js_class = "AroraRuntime")]
impl AroraWeb {
    /// Build the demo device on the runtime's in-process defaults: fake HAL,
    /// no bridge, a private store, an empty (idle) behavior interpreter.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<AroraWeb, JsValue> {
        Ok(AroraWeb::from(Arora::builder().build().map_err(|e| {
            JsValue::from_str(&format!("arora build failed: {e:?}"))
        })?))
    }

    /// Advance the device one step. `dt_ms` is the milliseconds elapsed since
    /// the previous step — a plain JS number, exactly what a
    /// `requestAnimationFrame` timestamp delta gives. This is the only place
    /// the rAF millisecond unit is converted to the core `Duration` dt.
    /// It applies every operation queued so far, so none of them takes a step
    /// at a zero dt. Unavailable once [`run`](Self::run) has taken the device,
    /// and from a callback the step itself invokes.
    pub fn step(&self, dt_ms: f64) -> Result<(), JsValue> {
        let mut parked = self
            .steps
            .device
            .try_borrow_mut()
            .map_err(|_| JsValue::from_str("the device is mid-step; call between steps"))?;
        let device = parked
            .as_mut()
            .ok_or_else(|| JsValue::from_str("run() drives the device; step() is unavailable"))?;
        self.steps.met();
        let stepped = device.step(Duration::from_secs_f64(dt_ms / 1_000.0));
        drop(parked);
        self.changes.borrow_mut().fold();
        stepped.map_err(|e| {
            // The operations this step left unanswered reject with its error
            // rather than wait for a step that may fail the same way.
            self.steps.failed(e.to_string());
            JsValue::from_str(&format!("step failed: {e}"))
        })
    }

    /// Hand the device to [`Arora::run`]: a self-paced loop at `period_ms`
    /// (default: the runtime's ~100 Hz) that owns the device until stepping
    /// fails — the promise rejects — or [`stop`](Self::stop) reclaims it — the
    /// promise resolves and `step()` works again. While it runs the rest of
    /// the surface keeps working, and the operations that change the device
    /// apply at the run's next step, adding no step of their own.
    ///
    /// `run` awaits between steps, so it shares its thread; see
    /// [`Arora::run`] for where a cadence survives in a browser (main thread
    /// vs dedicated worker).
    pub fn run(&self, period_ms: Option<f64>) -> js_sys::Promise {
        let taken = match self.steps.device.try_borrow_mut() {
            Ok(mut parked) => parked.take(),
            Err(_) => {
                return js_sys::Promise::reject(&JsValue::from_str(
                    "the device is mid-step; call between steps",
                ))
            }
        };
        let Some(mut device) = taken else {
            return js_sys::Promise::reject(&JsValue::from_str("the device is already running"));
        };
        let steps = self.steps.clone();
        let changes = self.changes.clone();
        let (stop_tx, stop_rx) = futures::channel::oneshot::channel::<()>();
        *self.stop.borrow_mut() = Some(stop_tx);
        wasm_bindgen_futures::future_to_promise(async move {
            let period = period_ms
                .map(|ms| Duration::from_secs_f64(ms / 1_000.0))
                .unwrap_or(Arora::DEFAULT_STEP_PERIOD);
            let outcome = {
                let run = device.run(period);
                futures::pin_mut!(run);
                // Each step runs inside one poll of the loop: folding after
                // every poll folds every step's changes.
                let run = futures::future::poll_fn(|cx| {
                    let polled = run.as_mut().poll(cx);
                    changes.borrow_mut().fold();
                    polled
                })
                .fuse();
                let mut stop_rx = stop_rx.fuse();
                futures::pin_mut!(run);
                futures::select_biased! {
                    result = run => Some(result),
                    _ = stop_rx => None,
                }
            };
            // Whatever ended the loop, the device parks back in the scheduler: a
            // failed run leaves a device the embedder can still step, probe
            // ([`Arora::behavior_error`]), or run again. An operation the
            // run's steps left unanswered now takes a step at a zero dt.
            steps.device.borrow_mut().replace(device);
            steps.stepped.send_replace(Stepped::Parked);
            match outcome {
                // `Arora::run` only returns by failing; surface that.
                Some(result) => {
                    result.map_err(|e| JsValue::from_str(&format!("run failed: {e}")))?;
                    Ok(JsValue::UNDEFINED)
                }
                // Stopped: the loop ended between steps.
                None => Ok(JsValue::UNDEFINED),
            }
        })
    }

    /// Reclaim the device from [`run`](Self::run): the loop ends at its next
    /// step boundary, the run promise resolves, and `step()` (or another
    /// `run()`) works again. A no-op while nothing runs.
    pub fn stop(&self) {
        if let Some(stop) = self.stop.borrow_mut().take() {
            let _ = stop.send(());
        }
    }

    /// Whether [`run`](Self::run) currently owns the device.
    #[wasm_bindgen(getter)]
    pub fn running(&self) -> bool {
        // Held mid-step by a page's own step or a zero-dt one: not running.
        self.steps
            .device
            .try_borrow()
            .is_ok_and(|parked| parked.is_none())
    }

    /// The behavior's standing error — the message of its latest failed
    /// tick, `undefined` while the behavior is healthy or none is installed.
    /// A failing tick does not stop the device; the reading stays available
    /// while `run()` owns it.
    #[wasm_bindgen(getter, js_name = behaviorError)]
    pub fn behavior_error(&self) -> Option<String> {
        self.behavior_error.borrow().clone()
    }

    /// Resolves on the next change of the behavior's standing error, with
    /// the new reading: a message when a distinct failure appears,
    /// `undefined` when a tick recovers. Sequential awaits share one cursor,
    /// so no change is missed between them; one await may be pending at a
    /// time. Rejects when the device is gone.
    #[wasm_bindgen(js_name = behaviorErrorChanged)]
    pub fn behavior_error_changed(&self) -> js_sys::Promise {
        let Some(mut errors) = self.behavior_error_changes.borrow_mut().take() else {
            return js_sys::Promise::reject(&JsValue::from_str(
                "a behaviorErrorChanged await is already pending",
            ));
        };
        let home = self.behavior_error_changes.clone();
        wasm_bindgen_futures::future_to_promise(async move {
            let outcome = errors.changed().await;
            let value = errors.borrow_and_update().clone();
            home.borrow_mut().replace(errors);
            outcome.map_err(|_| JsValue::from_str("the device is gone"))?;
            Ok(match value {
                Some(message) => JsValue::from_str(&message),
                None => JsValue::UNDEFINED,
            })
        })
    }

    /// Dispatch a [`Call`] into the device through its in-process caller.
    /// `call_json` is an `arora_types::call::Call` as JSON; the promise
    /// resolves to the `CallResult` as a JSON string once the call is applied
    /// (see [`AroraWeb`] for which step applies it).
    pub fn call(&self, call_json: &str) -> js_sys::Promise {
        let call: Call = match serde_json::from_str(call_json) {
            Ok(call) => call,
            Err(e) => return reject(format!("invalid call json: {e}")),
        };
        let reply = self.call_now(call);
        promise(async move {
            let result = reply
                .await
                .map_err(|e| JsValue::from_str(&format!("call failed: {e}")))?;
            let json = serde_json::to_string(&result)
                .map_err(|e| JsValue::from_str(&format!("serialize failed: {e}")))?;
            Ok(JsValue::from_str(&json))
        })
    }

    /// Call the device's method named `method` with arguments by parameter
    /// name, as a remote client invokes it over a bridge.
    ///
    /// `args_json` is a JSON object mapping each parameter name to an Arora
    /// [`Value`] in the JSON form `call` takes, e.g. `{"x": {"f64": 2.5}}`. The
    /// promise resolves to the return value, as a plain JS value of that JSON
    /// form. A task-shaped method — one returning the behavior `Status` — is
    /// spawned as a run instead, and resolves to the run's handle, the plain
    /// object [`spawn`](Self::spawn) answers with.
    ///
    /// Names are the bare names modules export, so two modules may share one:
    /// `module_id` (a uuid string) names the exporting module to choose. A name
    /// more than one module exports, without `module_id`, rejects naming those
    /// modules. An argument the method does not declare rejects too.
    ///
    /// The method's signature is read at once, from the device's method
    /// index; its call is applied as `call`'s is.
    pub fn invoke(
        &self,
        method: &str,
        args_json: &str,
        module_id: Option<String>,
    ) -> js_sys::Promise {
        let failed = |e: String| reject(format!("invoke {method} failed: {e}"));
        let args: HashMap<String, Value> = match serde_json::from_str(args_json) {
            Ok(args) => args,
            Err(e) => return reject(format!("invalid args json: {e}")),
        };
        let module = match module_id.map(|id| Uuid::parse_str(&id).map_err(|e| (id, e))) {
            None => None,
            Some(Ok(module)) => Some(module),
            Some(Err((id, e))) => return reject(format!("'{id}' is not a module id: {e}")),
        };
        let signature = match client::find_method(&self.methods, method, module) {
            Ok(signature) => signature,
            Err(e) => return failed(e),
        };
        let call = match client::call_of(signature, args) {
            Ok(call) => call,
            Err(e) => return failed(e),
        };
        let method = method.to_string();
        let failed = move |e: CallError| JsValue::from_str(&format!("invoke {method} failed: {e}"));
        if client::task_shaped(&signature.function) {
            let caller = self.caller.clone();
            let started = self.complete(async move { caller.spawn(call).await });
            promise(async move { run_to_js(&started.await.map_err(failed)?) })
        } else {
            let returned = self.call_now(call);
            promise(async move { value_to_js(&returned.await.map_err(failed)?.ret) })
        }
    }

    /// Start `call_json` (an `arora_types::call::Call` as JSON) as a task run,
    /// concurrently with every other run. The promise resolves to the run's
    /// handle, a plain object `{run, status, feedback, result, update}`: `run`
    /// the run id (a uuid string, which [`halt`](Self::halt) takes), `status`
    /// the path of the key that reports how the run goes and ends, and
    /// `feedback`, `result` and `update` the paths of the keys carrying its
    /// progress, its result, and what steers it. It resolves once the spawn is
    /// applied, as `call`'s does.
    pub fn spawn(&self, call_json: &str) -> js_sys::Promise {
        let call: Call = match serde_json::from_str(call_json) {
            Ok(call) => call,
            Err(e) => return reject(format!("invalid call json: {e}")),
        };
        let caller = self.caller.clone();
        let started = self.complete(async move { caller.spawn(call).await });
        promise(async move {
            let handle = started
                .await
                .map_err(|e| JsValue::from_str(&format!("spawn failed: {e}")))?;
            run_to_js(&handle)
        })
    }

    /// Stop the run `run_id` names (the `run` of its handle, a uuid string):
    /// it ends `Failure` on the step after the halt is applied. Idempotent —
    /// halting a finished or unknown run resolves all the same. It resolves
    /// once the halt is applied, as `call`'s does.
    pub fn halt(&self, run_id: &str) -> js_sys::Promise {
        let run = match Uuid::parse_str(run_id) {
            Ok(run) => TaskId(run),
            Err(e) => return reject(format!("'{run_id}' is not a run id: {e}")),
        };
        let caller = self.caller.clone();
        let halted = self.complete(async move { caller.halt(run).await });
        promise(async move {
            halted
                .await
                .map_err(|e| JsValue::from_str(&format!("halt failed: {e}")))?;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// The device's keys whose path starts with `prefix` (all of them when
    /// absent), sorted by path. The promise resolves to an array of
    /// `{path, __meta}` objects: each key with what its store says it is — the
    /// shape it holds, its range and unit, where it rests, whether a client may
    /// write it — as every Arora bridge lists keys. It answers from the store
    /// as it is, with no step, and resolves at once.
    #[wasm_bindgen(js_name = listKeys)]
    pub fn list_keys(&self, prefix: Option<String>) -> js_sys::Promise {
        let keys: Vec<KeyInfo> = arora::list_keys(&*self.store, prefix.as_deref())
            .into_iter()
            .map(|(path, meta)| KeyInfo { path, meta })
            .collect();
        settled(to_js(&keys))
    }

    /// The device's callable methods whose name starts with `prefix` (all of
    /// them when absent), sorted by name. The promise resolves to an array of
    /// method descriptions, as a bridge lists methods — `path` (the name),
    /// `params` (each `name`, `param_type`, `required`, in declaration order),
    /// `return_type`, `task` (whether invoking it starts a run) — plus `module`,
    /// the exporting module's id, which [`invoke`](Self::invoke) takes when two
    /// modules export one name. It answers from the device's method index,
    /// with no step, and resolves at once.
    #[wasm_bindgen(js_name = describeMethods)]
    pub fn describe_methods(&self, prefix: Option<String>) -> js_sys::Promise {
        let described = self
            .methods
            .iter()
            .filter(|signature| {
                prefix
                    .as_deref()
                    .is_none_or(|prefix| signature.name.starts_with(prefix))
            })
            .map(|signature| {
                let mut method = serde_json::to_value(client::method_info(signature))
                    .map_err(|e| JsValue::from_str(&format!("serialize failed: {e}")))?;
                method["module"] = serde_json::Value::String(signature.module_id.to_string());
                Ok(method)
            })
            .collect::<Result<Vec<_>, JsValue>>();
        settled(described.and_then(|methods| to_js(&methods)))
    }

    /// Write one key into the store (Arora [`Value`] as JSON, e.g. `{"f32": 1}`).
    /// The store holds it when this returns; the step that carries it to the
    /// HAL and the bridges is the one a client operation takes (see
    /// [`AroraWeb`]).
    #[wasm_bindgen(js_name = setValue)]
    pub fn set_value(&self, path: &str, value_json: &str) -> Result<(), JsValue> {
        let written = store_json::set_value(&*self.store, path, value_json);
        self.wrote(written)
    }

    /// Write several keys at once, as one store change (a JSON object mapping
    /// each key path to an Arora [`Value`]). The store holds them when this
    /// returns, and a step carries them out as `setValue`'s.
    #[wasm_bindgen(js_name = writeValues)]
    pub fn write_values(&self, values_json: &str) -> Result<(), JsValue> {
        let written = store_json::write_values(&*self.store, values_json);
        self.wrote(written)
    }

    /// Read keys from the store; `paths` is a `string[]`, result maps path→Value.
    #[wasm_bindgen(js_name = readValues)]
    pub fn read_values(&self, paths: JsValue) -> Result<JsValue, JsValue> {
        store_json::read_values(&*self.store, paths)
    }

    /// A snapshot of every key in the store as a path→Value object.
    pub fn snapshot(&self) -> Result<JsValue, JsValue> {
        store_json::snapshot(&*self.store)
    }

    /// Drain the keys that changed since the last call, as a path→Value object
    /// holding each key's latest value (`null` for a cleared key): the net
    /// change since the previous drain. The first drain returns the store's
    /// whole current state — the subscription opens on it. What is not
    /// drained is held as one value per key, however long the page goes
    /// without draining.
    #[wasm_bindgen(js_name = drainChanges)]
    pub fn drain_changes(&self) -> Result<JsValue, JsValue> {
        self.changes.borrow_mut().drain()
    }
}

impl AroraWeb {
    /// The device's in-process caller: the typed operations behind `call`,
    /// `invoke`, `spawn` and `halt`, for a Rust crate that wraps this device
    /// and wants them unserialized. An operation through it is a remote's: it
    /// applies at the device's next step. Hand its reply to
    /// [`complete`](Self::complete) to have it settle as this surface's
    /// operations do.
    pub fn caller(&self) -> LocalCaller {
        self.caller.clone()
    }

    /// Settle `reply`, the reply of an operation that changes the device, as
    /// this surface's own `call`, `invoke`, `spawn` and `halt` settle: applied
    /// by a step at a zero dt while no [`run`](Self::run) is ongoing — the
    /// operations issued in one turn sharing one — and by the run's next step
    /// while one is.
    ///
    /// `reply` is polled once before this returns, so an operation that
    /// queues itself at its future's first poll (a [`LocalCaller`] call
    /// wrapped in an `async` block) is queued by then, behind those issued
    /// before it. A reply that is already there settles at once. A step that
    /// fails rejects every operation it left unanswered with its error,
    /// without withdrawing it (see [`AroraWeb`]). A read-only operation has nothing to apply: answer it from the
    /// device's state instead (`listKeys`, `describeMethods`).
    pub fn complete<V: 'static>(
        &self,
        reply: impl Future<Output = Result<V, CallError>> + 'static,
    ) -> impl Future<Output = Result<V, CallError>> + 'static {
        let mut reply = Box::pin(reply);
        let first = reply.as_mut().poll(&mut std::task::Context::from_waker(
            futures::task::noop_waker_ref(),
        ));
        let mut stepped = self.steps.stepped.subscribe();
        let steps = self.steps.clone();
        if first.is_pending() {
            steps.request();
        }
        async move {
            if let Poll::Ready(answer) = first {
                return answer;
            }
            loop {
                {
                    // The sender lives in `steps`, which this future holds:
                    // the channel stays open while it waits.
                    let changed = std::pin::pin!(stepped.changed());
                    if let Either::Left((answer, _)) =
                        futures::future::select(reply.as_mut(), changed).await
                    {
                        return answer;
                    }
                }
                let event = stepped.borrow_and_update().clone();
                match event {
                    Stepped::Idle => {}
                    Stepped::Failed(message) => {
                        return match reply.as_mut().now_or_never() {
                            Some(answer) => answer,
                            None => Err(CallError::Generic {
                                message: format!("step failed: {message}"),
                            }),
                        }
                    }
                    Stepped::Parked => steps.request(),
                }
            }
        }
    }

    /// After a page's write to the store: fold it into what the page has not
    /// drained, and have a step carry it to the HAL and the bridges — a step
    /// at a zero dt while no run is ongoing, the run's next step while one is.
    ///
    /// A write from inside a step asks for none: the step that holds the
    /// device flushes it, or, written once that step has flushed, the next
    /// step does.
    fn wrote(&self, written: Result<(), JsValue>) -> Result<(), JsValue> {
        self.changes.borrow_mut().fold();
        if written.is_ok() && !self.steps.mid_step() {
            self.steps.request();
        }
        written
    }

    /// Queue `call` and settle its reply as [`complete`](Self::complete) does.
    fn call_now(
        &self,
        call: Call,
    ) -> impl Future<Output = Result<arora_types::call::CallResult, CallError>> + 'static {
        let caller = self.caller.clone();
        self.complete(async move { caller.call(call).await })
    }
}

/// A promise rejected with `message`.
fn reject(message: String) -> js_sys::Promise {
    js_sys::Promise::reject(&JsValue::from_str(&message))
}

/// A promise already settled with `answer`.
fn settled(answer: Result<JsValue, JsValue>) -> js_sys::Promise {
    match answer {
        Ok(value) => js_sys::Promise::resolve(&value),
        Err(error) => js_sys::Promise::reject(&error),
    }
}

/// Assemble a browser device from what the JS world can supply — guest wasm
/// modules and the records they are described by, over the demo defaults;
/// exported to JavaScript as `AroraRuntimeBuilder`. A device that needs a
/// real HAL, bridge, store, or behavior interpreter is composed in Rust with
/// [`arora::AroraBuilder`] and wrapped with `AroraWeb::from` instead: those
/// seams are trait objects that cannot cross the JS boundary.
#[wasm_bindgen(js_name = AroraRuntimeBuilder)]
#[derive(Default)]
pub struct AroraWebBuilder {
    inner: AroraBuilder,
    records: arora::LocalRegistry,
}

#[wasm_bindgen(js_class = "AroraRuntimeBuilder")]
impl AroraWebBuilder {
    #[wasm_bindgen(constructor)]
    pub fn new() -> AroraWebBuilder {
        AroraWebBuilder::default()
    }

    /// Load a guest module into the device's engine — in the browser, a
    /// `.wasm` run by the native `WebAssembly` host. `header_json` is the
    /// module's low-level header as JSON; `executable` its wasm bytes.
    /// Repeatable — each call loads one module.
    #[wasm_bindgen(js_name = withModule)]
    pub fn with_module(&mut self, header_json: &str, executable: &[u8]) -> Result<(), JsValue> {
        let header: Header = serde_json::from_str(header_json)
            .map_err(|e| JsValue::from_str(&format!("invalid header json: {e}")))?;
        let inner = std::mem::take(&mut self.inner);
        self.inner = inner.with_module(header, executable.to_vec());
        Ok(())
    }

    /// Load a guest module compiled ahead into the device's engine, as
    /// `withModule` loads one from its bytes. The device instantiates the
    /// compiled `WebAssembly.Module` instead of compiling the bytes, so devices
    /// built from one `CompiledModule` compile it once between them.
    /// Repeatable — each call loads one module.
    #[wasm_bindgen(js_name = withCompiledModule)]
    pub fn with_compiled_module(&mut self, module: &CompiledModule) {
        let inner = std::mem::take(&mut self.inner);
        self.inner = inner.with_compiled_module(&module.inner);
    }

    /// Add the records guest modules are described by: a module's
    /// `records/` directory as its package ships it — an object mapping each
    /// record file's path under `records/` (`module/<id>@<version>.yaml`,
    /// `structure/…`, `enumeration/…`, `folder/<id>.yaml`) to its YAML.
    /// Repeatable: the records of several modules fill one registry, a
    /// record they share held once.
    ///
    /// A guest module whose record the device holds has every export
    /// described — one taking or returning a structure included — and the
    /// build fails when that record does not match the module's header or
    /// lacks a record it depends on (`AroraBuilder::with_registry`). Fails
    /// here on a path outside that layout, a record that does not read, or a
    /// record another call gave differently under the same id and version;
    /// the records read before a failure stay, so a page builds a new
    /// builder rather than retrying on this one.
    #[wasm_bindgen(js_name = withRecords)]
    pub fn with_records(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "Record<string, string>")] records: JsValue,
    ) -> Result<(), JsValue> {
        use arora_registry::local_yaml::{load_records, RecordKind};
        use futures::FutureExt;

        let files: std::collections::BTreeMap<String, String> =
            serde_wasm_bindgen::from_value(records).map_err(|e| {
                JsValue::from_str(&format!(
                    "records must map each record file's path to its YAML: {e}"
                ))
            })?;
        let mut entries = Vec::with_capacity(files.len());
        for (path, yaml) in &files {
            let record = path.split_once('/').and_then(|(directory, file)| {
                Some((
                    RecordKind::from_directory(directory)?,
                    file.strip_suffix(".yaml")?,
                ))
            });
            let Some((kind, stem)) = record else {
                return Err(JsValue::from_str(&format!(
                    "'{path}' is not a record file's path: <folder|enumeration|structure|module>/<id>[@<version>].yaml"
                )));
            };
            entries.push((kind, stem, yaml.clone()));
        }
        load_records(entries, &mut self.records)
            .now_or_never()
            .ok_or_else(|| JsValue::from_str("the records did not load synchronously"))?
            .map_err(|e| JsValue::from_str(&format!("the records do not load: {e}")))
    }

    /// Build the device. The builder resets to its defaults, ready to
    /// assemble another.
    pub fn build(&mut self) -> Result<AroraWeb, JsValue> {
        let arora = std::mem::take(&mut self.inner)
            .with_registry(std::mem::take(&mut self.records))
            .build()
            .map_err(|e| JsValue::from_str(&format!("arora build failed: {e:?}")))?;
        Ok(AroraWeb::from(arora))
    }
}

/// A guest module compiled once, to load into any number of devices with
/// `AroraRuntimeBuilder.withCompiledModule`: each device instantiates it —
/// its own memory and state — without compiling it again.
#[wasm_bindgen]
pub struct CompiledModule {
    inner: arora::CompiledModule,
}

#[wasm_bindgen]
impl CompiledModule {
    /// Compile a guest module. `header_json` is the module's low-level header
    /// as JSON; `executable` its wasm bytes. Compiles synchronously
    /// (`new WebAssembly.Module`), which Chrome rejects above 8 MB on the main
    /// thread. Fails on an invalid header or an executable that does not
    /// compile.
    #[wasm_bindgen(constructor)]
    pub fn new(header_json: &str, executable: &[u8]) -> Result<CompiledModule, JsValue> {
        install_panic_hook();
        let header: Header = serde_json::from_str(header_json)
            .map_err(|e| JsValue::from_str(&format!("invalid header json: {e}")))?;
        let inner = arora::CompiledModule::new(header, executable)
            .map_err(|e| JsValue::from_str(&format!("module does not compile: {e}")))?;
        Ok(CompiledModule { inner })
    }
}

/// JS-callable handle to a configured Arora engine.
#[wasm_bindgen]
pub struct Engine {
    inner: std::pin::Pin<Box<arora_engine::engine::Engine>>,
    loader: SharedLoaderRc,
    function_module: HashMap<Uuid, Uuid>,
    module_headers: HashMap<Uuid, String>,
}

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Engine {
        install_panic_hook();
        let executor = BrowserExecutor::new();
        let loader = executor.shared();
        let inner = EngineBuilder::new().add_executor(executor).build();
        Engine {
            inner,
            loader,
            function_module: HashMap::new(),
            module_headers: HashMap::new(),
        }
    }

    /// Load a module given its header (as JSON) and executable bytes.
    /// Returns the module's UUID as a string.
    ///
    /// Compiles and instantiates synchronously: Chrome rejects both above
    /// 8 MB on the main thread — use `prepareModule` + `loadPreparedModule`
    /// for large executables.
    #[wasm_bindgen(js_name = loadModule)]
    pub fn load_module(&mut self, header_json: &str, executable: &[u8]) -> Result<String, JsValue> {
        self.load_module_inner(header_json, executable.to_vec().into_boxed_slice())
    }

    /// Asynchronously compile and instantiate a module's executable
    /// (via `WebAssembly.instantiate`, no main-thread size limit).
    /// Follow up with `loadPreparedModule` to complete the load.
    #[wasm_bindgen(js_name = prepareModule)]
    pub fn prepare_module(&self, header_json: &str, executable: Vec<u8>) -> js_sys::Promise {
        prepare_module_impl(self.loader.clone(), header_json, executable)
    }

    /// Load a module whose executable was staged by `prepareModule`.
    /// Returns the module's UUID as a string.
    #[wasm_bindgen(js_name = loadPreparedModule)]
    pub fn load_prepared_module(&mut self, header_json: &str) -> Result<String, JsValue> {
        self.load_module_inner(header_json, Box::new([]))
    }

    fn load_module_inner(
        &mut self,
        header_json: &str,
        executable: Box<[u8]>,
    ) -> Result<String, JsValue> {
        let header: Header = serde_json::from_str(header_json)
            .map_err(|e| JsValue::from_str(&format!("invalid header json: {e}")))?;
        let header_json_str = header_json.to_string();
        let loaded = load_module_from_parts(&mut self.inner, header, executable)
            .map_err(|e| JsValue::from_str(&format!("load_module failed: {e}")))?;
        for fn_id in &loaded.function_ids {
            self.function_module.insert(*fn_id, loaded.id);
        }
        self.module_headers.insert(loaded.id, header_json_str);
        Ok(loaded.id.to_string())
    }

    /// Returns a JSON array of all loaded module headers.
    #[wasm_bindgen(js_name = listModules)]
    pub fn list_modules(&self) -> String {
        let headers: Vec<serde_json::Value> = self
            .module_headers
            .values()
            .filter_map(|s| serde_json::from_str(s).ok())
            .collect();
        serde_json::to_string(&headers).unwrap_or_else(|_| "[]".to_string())
    }

    /// Call a function. `call_json` is a JSON document matching
    /// `arora_engine::call::Call`. Returns the result as a JSON string.
    #[wasm_bindgen]
    pub fn call(&mut self, call_json: &str) -> Result<String, JsValue> {
        let mut call: Call = serde_json::from_str(call_json)
            .map_err(|e| JsValue::from_str(&format!("invalid call json: {e}")))?;
        if call.module_id.is_none() {
            // Resolve locally: the module the function was loaded with.
            call.module_id = Some(
                *self
                    .function_module
                    .get(&call.id)
                    .ok_or_else(|| JsValue::from_str("no module known for function"))?,
            );
        }
        let result = self
            .inner
            .arora_call(call)
            .map_err(|e| JsValue::from_str(&format!("call failed: {e}")))?;
        serde_json::to_string(&result)
            .map_err(|e| JsValue::from_str(&format!("serialize failed: {e}")))
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared implementation of `prepareModule`: parses the header for the
/// module id, then stages an asynchronously-instantiated module in the
/// loader. Returns a `Promise<void>`.
fn prepare_module_impl(
    loader: SharedLoaderRc,
    header_json: &str,
    executable: Vec<u8>,
) -> js_sys::Promise {
    let header: Result<Header, _> = serde_json::from_str(header_json);
    wasm_bindgen_futures::future_to_promise(async move {
        let header = header.map_err(|e| JsValue::from_str(&format!("invalid header json: {e}")))?;
        loader.prepare(header.id, executable).await?;
        Ok(JsValue::UNDEFINED)
    })
}

// =============================================================================
// Behavior-tree runner
//
// A self-contained BT runtime built directly on arora's engine primitives —
// no dependency on the arora-behavior-tree crate.
// =============================================================================

// UUID constants from arora-behavior-tree-types generated code.
// TickId struct id: 6f49e650-84ca-4899-a9bd-1f3bf17fab51
const TICK_ID_STRUCT_BYTES: [u8; 16] = [
    0x6f, 0x49, 0xe6, 0x50, 0x84, 0xca, 0x48, 0x99, 0xa9, 0xbd, 0x1f, 0x3b, 0xf1, 0x7f, 0xab, 0x51,
];
// TickId::callable_id field: 237992d2-17d1-459f-bca1-7185fa6a69d7
const TICK_ID_CALLABLE_FIELD_BYTES: [u8; 16] = [
    0x23, 0x79, 0x92, 0xd2, 0x17, 0xd1, 0x45, 0x9f, 0xbc, 0xa1, 0x71, 0x85, 0xfa, 0x6a, 0x69, 0xd7,
];
// Status::Success variant: 766e9e9a-446d-4e46-83e6-14b7ca101169
const STATUS_SUCCESS_BYTES: [u8; 16] = [
    0x76, 0x6e, 0x9e, 0x9a, 0x44, 0x6d, 0x4e, 0x46, 0x83, 0xe6, 0x14, 0xb7, 0xca, 0x10, 0x11, 0x69,
];
// Status::Failure variant: 2468f46c-bb60-425c-9a4d-9ad326ccc7e2
const STATUS_FAILURE_BYTES: [u8; 16] = [
    0x24, 0x68, 0xf4, 0x6c, 0xbb, 0x60, 0x42, 0x5c, 0x9a, 0x4d, 0x9a, 0xd3, 0x26, 0xcc, 0xc7, 0xe2,
];
// Status enum type: 325a5767-e344-4532-860e-0749bcf2e428
const STATUS_ENUM_BYTES: [u8; 16] = [
    0x32, 0x5a, 0x57, 0x67, 0xe3, 0x44, 0x45, 0x32, 0x86, 0x0e, 0x07, 0x49, 0xbc, 0xf2, 0xe4, 0x28,
];
// _ret special out-parameter: 5f726574-0000-4000-8000-000000000000 ("_ret" in ASCII)
const RET_PARAM_BYTES: [u8; 16] = [
    0x5f, 0x72, 0x65, 0x74, 0x00, 0x00, 0x40, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

fn value_to_status(v: &Value) -> &'static str {
    if let Value::Enumeration(e) = v {
        if *e.variant_id.as_bytes() == STATUS_SUCCESS_BYTES {
            return "success";
        }
        if *e.variant_id.as_bytes() == STATUS_FAILURE_BYTES {
            return "failure";
        }
    }
    "running"
}

/// A node argument expression: either a literal value or a reference to a
/// named variable (identified by UUID).
#[derive(serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
enum BtExpression {
    VariableId(Uuid),
    Value(Value),
}

/// A single BT node – mirrors the arora-behavior-tree YAML schema.
#[derive(serde::Deserialize, Debug)]
struct BtNode {
    id: Uuid,
    function: Uuid,
    #[serde(default)]
    children: Option<Vec<Uuid>>,
    /// Parameter arguments: maps parameter UUID to a literal value or variable.
    /// Use the special key `5f726574-0000-4000-8000-000000000000` (_ret) to
    /// capture the return value into a variable; the node then always succeeds.
    #[serde(default)]
    arguments: HashMap<Uuid, BtExpression>,
}

/// Metadata extracted from a module header for one exported function.
struct FnMeta {
    module_id: Uuid,
    /// Parameter ID of the `children: TickId[]` argument, present only for
    /// control nodes (seq, fallback, parallel …).
    children_param_id: Option<Uuid>,
}

/// An arora Callable that wraps one BT node.
struct NodeCallable {
    node_id: Uuid,
    fn_id: Uuid,
    module_id: Uuid,
    children_param_id: Option<Uuid>,
    children_callable_ids: Vec<u64>,
    arguments: HashMap<Uuid, BtExpression>,
    variables: Rc<RefCell<HashMap<Uuid, Value>>>,
    trace: Rc<RefCell<Vec<(Uuid, &'static str)>>>,
}

impl Callable for NodeCallable {
    fn call(&self, caller: &mut dyn CallBridge) -> Result<Value, arora_engine::call::CallError> {
        let tick_id_type = Uuid::from_bytes(TICK_ID_STRUCT_BYTES);
        let callable_field = Uuid::from_bytes(TICK_ID_CALLABLE_FIELD_BYTES);
        let ret_param_id = Uuid::from_bytes(RET_PARAM_BYTES);

        let mut args = Vec::new();

        if let Some(children_param_id) = self.children_param_id {
            let elements: Vec<StructureWithoutId> = self
                .children_callable_ids
                .iter()
                .map(|&id| StructureWithoutId {
                    fields: vec![StructureField {
                        id: callable_field,
                        value: Box::new(Value::U64(id)),
                    }],
                })
                .collect();
            args.push(StructureField {
                id: children_param_id,
                value: Box::new(Value::ArrayStructure {
                    id: tick_id_type,
                    elements,
                }),
            });
        }

        for (&param_id, expr) in &self.arguments {
            if param_id == ret_param_id {
                continue;
            }
            let value = match expr {
                BtExpression::Value(v) => v.clone(),
                BtExpression::VariableId(var_id) => self
                    .variables
                    .borrow()
                    .get(var_id)
                    .cloned()
                    .unwrap_or(Value::Unit),
            };
            args.push(StructureField {
                id: param_id,
                value: Box::new(value),
            });
        }

        // Build a map of param_id -> variable_id for mutable arguments so we can
        // write mutated values back to the variable store after the call.
        let mutable_param_vars: HashMap<Uuid, Uuid> = self
            .arguments
            .iter()
            .filter_map(|(&param_id, expr)| {
                if param_id == ret_param_id {
                    return None;
                }
                if let BtExpression::VariableId(var_id) = expr {
                    Some((param_id, *var_id))
                } else {
                    None
                }
            })
            .collect();

        let result = caller.arora_call(Call {
            module_id: Some(self.module_id),
            id: self.fn_id,
            args,
        })?;

        // Write back mutated parameter values to bound variables.
        for mutated in &result.mutated {
            if let Some(&var_id) = mutable_param_vars.get(&mutated.id) {
                self.variables
                    .borrow_mut()
                    .insert(var_id, *mutated.value.clone());
            }
        }

        let has_ret = self.arguments.contains_key(&ret_param_id);
        if has_ret {
            if let Some(BtExpression::VariableId(var_id)) = self.arguments.get(&ret_param_id) {
                self.variables
                    .borrow_mut()
                    .insert(*var_id, result.ret.clone());
            }
        }

        let s = if has_ret {
            "success"
        } else {
            value_to_status(&result.ret)
        };
        self.trace.borrow_mut().push((self.node_id, s));

        if has_ret {
            Ok(Value::Enumeration(Enumeration {
                id: Uuid::from_bytes(STATUS_ENUM_BYTES),
                variant_id: Uuid::from_bytes(STATUS_SUCCESS_BYTES),
                value: Box::new(Value::Unit),
            }))
        } else {
            Ok(result.ret)
        }
    }
}

/// Recursively registers callables for `node_id` and all descendants.
/// Returns the callable id of the registered root callable.
fn register_node(
    engine: &mut dyn CallBridge,
    node_id: Uuid,
    node_index: &HashMap<Uuid, BtNode>,
    fn_meta: &HashMap<Uuid, FnMeta>,
    trace: &Rc<RefCell<Vec<(Uuid, &'static str)>>>,
    variables: &Rc<RefCell<HashMap<Uuid, Value>>>,
) -> Result<u64, String> {
    let node = node_index
        .get(&node_id)
        .ok_or_else(|| format!("node {node_id} not found in tree"))?;
    let meta = fn_meta
        .get(&node.function)
        .ok_or_else(|| format!("function {} not registered in fn_meta", node.function))?;

    let children_callable_ids = match &node.children {
        None => vec![],
        Some(ids) => ids
            .iter()
            .map(|&child_id| register_node(engine, child_id, node_index, fn_meta, trace, variables))
            .collect::<Result<Vec<_>, _>>()?,
    };

    let callable: Rc<dyn Callable> = Rc::new(NodeCallable {
        node_id,
        fn_id: node.function,
        module_id: meta.module_id,
        children_param_id: meta.children_param_id,
        children_callable_ids,
        arguments: node.arguments.clone(),
        variables: variables.clone(),
        trace: trace.clone(),
    });
    let id = engine.arora_register_callable(callable);
    Ok(id.id)
}

/// JS-callable handle for loading modules and executing behavior trees.
///
/// Usage:
/// 1. `new BehaviorTreeRunner()`
/// 2. `runner.loadModule(headerJson, wasmBytes)` – can be called for multiple modules
/// 3. `runner.run(nodesJson)` – returns `{status, trace}`
/// 4. Or `runner.setVariable(varId, valueJson)` + `runner.tick(nodesJson)` for
///    stateful tick-by-tick execution with variable bindings.
#[wasm_bindgen]
pub struct BehaviorTreeRunner {
    inner: Pin<Box<arora_engine::engine::Engine>>,
    loader: SharedLoaderRc,
    fn_meta: HashMap<Uuid, FnMeta>,
    variables: Rc<RefCell<HashMap<Uuid, Value>>>,
    module_headers: HashMap<Uuid, String>,
}

#[wasm_bindgen]
impl BehaviorTreeRunner {
    #[wasm_bindgen(constructor)]
    pub fn new() -> BehaviorTreeRunner {
        install_panic_hook();
        let executor = BrowserExecutor::new();
        let loader = executor.shared();
        let inner = EngineBuilder::new().add_executor(executor).build();
        BehaviorTreeRunner {
            inner,
            loader,
            fn_meta: HashMap::new(),
            variables: Rc::new(RefCell::new(HashMap::new())),
            module_headers: HashMap::new(),
        }
    }

    /// Load a WASM module. `header_json` must be the module's YAML header
    /// converted to JSON (the JS side can use js-yaml for that).
    /// Returns the module UUID string.
    ///
    /// Compiles and instantiates synchronously: Chrome rejects both above
    /// 8 MB on the main thread — use `prepareModule` + `loadPreparedModule`
    /// for large executables.
    #[wasm_bindgen(js_name = loadModule)]
    pub fn load_module(&mut self, header_json: &str, executable: &[u8]) -> Result<String, JsValue> {
        self.load_module_inner(header_json, executable.to_vec().into_boxed_slice())
    }

    /// Asynchronously compile and instantiate a module's executable
    /// (via `WebAssembly.instantiate`, no main-thread size limit).
    /// Follow up with `loadPreparedModule` to complete the load.
    #[wasm_bindgen(js_name = prepareModule)]
    pub fn prepare_module(&self, header_json: &str, executable: Vec<u8>) -> js_sys::Promise {
        prepare_module_impl(self.loader.clone(), header_json, executable)
    }

    /// Load a module whose executable was staged by `prepareModule`.
    /// Returns the module UUID string.
    #[wasm_bindgen(js_name = loadPreparedModule)]
    pub fn load_prepared_module(&mut self, header_json: &str) -> Result<String, JsValue> {
        self.load_module_inner(header_json, Box::new([]))
    }

    fn load_module_inner(
        &mut self,
        header_json: &str,
        executable: Box<[u8]>,
    ) -> Result<String, JsValue> {
        let header: Header = serde_json::from_str(header_json)
            .map_err(|e| JsValue::from_str(&format!("invalid header: {e}")))?;
        let module_id = header.id;
        let header_json_str = header_json.to_string();

        for export in &header.exports {
            let arora_types::module::low::ExportSymbol::Function(f) = export;
            let children_param_id = f.parameters.first().and_then(|p| {
                if let arora_types::module::low::TypeRef::Array { id } = &p.ty {
                    if id == &Uuid::from_bytes(TICK_ID_STRUCT_BYTES) {
                        Some(p.id)
                    } else {
                        None
                    }
                } else {
                    None
                }
            });

            self.fn_meta.insert(
                f.id,
                FnMeta {
                    module_id,
                    children_param_id,
                },
            );
        }

        let result = load_module_from_parts(&mut self.inner, header, executable)
            .map_err(|e| JsValue::from_str(&format!("load failed: {e}")))?;
        self.module_headers.insert(result.id, header_json_str);
        Ok(result.id.to_string())
    }

    /// Returns a JSON array of all loaded module headers.
    #[wasm_bindgen(js_name = listModules)]
    pub fn list_modules(&self) -> String {
        let headers: Vec<serde_json::Value> = self
            .module_headers
            .values()
            .filter_map(|s| serde_json::from_str(s).ok())
            .collect();
        serde_json::to_string(&headers).unwrap_or_else(|_| "[]".to_string())
    }

    /// Initialize or update a variable. `var_id` is a UUID string; `value_json`
    /// is the serialized `Value` (e.g. `{"f32": 0.0}`).
    #[wasm_bindgen(js_name = setVariable)]
    pub fn set_variable(&mut self, var_id: &str, value_json: &str) -> Result<(), JsValue> {
        let var_id: Uuid = var_id
            .parse()
            .map_err(|_| JsValue::from_str("bad var_id: not a valid UUID"))?;
        let value: Value = serde_json::from_str(value_json)
            .map_err(|e| JsValue::from_str(&format!("bad value JSON: {e}")))?;
        self.variables.borrow_mut().insert(var_id, value);
        Ok(())
    }

    /// Run one tick of the behavior tree. Variables persist across calls.
    ///
    /// `nodes_json` is a JSON array where each element is:
    ///   `{ id, function, children?, arguments?, return_binding? }`
    ///
    /// Returns: `{ "status": "...", "trace": [...], "variables": { varId: value } }`
    pub fn tick(&mut self, nodes_json: &str) -> Result<String, JsValue> {
        let nodes: Vec<BtNode> = serde_json::from_str(nodes_json)
            .map_err(|e| JsValue::from_str(&format!("bad nodes JSON: {e}")))?;
        if nodes.is_empty() {
            return Err(JsValue::from_str("tree has no nodes"));
        }

        let root_id = nodes[0].id;
        let node_index: HashMap<Uuid, BtNode> = nodes.into_iter().map(|n| (n.id, n)).collect();
        let trace: Rc<RefCell<Vec<(Uuid, &'static str)>>> = Rc::new(RefCell::new(Vec::new()));

        let fn_meta = &self.fn_meta;
        let root_callable_id = register_node(
            &mut *self.inner,
            root_id,
            &node_index,
            fn_meta,
            &trace,
            &self.variables,
        )
        .map_err(|e| JsValue::from_str(&format!("setup error: {e}")))?;

        let callable_id = CallableId {
            id: root_callable_id,
        };
        let result = Callable::call(&callable_id, &mut *self.inner)
            .map_err(|e| JsValue::from_str(&format!("tick error: {e}")))?;
        let status = value_to_status(&result);

        let trace_json: Vec<serde_json::Value> = trace
            .borrow()
            .iter()
            .map(|(id, s)| serde_json::json!({ "nodeId": id.to_string(), "status": s }))
            .collect();

        let vars_json: serde_json::Map<String, serde_json::Value> = self
            .variables
            .borrow()
            .iter()
            .filter_map(|(id, v)| serde_json::to_value(v).ok().map(|jv| (id.to_string(), jv)))
            .collect();

        Ok(serde_json::json!({
          "status": status,
          "trace": trace_json,
          "variables": vars_json,
        })
        .to_string())
    }

    /// Run a behavior tree to completion (ticks until not Running).
    ///
    /// `nodes_json` is a JSON array where each element is:
    ///   `{ id: "<uuid>", function: "<uuid>", children?: ["<uuid>", ...] }`
    /// The first element is the root node.
    ///
    /// Returns a JSON string:
    ///   `{ "status": "success"|"failure"|"running",
    ///      "trace": [{"nodeId": "<uuid>", "status": "..."}] }`
    pub fn run(&mut self, nodes_json: &str) -> Result<String, JsValue> {
        let nodes: Vec<BtNode> = serde_json::from_str(nodes_json)
            .map_err(|e| JsValue::from_str(&format!("bad nodes JSON: {e}")))?;
        if nodes.is_empty() {
            return Err(JsValue::from_str("tree has no nodes"));
        }

        let root_id = nodes[0].id;
        let node_index: HashMap<Uuid, BtNode> = nodes.into_iter().map(|n| (n.id, n)).collect();
        let trace: Rc<RefCell<Vec<(Uuid, &'static str)>>> = Rc::new(RefCell::new(Vec::new()));

        let fn_meta = &self.fn_meta;
        let root_callable_id = register_node(
            &mut *self.inner,
            root_id,
            &node_index,
            fn_meta,
            &trace,
            &self.variables,
        )
        .map_err(|e| JsValue::from_str(&format!("setup error: {e}")))?;

        let callable_id = CallableId {
            id: root_callable_id,
        };

        let mut last_status = "running";
        for _ in 0..10_000 {
            let result = Callable::call(&callable_id, &mut *self.inner)
                .map_err(|e| JsValue::from_str(&format!("tick error: {e}")))?;
            last_status = value_to_status(&result);
            if last_status != "running" {
                break;
            }
        }

        let trace_json: Vec<serde_json::Value> = trace
            .borrow()
            .iter()
            .map(|(id, s)| serde_json::json!({ "nodeId": id.to_string(), "status": s }))
            .collect();

        let out = serde_json::json!({
          "status": last_status,
          "trace": trace_json,
        });
        Ok(out.to_string())
    }
}

impl Default for BehaviorTreeRunner {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Type registry
//
// A minimal wasm-compatible registry for resolving type UUIDs to names.
// Loads from the records.json files emitted by arora-module-rust::generate_records.
// =============================================================================

/// JS-callable type registry that resolves type UUIDs to human-readable names.
#[wasm_bindgen]
pub struct Registry {
    entries: HashMap<Uuid, (String, Option<Uuid>)>,
}

#[derive(serde::Deserialize)]
struct RecordEntry {
    id: Uuid,
    name: String,
    parent: Option<Uuid>,
}

#[wasm_bindgen]
impl Registry {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Registry {
        Registry {
            entries: HashMap::new(),
        }
    }

    /// Load type records from a JSON array: `[{id, name, parent?}, ...]`.
    #[wasm_bindgen(js_name = loadRecordsJson)]
    pub fn load_records_json(&mut self, json: &str) -> Result<(), JsValue> {
        let records: Vec<RecordEntry> = serde_json::from_str(json)
            .map_err(|e| JsValue::from_str(&format!("invalid records JSON: {e}")))?;
        for r in records {
            self.entries.insert(r.id, (r.name, r.parent));
        }
        Ok(())
    }

    /// Resolve a UUID string to a dot-separated name path (e.g. `"arora.Status"`).
    /// Returns `None` if the UUID is not known.
    #[wasm_bindgen(js_name = resolveId)]
    pub fn resolve_id(&self, id: &str) -> Option<String> {
        let uuid: Uuid = id.parse().ok()?;
        self.compute_path(&uuid)
    }

    fn compute_path(&self, id: &Uuid) -> Option<String> {
        let (name, parent) = self.entries.get(id)?;
        match parent {
            None => Some(name.clone()),
            Some(parent_id) if !self.entries.contains_key(parent_id) => Some(name.clone()),
            Some(parent_id) => Some(format!("{}.{}", self.compute_path(parent_id)?, name)),
        }
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

/// What a page has not drained stays one entry per changed key, whichever
/// way the device steps. Run in a browser with `wasm-pack test --headless
/// --firefox crates/arora-web --lib`.
#[cfg(test)]
mod undrained_tests {
    use super::*;
    use wasm_bindgen_futures::JsFuture;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    /// The keys a busy device changes on each of its steps.
    const KEYS: usize = 200;

    /// Write every one of the [`KEYS`] to `value`, as one store change.
    fn frame(store: &dyn DataStore, value: f32) {
        let mut change = StateChange::new();
        for key in 0..KEYS {
            change
                .set
                .insert(Key::new(format!("face/{key}")), Some(Value::F32(value)));
        }
        store.write(change).expect("write a frame");
    }

    /// A demo device and a sibling handle of its store, through which a test
    /// writes as the device's own writers do.
    fn device() -> (AroraWeb, Box<dyn DataStore>) {
        let arora = Arora::builder().build().expect("build the device");
        let store = arora.store().clone_box();
        (AroraWeb::from(arora), store)
    }

    /// Whether the subscription's channel is empty: every change it received
    /// has been folded.
    fn channel_is_empty(web: &AroraWeb) -> bool {
        web.changes.borrow().subscription.try_recv().is_none()
    }

    /// The value the backlog holds for `face/{key}`.
    fn held(web: &AroraWeb, key: usize) -> Option<Value> {
        web.changes
            .borrow()
            .latest
            .get(&Key::new(format!("face/{key}")))
            .cloned()
            .flatten()
    }

    async fn sleep(ms: i32) {
        let timer = js_sys::Promise::new(&mut |resolve, _| {
            let set_timeout: js_sys::Function =
                js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("setTimeout"))
                    .expect("a global setTimeout")
                    .into();
            set_timeout
                .call2(&JsValue::NULL, &resolve, &JsValue::from(ms))
                .expect("setTimeout");
        });
        JsFuture::from(timer).await.expect("the timer fires");
    }

    #[wasm_bindgen_test]
    fn stepping_without_a_drain_holds_one_value_per_key() {
        let (web, store) = device();
        for step in 0..1_000 {
            frame(&*store, step as f32);
            web.step(10.0).expect("step");
            assert!(channel_is_empty(&web), "step {step} left changes unfolded");
        }
        let keys = store.snapshot().storage.len();
        assert!(keys >= KEYS, "the store holds the frame's keys");
        assert!(
            web.changes.borrow().latest.len() <= keys,
            "the backlog holds no more than one entry per key"
        );
        assert_eq!(held(&web, 7), Some(Value::F32(999.0)));
    }

    #[wasm_bindgen_test]
    async fn a_running_device_folds_every_step() {
        let (web, store) = device();
        let running = web.run(Some(1.0));
        for step in 0..50 {
            frame(&*store, step as f32);
            // Several periods: the loop steps, and folds, before this resumes.
            sleep(5).await;
            assert!(channel_is_empty(&web), "step {step} left changes unfolded");
            assert_eq!(held(&web, 0), Some(Value::F32(step as f32)));
        }
        web.stop();
        JsFuture::from(running).await.expect("the run stops");
    }

    #[wasm_bindgen_test]
    fn a_page_write_is_folded_at_once() {
        let (web, _store) = device();
        web.drain_changes().expect("the first drain");
        for value in 0..100 {
            web.set_value("face/0", &format!(r#"{{"f32": {value}}}"#))
                .expect("set a key");
            assert!(channel_is_empty(&web));
        }
        assert_eq!(held(&web, 0), Some(Value::F32(99.0)));
    }
}
