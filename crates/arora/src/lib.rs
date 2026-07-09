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
//! Build one with the [`builder`](Arora::builder): pick a data store, a HAL,
//! zero or more bridges, the modules whose functions behaviors may call, and
//! the behavior interpreter — each with a sensible default, so
//! `Arora::builder().build()` yields a self-contained in-process device (fake
//! HAL, no bridge, an empty behavior, a private [`SimpleDataStore`]). Then
//! drive it with [`step`](Arora::step) (once per frame) or
//! [`run`](Arora::run) (the visible loop over `step`).

#[cfg(feature = "native")]
pub mod operator;
mod run;
pub mod runtime;
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
use arora_bridge::{Bridge, BridgeError, Inbound, InboundStream};
use arora_engine::engine::{EngineBuilder, PinnedEngine};
#[cfg(feature = "native")]
use arora_engine::executor::{native::NativeExecutor, wasm::WebAssemblyExecutor};
use arora_hal::{FakeHal, Hal, UpdatesStream};
use arora_simple_data_store::SimpleDataStore;
use arora_types::data::{DataStore, Subscription};
use futures::stream::{self, Fuse, SelectAll};
use futures::StreamExt;
use runtime::{Clock, Pending};
use std::collections::HashMap;
use std::rc::Rc;
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
/// threads: its inbound seams are owned streams it polls from its own loop, its
/// outbound seams are non-blocking pushes, and any real async work lives
/// *inside* the seam implementations. That is also why it drops unchanged into
/// a Web Worker — the worker boundary is the seam's problem, not `Arora`'s.
///
/// Build one with [`Arora::builder`].
pub struct Arora {
    // Owned and touched ONLY by the stepping thread (single-threaded state).
    // Held behind `dyn DataStore` so a wrapping store (e.g. a `NamespacedStore`
    // over one mutualized backend) can be injected via the builder; any sharing
    // lives inside the implementation, the device owns its view.
    pub(crate) store: Box<dyn DataStore>,
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
    // The HAL, owned by the device; outbound writes go through its
    // non-blocking `try_send`. An implementation that also feeds an observer
    // (a simulator UI, a test) shares its internals and hands a sibling handle
    // out itself.
    pub(crate) hal: Box<dyn Hal>,
    // The bridge endpoints, each owned exclusively by this device (their
    // inbound streams were taken at build and merged below); after build they
    // serve outbound `try_send` fan-out and nothing else. A Vec: writes fan
    // out to every remote, reads fan in through the merge.
    pub(crate) bridges: Vec<Box<dyn Bridge>>,
    // The HAL's sensor feed, owned by this device — the step (and natively the
    // `run` select) is its one poller. Fused: once the hardware feed ends it
    // stays quietly finished.
    pub(crate) hal_feed: Fuse<UpdatesStream>,
    // Every endpoint's inbound stream, merged. Each is chained with a terminal
    // disconnect marker at build, so an endpoint's stream ending is an explicit
    // event, never a silent drop from the merge.
    pub(crate) inbound: SelectAll<InboundStream>,
    // What the seams delivered since the previous step; applied and drained by
    // the next step.
    pub(crate) pending: Pending,
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

    /// Borrow the device's blackboard, e.g. to read results between direct
    /// `step` calls. The device owns the store; an embedder that needs an
    /// independent live handle keeps one to the store's shared internals from
    /// before [`build`](AroraBuilder::build) (stores are cheap to clone — a
    /// [`SimpleDataStore`] clone shares its storage).
    pub fn store(&self) -> &dyn DataStore {
        &*self.store
    }
}

/// Assembles an [`Arora`] from its seams, each defaulted so only what differs
/// from the in-process default device has to be named. Every setter returns
/// `self`, so calls chain; [`build`](AroraBuilder::build) wires the engine and
/// the store subscriptions and returns the finished [`Arora`].
#[derive(Default)]
pub struct AroraBuilder {
    store: Option<Box<dyn DataStore>>,
    hal: Option<Box<dyn Hal>>,
    bridges: Vec<Box<dyn Bridge>>,
    interpreter: Option<Box<dyn BehaviorInterpreter>>,
    functions: HashMap<Uuid, ModuleFunction>,
}

impl AroraBuilder {
    /// Use `store` as the device blackboard, **by value**: the device owns its
    /// view. The caller chooses the backend: a plain [`SimpleDataStore`], or a
    /// wrapping store such as a `NamespacedStore` that prefixes every key with a
    /// device namespace before delegating to one mutualized backend (how Studio
    /// mutualizes one store across every spawned device). Sharing lives inside
    /// the implementation — stores clone cheaply onto the same storage, so keep
    /// a clone before handing one in if you need an independent live handle.
    /// Default: a fresh, private [`SimpleDataStore`].
    pub fn with_data_store(mut self, store: Box<dyn DataStore>) -> Self {
        self.store = Some(store);
        self
    }

    /// Use `hal` as the hardware abstraction layer — exactly one per device,
    /// owned **by value**. An implementation that also serves an observer (a
    /// simulator UI, a test double) shares its internals and hands sibling
    /// handles out itself ([`FakeHal`] clones onto the same state). Default: an
    /// in-process [`FakeHal`].
    pub fn with_hal(mut self, hal: Box<dyn Hal>) -> Self {
        self.hal = Some(hal);
        self
    }

    /// Add a bridge endpoint, **by value**: the device owns it exclusively (its
    /// inbound stream is taken at [`build`](AroraBuilder::build) — one poller
    /// per endpoint). An implementation whose transport serves several devices
    /// shares that transport *inside* itself and hands out one endpoint per
    /// device.
    ///
    /// Repeatable: reads fan in from every bridge and writes fan out to every
    /// bridge, so several remotes can observe/command one device. Also
    /// optional: with none added the device runs standalone (e.g. a preview or
    /// a bench test) — nothing arrives, nothing is pushed out.
    pub fn with_bridge(mut self, bridge: Box<dyn Bridge>) -> Self {
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

    /// Wire the engine, apply the defaults for any unset seam, take each
    /// endpoint's inbound stream and merge them, subscribe to the HAL and store
    /// feeds, and return the finished [`Arora`].
    ///
    /// Fails only if the engine's executor host cannot be created. Fully
    /// synchronous: there is nothing to spawn — the HAL and bridges own any
    /// async internally, behind their stream/push seams.
    pub fn build(self) -> Result<Arora> {
        let engine = build_engine()?;
        let store = self
            .store
            .unwrap_or_else(|| Box::new(SimpleDataStore::new()));
        let hal: Box<dyn Hal> = self.hal.unwrap_or_else(|| Box::new(FakeHal::new()));
        let store_changes = store.subscribe();
        let hal_feed = hal.updates().fuse();

        // Take each endpoint's inbound stream (the take-once seam: from here on
        // the device is the endpoint's one poller) and merge them. A stream
        // ending means that endpoint disconnected — chain a terminal marker so
        // the merge surfaces it as an explicit event instead of dropping the
        // endpoint silently.
        let mut bridges = self.bridges;
        let mut inbound = SelectAll::new();
        for bridge in &mut bridges {
            let disconnected = stream::once(async {
                Inbound::DeviceInfo(Err(BridgeError::Disconnected(
                    "the endpoint's inbound stream ended".into(),
                )))
            });
            inbound.push(bridge.take_inbound().chain(disconnected).boxed() as InboundStream);
        }

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
            hal_feed,
            inbound,
            pending: Pending::default(),
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
