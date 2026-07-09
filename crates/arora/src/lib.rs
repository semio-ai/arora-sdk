//! Opinionated Arora runtime.
//!
//! Where [`arora_engine`] is the bare, unopinionated runtime, this crate wires
//! a ready-to-use [`Arora`]: the whole device in one object — the engine (with
//! the WebAssembly and native executors), the shared data store, the HAL and
//! bridge I/O seams, one behavior interpreter, and the step loop that drives
//! them. The basic behavior-tree control nodes are wired natively into
//! [`arora_behavior_tree`], so no module needs to be loaded to run a tree of
//! them.
//!
//! Build one with the [`builder`](Arora::builder): pick a data store, a HAL, one
//! or more bridges, the modules whose functions behaviors may call, and the
//! behavior interpreter — each with a sensible default, so
//! `Arora::builder().build()` yields a self-contained in-process device (fake
//! HAL, in-process bridge, an empty behavior, a private [`SimpleDataStore`]).
//! Then drive it with [`step`](Arora::step) (once per frame) or
//! [`run`](Arora::run) (the visible loop over `step`).

#[cfg(feature = "native")]
pub mod operator;
mod run;
pub mod runtime;
#[cfg(feature = "studio-bridge")]
mod studio;
/// The terminal operator UI. Native, and only when the `tui` feature is on; an
/// embedder that brings its own UI builds without it.
#[cfg(feature = "tui")]
pub mod tui;

#[cfg(feature = "native")]
pub use run::{run, run_with, run_with_frontend, run_with_hal};
pub use runtime::{RuntimeError, StepOutcome, Telemetry, TelemetrySnapshot};

/// Re-exported so embedders can construct the default behavior executor — an
/// empty, ready [`BehaviorTreeInterpreter`] — and load a behavior into it before
/// injecting it with [`AroraBuilder::with_behavior_interpreter`].
pub use arora_behavior_tree::behavior::BehaviorTreeInterpreter;

use anyhow::Result;
use arora_behavior::BehaviorInterpreter;
use arora_behavior_tree::ModuleFunction;
use arora_bridge::{Bridge, FakeBridge};
use arora_engine::engine::{EngineBuilder, PinnedEngine};
#[cfg(feature = "native")]
use arora_engine::executor::{native::NativeExecutor, wasm::WebAssemblyExecutor};
use arora_hal::{FakeHal, Hal};
use arora_simple_data_store::SimpleDataStore;
use arora_types::data::{DataStore, Subscription};
use runtime::Clock;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use uuid::Uuid;

/// An opinionated Arora device: the engine (with the basic behavior-tree control
/// nodes wired natively) plus everything a running device needs — the shared
/// data store, the HAL and bridge I/O seams, one behavior interpreter, and the
/// clock — advanced one [`step`](Arora::step) at a time.
///
/// `Arora` is the single owner of the state. Several things want to change the
/// blackboard — the bridge (commands/state from the remote), the HAL (sensor
/// readings), and the behavior (intent it writes while ticking) — and rather
/// than share the state behind a lock and race, `Arora` serializes them as
/// phases of one [`step`](Arora::step). It owns no async runtime and spawns no
/// threads: it pokes its I/O seams through their synchronous poll/push surface,
/// and any real async work lives *inside* those implementations. That is also
/// why it drops unchanged into a Web Worker — the worker boundary is the seam's
/// problem, not `Arora`'s.
///
/// Build one with [`Arora::builder`].
pub struct Arora {
    // Owned and touched ONLY by the stepping thread (single-threaded state).
    // Held behind `dyn DataStore` so a wrapping store (e.g. a `NamespacedStore`
    // over one mutualized backend) can be injected via the builder.
    pub(crate) store: Arc<dyn DataStore>,
    pub(crate) engine: PinnedEngine,
    /// Module functions referenced by behavior-tree nodes, keyed by function
    /// UUID. The basic control nodes are dispatched natively and are not in this
    /// index; it holds only the functions of modules registered through
    /// [`AroraBuilder::with_module`].
    pub(crate) function_index: Rc<HashMap<Uuid, ModuleFunction>>,
    /// The one behavior interpreter, ticked each step — an executor injected once
    /// at [`build`](AroraBuilder::build), not swapped afterwards. It defaults to
    /// an empty, ready [`BehaviorTreeInterpreter`] (see
    /// [`with_behavior_interpreter`](AroraBuilder::with_behavior_interpreter));
    /// a behavior is loaded *into* it as a separate step. `None` means nothing to
    /// tick; the interpreter is dropped back to `None` once it reports
    /// [`BehaviorStatus::Done`](arora_behavior::BehaviorStatus).
    pub(crate) interpreter: Option<Box<dyn BehaviorInterpreter>>,
    pub(crate) telemetry: Telemetry,
    // The synchronous I/O seams the step drives directly. Each owns its own
    // async internally; `Arora` only pokes their non-blocking poll/push.
    pub(crate) hal: Arc<dyn Hal>,
    // A Vec even though exactly one is expected today: reads fan in and writes
    // fan out over all of them, so several can be added later (see PR 3) with no
    // shape change.
    pub(crate) bridges: Vec<Arc<dyn Bridge>>,
    // The HAL's sensor feed, a sync subscription the step drains each frame.
    pub(crate) hal_updates: Subscription,
    pub(crate) store_changes: Subscription,
    // The golden clock: monotonic nanoseconds since start, advanced by each
    // step's `dt`. Published into the store's golden keys each step, before any
    // behavior ticks; the flush phase filters the golden namespace out of what
    // it forwards outbound.
    pub(crate) clock: Clock,
}

impl Arora {
    /// Start assembling an [`Arora`]. Every seam has a default, so the shortest
    /// device is `Arora::builder().build()`.
    pub fn builder() -> AroraBuilder {
        AroraBuilder::default()
    }
}

