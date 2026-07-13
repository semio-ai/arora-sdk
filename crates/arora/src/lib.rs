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
/// The Semio Studio connection. Its [`connect`](studio::connect) builds a
/// ready-to-inject Studio [`Bridge`] an embedder attaches with
/// [`AroraBuilder::with_bridge`] — the producer side of viewing a runtime's
/// live data through the Studio bridge.
#[cfg(feature = "studio-bridge")]
pub mod studio;
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
/// Re-exported so binding crates (e.g. `arora-web`) can name the host-function
/// type that [`AroraBuilder::with_host_module`] accepts.
pub use arora_behavior_tree::ModuleFunction;

use anyhow::Result;
use arora_behavior::{interpreter_module, BehaviorInterpreter};
use arora_bridge::{Bridge, BridgeError, Inbound, InboundStream};
use arora_engine::engine::{EngineBuilder, PinnedEngine};
#[cfg(feature = "native")]
use arora_engine::executor::{native::NativeExecutor, wasm::WebAssemblyExecutor};
use arora_engine::load::load_module_from_parts;
use arora_engine::module::ModuleBuilder;
use arora_hal::{FakeHal, Hal, UpdatesStream};
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::CallError;
use arora_types::data::{DataStore, Subscription};
use arora_types::module::low::Header;
use futures::stream::{self, Fuse, SelectAll};
use futures::StreamExt;
use runtime::{Clock, Pending};
use std::cell::RefCell;
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
    /// [`AroraBuilder::with_host_module`].
    pub(crate) function_index: Rc<HashMap<Uuid, ModuleFunction>>,
    /// The one behavior interpreter, ticked each step — an executor injected once
    /// at [`build`](AroraBuilder::build), not swapped afterwards. It defaults to
    /// an empty, ready [`BehaviorTreeInterpreter`] (see
    /// [`with_behavior_interpreter`](AroraBuilder::with_behavior_interpreter));
    /// a behavior is loaded *into* it as a separate step. `None` means nothing to
    /// tick; the interpreter is dropped back to `None` once it reports
    /// [`BehaviorStatus::Done`](arora_behavior::BehaviorStatus).
    ///
    /// Held in a shared cell because two single-threaded phases reach it: the
    /// step loop ticks it, and the interpreter module the builder registered
    /// on the engine loads/edits it (see [`runtime::InterpreterCell`] and
    /// [`arora_behavior::interpreter_module`]).
    pub(crate) interpreter: runtime::InterpreterCell,
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
    modules: Vec<(Header, Box<[u8]>)>,
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

    /// Load a module into the device's engine so behaviors may call its
    /// functions. Repeatable — each call loads one module.
    ///
    /// `header` is the module's low-level [`Header`] — its id, its exported
    /// functions (each with a UUID), and the **executor** that runs it.
    /// `executable` is the module's bytes in whatever format that executor
    /// expects: a `.wasm` for the WebAssembly executor, or a native dynamic
    /// library for the native executor. The engine selects the executor by the
    /// name the header announces, so this one seam loads either format. The
    /// module is loaded at [`build`](Self::build); once loaded, its exported
    /// functions dispatch to guest code through the engine's `CallBridge` —
    /// what a behavior reaches to call them.
    ///
    /// For functions the engine hosts in-process (Rust closures rather than a
    /// loadable executable), use [`with_host_module`](Self::with_host_module).
    pub fn with_module(mut self, header: Header, executable: impl Into<Box<[u8]>>) -> Self {
        self.modules.push((header, executable.into()));
        self
    }

    /// Register natively-hosted module functions so behaviors may call them.
    /// Repeatable — each call adds one module's functions to the
    /// `function_index`, keyed by function UUID.
    ///
    /// This is the host-side counterpart to [`with_module`](Self::with_module):
    /// where `with_module` loads a guest executable, this registers functions
    /// the engine already dispatches in-process. A behavior-tree node bound to
    /// one of these functions builds its call from the frozen `Function` record
    /// carried here. A [`ModuleFunction`] carries no executable, so these must
    /// be functions the engine can already dispatch (natively-hosted).
    pub fn with_host_module(mut self, functions: impl IntoIterator<Item = ModuleFunction>) -> Self {
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
        let mut engine = build_engine()?;

        // Load each guest module into the engine so its exported functions
        // dispatch through the engine's `CallBridge`. Done before the store and
        // seams are wired: a module that fails to load fails the whole build.
        for (header, executable) in self.modules {
            load_module_from_parts(&mut engine, header, executable)
                .map_err(|e| anyhow::anyhow!("failed to load module: {e}"))?;
        }

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

        // The interpreter as a module: a function module under
        // `interpreter_module::ID` whose `LOAD`/`EDIT` functions run on the
        // same cell the step loop ticks. A Call to those ids — from a remote
        // or from a behavior — reaches the interpreter through the engine's
        // normal dispatch, like any module function.
        let interpreter: runtime::InterpreterCell = Rc::new(RefCell::new(Some(interpreter)));
        let module = ModuleBuilder::new(interpreter_module::ID)
            .function(interpreter_module::LOAD, {
                let cell = interpreter.clone();
                move |call| {
                    let graph = interpreter_module::decode_load(&call)
                        .map_err(|message| CallError::Guest { message })?;
                    runtime::with_interpreter(&cell, |interpreter| interpreter.load(graph))
                }
            })
            .function(interpreter_module::EDIT, {
                let cell = interpreter.clone();
                move |call| {
                    let diff = interpreter_module::decode_edit(&call)
                        .map_err(|message| CallError::Guest { message })?;
                    runtime::with_interpreter(&cell, |interpreter| interpreter.apply(diff))
                }
            })
            .build();
        engine.register_module(module.id(), Box::new(module));

        Ok(Arora {
            store,
            engine,
            function_index,
            interpreter,
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

/// Loading a guest wasm module through the builder and dispatching it. Needs
/// the `native` feature: the module runs on the WebAssembly (wasmtime)
/// executor. `test-rust-wasm` is a small guest built as a `wasm32-wasip1`
/// cdylib artifact dependency; Cargo hands its generated header and `.wasm`
/// bytes to the test.
#[cfg(all(test, feature = "native"))]
mod module_loading_tests {
    use super::*;
    use arora_types::call::{Call, CallBridge};
    use arora_types::value::Value;

    const HEADER_YAML: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../modules/test-rust-wasm/src/arora_generated/module.yaml"
    ));
    const WASM: &[u8] = include_bytes!(env!("CARGO_CDYLIB_FILE_TEST_RUST_WASM_test_rust_wasm"));

    // Function id from modules/test-rust-wasm/module.yaml.
    const SUCCEED: &str = "00cd31a8-2cf4-48e6-a957-69a55de90424"; // () -> bool

    fn test_module_header() -> Header {
        serde_yaml::from_str(HEADER_YAML).expect("parse test-rust-wasm header yaml")
    }

    /// `with_module` loads the guest executable into the engine, and its
    /// exported functions dispatch through the engine's `CallBridge` — the same
    /// bridge a behavior reaches when it calls a module.
    #[test]
    fn with_module_loads_a_wasm_module_callable_through_the_engine() {
        let header = test_module_header();
        let module_id = header.id;
        let mut arora = Arora::builder()
            .with_module(header, WASM.to_vec())
            .build()
            .expect("build a device with a loaded wasm module");

        let result = arora
            .engine
            .arora_call(
                &module_id,
                Call {
                    module_id: None,
                    id: Uuid::parse_str(SUCCEED).expect("valid uuid"),
                    args: Vec::new(),
                },
            )
            .expect("call succeed() on the loaded module");
        assert_eq!(result.ret, Value::Boolean(true));
    }

    /// The default device builds fine with no modules loaded.
    #[test]
    fn builds_without_any_module() {
        Arora::builder()
            .build()
            .expect("the default device builds with no modules loaded");
    }

    /// A module whose executable cannot load fails the whole build, rather than
    /// silently yielding a device with a broken module.
    #[test]
    fn a_module_that_fails_to_load_fails_the_build() {
        let header = test_module_header();
        let result = Arora::builder()
            .with_module(header, vec![0xDE, 0xAD, 0xBE, 0xEF]) // not a valid wasm binary
            .build();
        assert!(
            result.is_err(),
            "build must fail when a module's executable cannot load"
        );
    }
}
