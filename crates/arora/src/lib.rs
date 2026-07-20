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
pub use runtime::RuntimeError;

/// Re-exported so embedders can construct the default behavior executor — an
/// empty, ready [`BehaviorTreeInterpreter`] — and load a behavior into it before
/// injecting it with [`AroraBuilder::with_behavior_interpreter`].
pub use arora_behavior_tree::behavior::BehaviorTreeInterpreter;
/// Re-exported so an embedder can name the host-function type that
/// [`AroraBuilder::with_host_module`] accepts.
pub use arora_behavior_tree::ModuleFunction;

use crate::runtime::EndpointInbound;
use anyhow::Result;
use arora_behavior::{interpreter_module, BehaviorInterpreter};
use arora_bridge::{Bridge, BridgeCommand, BridgeError, BridgeOp, Inbound};
use arora_engine::engine::{EngineBuilder, PinnedEngine};
#[cfg(feature = "native")]
use arora_engine::executor::{native::NativeExecutor, wasm::WebAssemblyExecutor};
use arora_engine::load::load_module_from_parts;
use arora_engine::module::ModuleBuilder;
use arora_hal::{FakeHal, Hal, UpdatesStream};
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::{Call, CallBridge, CallError, CallResult};
use arora_types::data::{DataStore, Subscription};
use arora_types::module::low::Header;
use futures::channel::{mpsc, oneshot};
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
    // The HAL's sensor feed, owned by this device — the step (and the `run`
    // select) is its one poller. Fused: once the hardware feed ends it stays
    // quietly finished.
    pub(crate) hal_feed: Fuse<UpdatesStream>,
    // Every endpoint's inbound stream, merged. Each is chained with a terminal
    // disconnect marker at build, so an endpoint's stream ending is an explicit
    // event, never a silent drop from the merge.
    pub(crate) inbound: SelectAll<EndpointInbound>,
    // What the seams delivered since the previous step; applied and drained by
    // the next step.
    pub(crate) pending: Pending,
    // Per endpoint, whether that remote asked for the device's data — parallel
    // to `bridges`. Outbound changes go to an endpoint only while it asks; a
    // device nobody listens to keeps stepping, it just does not talk.
    pub(crate) data_requested: Vec<bool>,
    // The sending end every in-process `Caller` clones; its receiving end is
    // merged into `inbound`, so a caller's Call travels the same path as a
    // remote's.
    pub(crate) caller_tx: mpsc::UnboundedSender<Inbound>,
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

    /// Dispatch a [`Call`] against the module it names, in-process — the same
    /// dispatch a bridge Call takes, without a bridge. Everything reachable
    /// over a remote's Call is reachable here: a loaded module's exported
    /// functions, the natively-hosted ones, and the interpreter module's
    /// LOAD/EDIT functions — so an embedder loads or edits the running
    /// behavior with no bridge attached.
    ///
    /// Call it between steps: dispatch runs synchronously on the device's
    /// thread. Errors are the dispatch's own — a call naming no module, a
    /// module or function the engine does not know, or the callee failing.
    pub fn call(&mut self, call: Call) -> Result<CallResult, CallError> {
        self.engine.arora_call(call)
    }

    /// Borrow the engine's call seam. [`call`](Arora::call) covers plain
    /// dispatch; this is for embedders that need the rest of the
    /// [`CallBridge`] — registering an in-process [`Callable`]
    /// (e.g. a host closure a behavior invokes indirectly) and dispatching to
    /// it by its [`CallableId`].
    ///
    /// [`Callable`]: arora_types::call::Callable
    /// [`CallableId`]: arora_types::call::CallableId
    pub fn engine(&mut self) -> &mut dyn CallBridge {
        &mut self.engine
    }

    /// An in-process [`Caller`] onto this device. Take it before handing the
    /// device to [`run`](Arora::run) — `run` owns the device for its whole
    /// life, while the caller stays usable throughout.
    pub fn caller(&self) -> Caller {
        Caller {
            tx: self.caller_tx.clone(),
        }
    }
}

/// Dispatch [`Call`]s into the device from the same process, including while
/// [`run`](Arora::run) owns it. Obtained from [`Arora::caller`]; clones
/// freely, every clone reaching the same device.
///
/// A call is enqueued immediately and applied at the next step's event phase —
/// the same path and ordering as a remote's Call — and the future resolves on
/// that step's reply. [`Arora::call`] is the synchronous counterpart for an
/// embedder holding the device between steps.
#[derive(Clone)]
pub struct Caller {
    tx: mpsc::UnboundedSender<Inbound>,
}

