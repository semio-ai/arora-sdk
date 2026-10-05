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

use arora::{Arora, AroraBuilder, Caller, Invoked, LocalCaller, TaskId};
use arora_bridge::client::{self, KeyInfo};
use arora_types::data::{DataStore, Key, StateChange, Subscription};
use std::future::Future;
use std::time::Duration;

/// Hand `future` to JavaScript as a Promise, having polled it once already.
///
/// Every device operation goes on the device's inbound queue at its future's
/// first poll; a `future_to_promise` alone would first poll it a microtask
/// later. Polling once here means the operation is queued before the method
/// returns, so a page that calls and then `step()`s in the same turn has it
/// applied by that step. A future that is already done settles the promise
/// at once.
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

    /// Drain the changes the subscription accumulated, as a JS object mapping
    /// path → new Arora [`Value`] (or `null` for a cleared key). Poll-based
    /// counterpart to a push subscription: [`DataStore::subscribe`] delivers
    /// over a std channel JavaScript cannot await, so changes accumulate and
    /// are handed over on demand. A subscription opens on the store's whole
    /// current state, so the first drain returns the full picture.
    pub fn drain_changes(changes: &Subscription) -> Result<JsValue, JsValue> {
        let mut out = serde_json::Map::new();
        while let Some(change) = changes.try_recv() {
            for (key, value) in change.set {
                out.insert(key.path, value_to_json(value)?);
            }
            for key in change.unset {
                out.insert(key.path, serde_json::Value::Null);
            }
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
/// Either way the rest of the surface stays
/// live: `setValue`/`readValues`/`snapshot` work on a sibling handle of the
/// store, `drainChanges` on its subscription, and the client operations —
/// `call`, `invoke`, `spawn`, `halt`, `listKeys`, `describeMethods` — on the
/// device's in-process [`LocalCaller`], the same operations a remote client has
/// over a bridge. None of them touch the stepping device: each is queued
/// before the method returns and applied by the next step, so the device must
/// be stepping (a `run()` loop, or your own `step` calls) for its promise to
/// settle.
///
/// Every method is an ordinary Rust method too, for a Rust crate that wraps
/// this device in its own `wasm_bindgen` type; [`caller`](Self::caller) hands
/// such a crate the typed operations.
#[wasm_bindgen(js_name = AroraRuntime)]
pub struct AroraWeb {
    // The device parks here between manual steps; `run` takes it out and
    // `stop` puts it back. Shared with the run future, which returns the
    // device on a stop.
    device: Rc<RefCell<Option<Arora>>>,
    // Ends the current `run` loop at its next step boundary; `None` while no
    // loop runs (the sender is consumed by `stop`).
    stop: RefCell<Option<futures::channel::oneshot::Sender<()>>>,
    caller: LocalCaller,
    store: Box<dyn DataStore>,
    changes: Subscription,
    // The device's standing-error watch, taken before `run` can own the
    // device, so the reading stays available while it self-paces.
    behavior_error: tokio::sync::watch::Receiver<Option<String>>,
    // The awaitable end of the same watch: taken out for the duration of one
    // `behaviorErrorChanged` await, so sequential awaits share one seen
    // cursor and no change is missed between them.
    behavior_error_changes: Rc<RefCell<Option<tokio::sync::watch::Receiver<Option<String>>>>>,
}

impl From<Arora> for AroraWeb {
    /// Wrap a composed device: keep its in-process [`LocalCaller`], a sibling
    /// handle of its store, and a subscription (opening on the whole current
    /// state), then expose it all to JavaScript.
    fn from(arora: Arora) -> Self {
        install_panic_hook();
        let caller = arora.caller();
        let store = arora.store().clone_box();
        let changes = store.subscribe();
        let behavior_error = arora.behavior_error();
        let behavior_error_changes = Rc::new(RefCell::new(Some(arora.behavior_error())));
        Self {
            device: Rc::new(RefCell::new(Some(arora))),
            stop: RefCell::new(None),
            caller,
            store,
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
    /// Unavailable once [`run`](Self::run) has taken the device.
    pub fn step(&self, dt_ms: f64) -> Result<(), JsValue> {
        let mut slot = self
            .device
            .try_borrow_mut()
            .map_err(|_| JsValue::from_str("the device is mid-step; call between steps"))?;
        let device = slot
            .as_mut()
            .ok_or_else(|| JsValue::from_str("run() drives the device; step() is unavailable"))?;
        device
            .step(Duration::from_secs_f64(dt_ms / 1_000.0))
            .map_err(|e| JsValue::from_str(&format!("step failed: {e}")))
    }

    /// Hand the device to [`Arora::run`]: a self-paced loop at `period_ms`
    /// (default: the runtime's ~100 Hz) that owns the device until stepping
    /// fails — the promise rejects — or [`stop`](Self::stop) reclaims it — the
    /// promise resolves and `step()` works again. While it runs the rest of
    /// the surface keeps working: it never touches the stepping device.
    ///
    /// `run` awaits between steps, so it shares its thread; see
    /// [`Arora::run`] for where a cadence survives in a browser (main thread
    /// vs dedicated worker).
    pub fn run(&self, period_ms: Option<f64>) -> js_sys::Promise {
        let Some(mut device) = self.device.borrow_mut().take() else {
            return js_sys::Promise::reject(&JsValue::from_str("the device is already running"));
        };
        let slot = self.device.clone();
        let (stop_tx, stop_rx) = futures::channel::oneshot::channel::<()>();
        *self.stop.borrow_mut() = Some(stop_tx);
        wasm_bindgen_futures::future_to_promise(async move {
            let period = period_ms
                .map(|ms| Duration::from_secs_f64(ms / 1_000.0))
                .unwrap_or(Arora::DEFAULT_STEP_PERIOD);
            let outcome = {
                use futures::FutureExt;
                let run = device.run(period).fuse();
                let mut stop_rx = stop_rx.fuse();
                futures::pin_mut!(run);
                futures::select_biased! {
                    result = run => Some(result),
                    _ = stop_rx => None,
                }
            };
            // Whatever ended the loop, the device parks back in its slot: a
            // failed run leaves a device the embedder can still step, probe
            // ([`Arora::behavior_error`]), or run again.
            slot.borrow_mut().replace(device);
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
        self.device.borrow().is_none()
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
        let slot = self.behavior_error_changes.clone();
        wasm_bindgen_futures::future_to_promise(async move {
            let outcome = errors.changed().await;
            let value = errors.borrow_and_update().clone();
            slot.borrow_mut().replace(errors);
            outcome.map_err(|_| JsValue::from_str("the device is gone"))?;
            Ok(match value {
                Some(message) => JsValue::from_str(&message),
                None => JsValue::UNDEFINED,
            })
        })
    }

    /// Dispatch a [`Call`] into the device through its in-process caller. The
    /// call is queued before this returns, and the promise resolves after the
    /// step that applies it — the device must be stepping (a `run()` loop, or
    /// your own `step` calls) for it to land. `call_json` is an
    /// `arora_types::call::Call` as JSON; the result is the `CallResult` as a
    /// JSON string.
    pub fn call(&self, call_json: &str) -> js_sys::Promise {
        let parsed: Result<Call, _> = serde_json::from_str(call_json)
            .map_err(|e| JsValue::from_str(&format!("invalid call json: {e}")));
        let caller = self.caller.clone();
        promise(async move {
            let result = caller
                .call(parsed?)
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
    /// spawned as a run instead, and resolves at once to the run's handle, the
    /// plain object [`spawn`](Self::spawn) answers with.
    ///
    /// Names are the bare names modules export, so two modules may share one:
    /// `module_id` (a uuid string) names the exporting module to choose. A name
    /// more than one module exports, without `module_id`, rejects naming those
    /// modules. An argument the method does not declare rejects too.
    ///
    /// Queued before this returns, like every operation; it reads the method's
    /// signature on the next step and applies its call on the step after.
    pub fn invoke(
        &self,
        method: &str,
        args_json: &str,
        module_id: Option<String>,
    ) -> js_sys::Promise {
        let args: Result<HashMap<String, Value>, _> = serde_json::from_str(args_json)
            .map_err(|e| JsValue::from_str(&format!("invalid args json: {e}")));
        let module = module_id
            .map(|id| {
                Uuid::parse_str(&id)
                    .map_err(|e| JsValue::from_str(&format!("'{id}' is not a module id: {e}")))
            })
            .transpose();
        let caller = self.caller.clone();
        let method = method.to_string();
        promise(async move {
            let invoked = caller
                .invoke(&method, args?, module?)
                .await
                .map_err(|e| JsValue::from_str(&format!("invoke {method} failed: {e}")))?;
            match invoked {
                Invoked::Returned(value) => value_to_js(&value),
                Invoked::Started(handle) => run_to_js(&handle),
            }
        })
    }

    /// Start `call_json` (an `arora_types::call::Call` as JSON) as a task run,
    /// concurrently with every other run. The promise resolves to the run's
    /// handle, a plain object `{run, status, feedback, result, update}`: `run`
    /// the run id (a uuid string, which [`halt`](Self::halt) takes), `status`
    /// the path of the key that reports how the run goes and ends, and
    /// `feedback`, `result` and `update` the paths of the keys carrying its
    /// progress, its result, and what steers it. Queued before this returns;
    /// applied by the next step.
    pub fn spawn(&self, call_json: &str) -> js_sys::Promise {
        let parsed: Result<Call, _> = serde_json::from_str(call_json)
            .map_err(|e| JsValue::from_str(&format!("invalid call json: {e}")));
        let caller = self.caller.clone();
        promise(async move {
            let handle = caller
                .spawn(parsed?)
                .await
                .map_err(|e| JsValue::from_str(&format!("spawn failed: {e}")))?;
            run_to_js(&handle)
        })
    }

    /// Stop the run `run_id` names (the `run` of its handle, a uuid string):
    /// it ends `Failure` on the step after the halt is applied. Idempotent —
    /// halting a finished or unknown run resolves all the same. Queued before
    /// this returns; applied by the next step.
    pub fn halt(&self, run_id: &str) -> js_sys::Promise {
        let run = Uuid::parse_str(run_id)
            .map_err(|e| JsValue::from_str(&format!("'{run_id}' is not a run id: {e}")));
        let caller = self.caller.clone();
        promise(async move {
            caller
                .halt(TaskId(run?))
                .await
                .map_err(|e| JsValue::from_str(&format!("halt failed: {e}")))?;
            Ok(JsValue::UNDEFINED)
        })
    }

    /// The device's keys whose path starts with `prefix` (all of them when
    /// absent), sorted by path. The promise resolves to an array of
    /// `{path, __meta}` objects: each key with what its store says it is — the
    /// shape it holds, its range and unit, where it rests, whether a client may
    /// write it — as every Arora bridge lists keys. Queued before this returns;
    /// applied by the next step.
    #[wasm_bindgen(js_name = listKeys)]
    pub fn list_keys(&self, prefix: Option<String>) -> js_sys::Promise {
        let listed = self.caller.list_keys(prefix);
        promise(async move {
            let keys: Vec<KeyInfo> = listed
                .await
                .map_err(|e| JsValue::from_str(&format!("listKeys failed: {e}")))?
                .into_iter()
                .map(|(path, meta)| KeyInfo { path, meta })
                .collect();
            to_js(&keys)
        })
    }

    /// The device's callable methods whose name starts with `prefix` (all of
    /// them when absent), sorted by name. The promise resolves to an array of
    /// method descriptions, as a bridge lists methods — `path` (the name),
    /// `params` (each `name`, `param_type`, `required`, in declaration order),
    /// `return_type`, `task` (whether invoking it starts a run) — plus `module`,
    /// the exporting module's id, which [`invoke`](Self::invoke) takes when two
    /// modules export one name. Queued before this returns; applied by the next
    /// step.
    #[wasm_bindgen(js_name = describeMethods)]
    pub fn describe_methods(&self, prefix: Option<String>) -> js_sys::Promise {
        let described = self.caller.describe_methods(prefix);
        promise(async move {
            let signatures = described
                .await
                .map_err(|e| JsValue::from_str(&format!("describeMethods failed: {e}")))?;
            let mut methods = Vec::with_capacity(signatures.len());
            for signature in &signatures {
                let mut method = serde_json::to_value(client::method_info(signature))
                    .map_err(|e| JsValue::from_str(&format!("serialize failed: {e}")))?;
                method["module"] = serde_json::Value::String(signature.module_id.to_string());
                methods.push(method);
            }
            to_js(&methods)
        })
    }

    /// Write one key into the store (Arora [`Value`] as JSON, e.g. `{"f32": 1}`).
    #[wasm_bindgen(js_name = setValue)]
    pub fn set_value(&self, path: &str, value_json: &str) -> Result<(), JsValue> {
        store_json::set_value(&*self.store, path, value_json)
    }

    /// Write several keys at once, as one store change (a JSON object mapping
    /// each key path to an Arora [`Value`]).
    #[wasm_bindgen(js_name = writeValues)]
    pub fn write_values(&self, values_json: &str) -> Result<(), JsValue> {
        store_json::write_values(&*self.store, values_json)
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
    /// (`null` for a cleared key). The first drain returns the store's whole
    /// current state — the subscription opens on it.
    #[wasm_bindgen(js_name = drainChanges)]
    pub fn drain_changes(&self) -> Result<JsValue, JsValue> {
        store_json::drain_changes(&self.changes)
    }
}

impl AroraWeb {
    /// The device's in-process caller: the typed operations behind `call`,
    /// `invoke`, `spawn`, `halt`, `listKeys` and `describeMethods`, for a Rust
    /// crate that wraps this device and wants them unserialized.
    pub fn caller(&self) -> LocalCaller {
        self.caller.clone()
    }
}

/// Assemble a browser device from what the JS world can supply — guest wasm
/// modules over the demo defaults; exported to JavaScript as
/// `AroraRuntimeBuilder`. A device that needs a real HAL, bridge, store, or
/// behavior interpreter is composed in Rust with [`arora::AroraBuilder`] and
/// wrapped with `AroraWeb::from` instead: those seams are trait objects that
/// cannot cross the JS boundary.
#[wasm_bindgen(js_name = AroraRuntimeBuilder)]
#[derive(Default)]
pub struct AroraWebBuilder {
    inner: AroraBuilder,
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

    /// Build the device. The builder resets to its defaults, ready to
    /// assemble another.
    pub fn build(&mut self) -> Result<AroraWeb, JsValue> {
        let arora = std::mem::take(&mut self.inner)
            .build()
            .map_err(|e| JsValue::from_str(&format!("arora build failed: {e:?}")))?;
        Ok(AroraWeb::from(arora))
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

    /// Resolve a UUID string to a dot-separated name path (e.g. `"behavior_tree.Status"`).
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