/// Assembles an [`Arora`] from its seams, each defaulted so only what differs
/// from the in-process default device has to be named. Every setter returns
/// `self`, so calls chain; [`build`](AroraBuilder::build) wires the engine and
/// the store subscriptions and returns the finished [`Arora`].
#[derive(Default)]
pub struct AroraBuilder {
    store: Option<Arc<dyn DataStore>>,
    hal: Option<Arc<dyn Hal>>,
    bridges: Vec<Arc<dyn Bridge>>,
    interpreter: Option<Box<dyn BehaviorInterpreter>>,
    functions: HashMap<Uuid, ModuleFunction>,
}

impl AroraBuilder {
    /// Use `store` as the device blackboard. Takes the trait object, so the
    /// caller chooses the backend: a plain shared [`SimpleDataStore`], or a
    /// wrapping store such as a `NamespacedStore` that prefixes every key with a
    /// device namespace before delegating to one mutualized backend (how Studio
    /// mutualizes one store across every spawned device). Default: a fresh,
    /// private [`SimpleDataStore`].
    pub fn with_data_store(mut self, store: Arc<dyn DataStore>) -> Self {
        self.store = Some(store);
        self
    }

    /// Use `hal` as the hardware abstraction layer — exactly one per device.
    /// Default: an in-process [`FakeHal`].
    pub fn with_hal(mut self, hal: Arc<dyn Hal>) -> Self {
        self.hal = Some(hal);
        self
    }

    /// Add a bridge. Repeatable: reads fan in from every bridge and writes fan
    /// out to every bridge (PR 3), so several remotes can observe/command one
    /// device. Default (when none is added): an in-process [`FakeBridge`].
    pub fn with_bridge(mut self, bridge: Arc<dyn Bridge>) -> Self {
        self.bridges.push(bridge);
        self
    }

    /// Inject the behavior interpreter the device ticks — the one executor, set
    /// once here and not swapped afterwards. An interpreter is constructed empty
    /// and ready; a behavior is loaded *into* it as a separate step (e.g.
    /// [`BehaviorTreeInterpreter::load_groot`]) before it is handed here. Default
    /// (when none is injected): an empty [`BehaviorTreeInterpreter`] over the
    /// assembled function index, so the device idles (each tick a no-op) until a
    /// behavior is loaded.
    pub fn with_behavior_interpreter(mut self, interpreter: Box<dyn BehaviorInterpreter>) -> Self {
        self.interpreter = Some(interpreter);
        self
    }

    /// Register a module's functions so behaviors may call them. Repeatable —
    /// each call adds one module's functions to the `function_index`, keyed by
    /// function UUID.
    ///
    /// This makes real the function-index half of the module-load seam: a
    /// behavior-tree node bound to one of these functions builds its call from
    /// the frozen `Function` record carried here. The remaining half — loading
    /// the module's executable into the engine so the call actually dispatches
    /// to guest code — is not wired yet (a [`ModuleFunction`] carries no
    /// executable), so registered functions must be ones the engine can already
    /// dispatch (natively-hosted). TODO: accept a loadable module (header +
    /// executable) and load it into the engine here too.
    pub fn with_module(mut self, functions: impl IntoIterator<Item = ModuleFunction>) -> Self {
        for function in functions {
            self.functions.insert(function.function_id, function);
        }
        self
    }

    /// Wire the engine, apply the defaults for any unset seam, subscribe to the
    /// HAL and store feeds, and return the finished [`Arora`].
    ///
    /// Fails only if the engine's executor host cannot be created. Fully
    /// synchronous: there is no I/O pump to spawn — the HAL and bridges own any
    /// async internally, behind their synchronous seams.
    pub fn build(self) -> Result<Arora> {
        let engine = build_engine()?;
        let store = self
            .store
            .unwrap_or_else(|| Arc::new(SimpleDataStore::new()));
        let hal: Arc<dyn Hal> = self.hal.unwrap_or_else(|| Arc::new(FakeHal::new()));
        let bridges = if self.bridges.is_empty() {
            vec![Arc::new(FakeBridge::new()) as Arc<dyn Bridge>]
        } else {
            self.bridges
        };
        let store_changes = store.subscribe();
        let hal_updates = hal.updates();

        let function_index = Rc::new(self.functions);
        // Default executor: an empty, ready behavior-tree interpreter over the
        // assembled function index. It is injected once here (never swapped); a
        // behavior is loaded into it as a separate step. With none loaded it
        // idles, so an un-configured device ticks a no-op.
        let interpreter = self
            .interpreter
            .unwrap_or_else(|| Box::new(BehaviorTreeInterpreter::new(function_index.clone())));

        Ok(Arora {
            store,
            engine,
            function_index,
            interpreter: Some(interpreter),
            telemetry: Telemetry::default(),
            hal,
            bridges,
            hal_updates,
            store_changes,
            clock: Clock::default(),
        })
    }
}

/// Build the engine with the right executor host for the target: the browser's
/// native `WebAssembly` runtime on wasm, or the wasmtime + native (dynamic
/// library) hosts otherwise.
#[cfg(feature = "native")]
fn build_engine() -> Result<PinnedEngine> {
    Ok(EngineBuilder::new()
        .add_executor(
            WebAssemblyExecutor::new()
                .map_err(|e| anyhow::anyhow!("failed to create wasm executor: {e}"))?,
        )
        .add_executor(NativeExecutor::new())
        .build())
}

#[cfg(not(feature = "native"))]
fn build_engine() -> Result<PinnedEngine> {
    use arora_engine::executor::browser::BrowserExecutor;
    Ok(EngineBuilder::new()
        .add_executor(BrowserExecutor::new())
        .build())
}