impl Caller {
    /// Dispatch `call`, resolving after the step that applies it. Errors are
    /// the dispatch's own ([`Arora::call`]'s), plus the device being gone.
    pub async fn call(&self, call: Call) -> Result<CallResult, CallError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .unbounded_send(Inbound::Command(BridgeCommand::new(
                BridgeOp::Call(call),
                tx,
            )))
            .map_err(|_| CallError::Generic {
                message: "the device is gone".to_string(),
            })?;
        match rx.await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(message)) => Err(CallError::Generic { message }),
            Err(_) => Err(CallError::Generic {
                message: "the device dropped the call".to_string(),
            }),
        }
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

    /// Run the assembled device to completion with the standard operator flow:
    /// pick the front end (terminal UI or headless), fill the default bridge
    /// when none was injected — Semio Studio under the `studio-bridge` feature
    /// (operator prompt, local-bridge fallback when declined), the open local
    /// bridge otherwise — default any other unset seam (a private
    /// `SimpleDataStore`, the fake HAL, an empty interpreter), and drive the
    /// step loop.
    ///
    /// This is the run entrypoint for composed devices: features only pick the
    /// *defaults*, never what you can inject — e.g. a device whose blackboard
    /// is a custom [`DataStore`] runs with
    /// `Arora::builder().with_hal(hal).with_data_store(store).run()`.
    #[cfg(feature = "native")]
    pub async fn run(mut self) -> Result<()> {
        // The front end is picked first: building it installs the matching log
        // sink, so everything after (including bridge resolution) is captured.
        let frontend = run::select_frontend();
        if self.bridges.is_empty() {
            #[cfg(feature = "studio-bridge")]
            {
                self = self.with_bridge(studio::default_bridge(&frontend).await?);
            }
            #[cfg(not(feature = "studio-bridge"))]
            {
                self = self.with_bridge(run::local_ws_bridge().await?);
            }
        }
        run::run_builder_with_frontend(self, frontend).await
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
        for (endpoint, bridge) in bridges.iter_mut().enumerate() {
            let disconnected = stream::once(async {
                Inbound::DeviceInfo(Err(BridgeError::Disconnected(
                    "the endpoint's inbound stream ended".into(),
                )))
            });
            // Each event carries the endpoint it came from: what one remote
            // asks for is not what another asks for.
            inbound.push(
                bridge
                    .take_inbound()
                    .chain(disconnected)
                    .map(move |event| (Some(endpoint), event))
                    .boxed(),
            );
        }

        // The in-process callers' feed: one more inbound stream, delivered and
        // applied exactly like a remote's, tagged as no endpoint.
        let (caller_tx, caller_rx) = mpsc::unbounded();
        inbound.push(caller_rx.map(|event| (None, event)).boxed());

        let endpoints = bridges.len();
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
            hal,
            bridges,
            hal_feed,
            inbound,
            pending: Pending::default(),
            data_requested: vec![false; endpoints],
            caller_tx,
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
    /// exported functions dispatch through [`Arora::call`] — the same path a
    /// behavior or a remote reaches when it calls a module.
    #[test]
    fn with_module_loads_a_wasm_module_reachable_through_call() {
        let header = test_module_header();
        let module_id = header.id;
        let mut arora = Arora::builder()
            .with_module(header, WASM.to_vec())
            .build()
            .expect("build a device with a loaded wasm module");

        let result = arora
            .call(Call {
                module_id: Some(module_id),
                id: Uuid::parse_str(SUCCEED).expect("valid uuid"),
                args: Vec::new(),
            })
            .expect("call succeed() on the loaded module");
        assert_eq!(result.ret, Value::Boolean(true));
    }

    /// Dispatch is always module-scoped: a call naming no module is refused.
    #[test]
    fn a_call_naming_no_module_is_refused() {
        let mut arora = Arora::builder().build().expect("build the default device");
        let err = arora
            .call(Call {
                module_id: None,
                id: Uuid::parse_str(SUCCEED).expect("valid uuid"),
                args: Vec::new(),
            })
            .expect_err("a module-less call is refused");
        assert!(err.to_string().contains("module id"), "{err}");
    }

    /// The interpreter module the builder registered is reachable in-process:
    /// a device with no bridge at all loads a behavior through [`Arora::call`].
    #[test]
    fn call_loads_a_behavior_with_no_bridge() {
        let mut arora = Arora::builder().build().expect("build the default device");
        let result = arora
            .call(interpreter_module::encode_load(
                &arora_behavior::Graph::empty(),
            ))
            .expect("the load call succeeds");
        assert_eq!(result.ret, arora_types::value::Value::Unit);
    }

    /// A [`Caller`] reaches the device without borrowing it: the call is
    /// enqueued at once and applied by the next step.
    #[tokio::test]
    async fn a_caller_call_lands_on_the_next_step() {
        let mut arora = Arora::builder().build().expect("build the default device");
        let caller = arora.caller();
        let mut call = Box::pin(caller.call(interpreter_module::encode_load(
            &arora_behavior::Graph::empty(),
        )));
        // Enqueued but not applied: nothing has stepped yet.
        assert!(futures::poll!(call.as_mut()).is_pending());
        arora
            .step(std::time::Duration::from_millis(10))
            .expect("step");
        let result = call.await.expect("the load call succeeds");
        assert_eq!(result.ret, arora_types::value::Value::Unit);
    }

    /// The caller serves a running device: [`run`](Arora::run) owns the device
    /// exclusively for its whole life, and the caller still gets its reply.
    #[tokio::test]
    async fn a_caller_reaches_a_running_device() {
        use futures::FutureExt;

        let mut arora = Arora::builder().build().expect("build the default device");
        let caller = arora.caller();
        let run = arora.run(std::time::Duration::from_millis(5));
        let call = caller.call(interpreter_module::encode_load(
            &arora_behavior::Graph::empty(),
        ));
        futures::pin_mut!(run, call);
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            futures::select! {
                result = call.fuse() => result,
                _ = run.fuse() => panic!("run ended before the call resolved"),
            }
        })
        .await
        .expect("the running device answers promptly");
        assert_eq!(
            outcome.expect("the load call succeeds").ret,
            arora_types::value::Value::Unit
        );
    }

    /// [`Arora::engine`] exposes the rest of the `CallBridge`: registering an
    /// in-process callable and dispatching to it by id.
    #[test]
    fn engine_registers_and_dispatches_an_in_process_callable() {
        use arora_types::call::Callable;

        struct Answer;
        impl Callable for Answer {
            fn call(&self, _caller: &mut dyn CallBridge) -> Result<Value, CallError> {
                Ok(Value::I32(42))
            }
        }

        let mut arora = Arora::builder().build().expect("build the default device");
        let id = arora.engine().arora_register_callable(Rc::new(Answer));
        let result = arora
            .engine()
            .arora_call_indirect(&id)
            .expect("the registered callable dispatches");
        assert!(matches!(result, Value::I32(42)));
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
