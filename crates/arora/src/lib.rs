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

/// A device's directory: what the device keeps of its own from one run to the
/// next, each use in a subdirectory of its own.
#[cfg(feature = "native")]
pub mod device_dir;
mod hal_module;
/// A module directory: a guest module's header beside its artifact, the form a
/// device carries a module in — under its device directory's `modules/`, or
/// anywhere `--module` names.
#[cfg(feature = "native")]
pub mod module_dir;
mod module_discovery;
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

/// The open local bridge's crate, re-exported: an embedder configuring the
/// bridge it hands to [`local_ws_bridge_with`] names `ServerConfig` here instead
/// of depending on the crate separately, so the version it configures is the one
/// this arora serves.
#[cfg(feature = "native")]
pub use arora_bridge_ws as bridge_ws;
#[cfg(feature = "native")]
pub use run::{
    local_ws_bridge, local_ws_bridge_with, run, run_with, run_with_frontend, run_with_hal,
    serve_local_ws_bridge, standard_frontend, DeviceCli,
};
pub use runtime::RuntimeError;

/// Re-exported so embedders can construct the default behavior executor — an
/// empty, ready [`BehaviorTreeInterpreter`] — and load a behavior into it before
/// injecting it with [`AroraBuilder::with_behavior_interpreter`].
pub use arora_behavior_tree::behavior::BehaviorTreeInterpreter;
/// Re-exported so a behavior-tree embedder can name the host-function metadata
/// type a Groot tree binds its action/condition nodes to.
pub use arora_behavior_tree::ModuleFunction;

use crate::runtime::EndpointInbound;
use anyhow::Result;
use arora_behavior::{interpreter_module, BehaviorInterpreter};
/// Re-exported so an embedder holding a [`LocalCaller`] can name the run handle
/// [`spawn`](LocalCaller::spawn) and [`invoke`](LocalCaller::invoke) answer
/// with and the run id [`halt`](LocalCaller::halt) takes.
pub use arora_behavior::{TaskHandle, TaskId};
/// Re-exported so an embedder holding a device's [`LocalCaller`] (or any other
/// caller) can name the trait its `call` comes from.
pub use arora_bridge::Caller;
/// Re-exported so an embedder holding a [`LocalCaller`] can name what
/// [`describe_methods`](LocalCaller::describe_methods) answers with.
pub use arora_bridge::MethodSignature;
use arora_bridge::{client, Bridge, BridgeCommand, BridgeError, BridgeOp, Inbound};
/// Re-exported so an embedder can compile a guest module once and load it into
/// any number of devices with [`AroraBuilder::with_compiled_module`].
pub use arora_engine::compiled::CompiledModule;
use arora_engine::engine::{EngineBuilder, PinnedEngine};
#[cfg(feature = "native")]
use arora_engine::executor::{native::NativeExecutor, wasm::WebAssemblyExecutor};
/// Re-exported so an embedder can assemble a host-side module — a set of
/// in-process functions under a module id — and inject it with
/// [`AroraBuilder::with_host_module`].
pub use arora_engine::module::{FunctionDescription, HostModule, ModuleBuilder};
use arora_hal::{FakeHal, Hal, UpdatesStream};
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::{Call, CallBridge, CallError, CallResult};
use arora_types::data::{DataStore, KeyMeta, Subscription};
use arora_types::module::declared::AroraModule;
use arora_types::module::low::{self, Header};
use arora_types::record::module::frozen::ExportKind;
use arora_types::value::Value;
use futures::channel::{mpsc, oneshot};
use futures::stream::{self, Fuse, SelectAll};
use futures::StreamExt;
use runtime::{Clock, Pending};
use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
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
    /// Host-function metadata a behavior-tree Groot tree binds its action and
    /// condition nodes to, keyed by function UUID. The basic control nodes are
    /// dispatched natively and are not in this index. A host module registered
    /// through [`AroraBuilder::with_host_module`] dispatches through the engine
    /// directly, so it does not appear here.
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
    /// The behavior's standing error — the message of its latest failed
    /// tick, `None` while healthy. A watch, so a receiver taken before
    /// [`run`](Arora::run) owns the device still observes a self-paced
    /// loop's failures; each distinct failure is sent once.
    pub(crate) behavior_error: watch::Sender<Option<String>>,
    // The HAL, owned by the device; outbound writes go through its
    // non-blocking `try_send`. An implementation that also feeds an observer
    // (a simulator UI, a test) shares its internals and hands a sibling handle
    // out itself. Shared with the HAL module the builder registered on the
    // engine, whose functions answer from it.
    pub(crate) hal: Arc<dyn Hal>,
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
    // The sending end every in-process `LocalCaller` clones; its receiving
    // end is merged into `inbound`, so a caller's Call travels the same path
    // as a remote's.
    pub(crate) caller_tx: mpsc::UnboundedSender<Inbound>,
    pub(crate) store_changes: Subscription,
    // The built-in clock: monotonic nanoseconds since start, advanced by each
    // step's `dt`. Published into the store's built-in keys each step, before any
    // behavior ticks; the flush phase filters the built-in namespace out of what
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

    /// Install a Groot behavior tree as the device's behavior. Its tags are
    /// resolved against the device's own method index — the natively-hosted
    /// control nodes and every loaded module's exports — and the lowered graph
    /// is loaded through the interpreter module, the path a remote's load
    /// takes. Call it between steps, after [`build`](AroraBuilder::build); the
    /// tree binds to the store at its first tick.
    pub fn load_groot(&mut self, xml: &str) -> Result<()> {
        let tree = arora_behavior_tree::schema_groot::BehaviorTree::try_from_groot_xml(xml)
            .map_err(|e| anyhow::anyhow!("the Groot tree does not parse: {e:?}"))?;
        let graph = tree
            .into_graph(&self.function_index)
            .map_err(|e| anyhow::anyhow!("the Groot tree does not lower: {e:?}"))?;
        self.call(interpreter_module::encode_load(&graph))
            .map_err(|e| anyhow::anyhow!("the behavior did not load: {e}"))?;
        Ok(())
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

    /// An in-process [`LocalCaller`] onto this device. Take it before handing
    /// the device to [`run`](Arora::run) — `run` owns the device for its whole
    /// life, while the caller stays usable throughout.
    pub fn caller(&self) -> LocalCaller {
        LocalCaller {
            tx: self.caller_tx.clone(),
        }
    }

    /// Watch the behavior's standing error: the message of its latest failed
    /// tick, `None` while the behavior is healthy or none is installed. A
    /// failing tick does not stop the device — the failure stands (and is
    /// logged, once per distinct message) until a tick succeeds. The
    /// receiver outlives [`run`](Arora::run) owning the device, so a
    /// self-paced device's embedder takes one first and still observes
    /// failures.
    pub fn behavior_error(&self) -> watch::Receiver<Option<String>> {
        self.behavior_error.subscribe()
    }
}

/// The in-process client of a device: everything a remote client asks of it
/// over a bridge — calling a function, listing its keys, describing its
/// methods, invoking one by name, starting and halting a run — from the same
/// process, including while [`run`](Arora::run) owns it. Obtained from
/// [`Arora::caller`]; clones freely, every clone reaching the same device.
///
/// Each operation is the [`BridgeOp`] a remote sends, put on the device's
/// inbound queue **before the method returns** — the future is only the reply —
/// and applied at the next step's event phase, the same path and ordering as a
/// remote's. A caller can therefore fire and step the device in the same breath
/// without touching the future (a JS Promise, for one, is first polled a
/// microtask later). [`invoke`](Self::invoke) is the one operation that takes
/// two steps: it reads the method's signature on one, and its call is applied
/// on the next. [`Arora::call`] is the synchronous counterpart of
/// [`call`](Caller::call) for an embedder holding the device between steps.
///
/// Every future resolves to a [`CallError::Generic`] when the device is gone
/// (dropped before answering) or refuses the operation, carrying its message.
#[derive(Clone)]
pub struct LocalCaller {
    tx: mpsc::UnboundedSender<Inbound>,
}

impl LocalCaller {
    /// Put `op` on the device's inbound queue now, and await its reply.
    fn ask(
        &self,
        op: BridgeOp,
    ) -> impl Future<Output = Result<CallResult, CallError>> + Send + 'static {
        let (tx, rx) = oneshot::channel();
        let sent = self
            .tx
            .unbounded_send(Inbound::Command(BridgeCommand::new(op, tx)))
            .map_err(|_| generic("the device is gone"));
        async move {
            sent?;
            match rx.await {
                Ok(Ok(result)) => Ok(result),
                Ok(Err(message)) => Err(CallError::Generic { message }),
                Err(_) => Err(generic("the device dropped the request")),
            }
        }
    }

    /// The device's keys under `prefix` (every key when `None`), sorted by path,
    /// each with what its store says it is: every key that holds a value or that
    /// the store has meta for. A key nothing has described carries the default
    /// meta, whose only statement is the shape of the value it holds.
    pub fn list_keys(
        &self,
        prefix: Option<String>,
    ) -> impl Future<Output = Result<Vec<(String, KeyMeta)>, CallError>> + Send + 'static {
        let reply = self.ask(BridgeOp::ListKeys { prefix });
        async move {
            arora_types::value_serde::from_value(reply.await?.ret)
                .map_err(|e| generic(format!("the listed keys did not decode: {e}")))
        }
    }

    /// The device's callable methods whose name starts with `prefix` (all of
    /// them when `None`), sorted by name, each with its full signature: the
    /// module and function ids a [`Call`] targets, and the parameters and return
    /// type. Those are the methods its modules and its behavior interpreter
    /// describe.
    pub fn describe_methods(
        &self,
        prefix: Option<String>,
    ) -> impl Future<Output = Result<Vec<MethodSignature>, CallError>> + Send + 'static {
        let reply = self.ask(BridgeOp::DescribeMethods { prefix });
        async move {
            arora_types::value_serde::from_value(reply.await?.ret)
                .map_err(|e| generic(format!("the described methods did not decode: {e}")))
        }
    }

    /// Call the method named `method`, with `args` by parameter name, as a
    /// remote client invokes it over a bridge.
    ///
    /// A plain method answers with its return value ([`Invoked::Returned`]). A
    /// task-shaped one — returning the behavior `Status` — is spawned as a run
    /// instead and answers at once with its handle ([`Invoked::Started`]); the
    /// run reports on the handle's status key, and [`halt`](Self::halt) stops
    /// it.
    ///
    /// Names are the bare names modules export, so two modules may share one:
    /// pass the exporting module's id in `module` to choose. A name more than
    /// one module exports, with no `module`, fails naming those modules rather
    /// than calling any of them. An argument the signature does not declare
    /// fails the call; a required parameter no argument names fails it in the
    /// callee.
    ///
    /// The method's signature is read on the next step and its call applied on
    /// the step after.
    pub fn invoke(
        &self,
        method: &str,
        args: HashMap<String, Value>,
        module: Option<Uuid>,
    ) -> impl Future<Output = Result<Invoked, CallError>> + Send + 'static {
        let described = self.describe_methods(Some(method.to_string()));
        let caller = self.clone();
        let method = method.to_string();
        async move {
            let signatures = described.await?;
            let signature = client::find_method(&signatures, &method, module).map_err(generic)?;
            let call = client::call_of(signature, args).map_err(generic)?;
            if client::task_shaped(&signature.function) {
                caller.spawn(call).await.map(Invoked::Started)
            } else {
                caller
                    .ask(BridgeOp::Call(call))
                    .await
                    .map(|result| Invoked::Returned(result.ret))
            }
        }
    }

    /// Start `call` as a task run, concurrently with every other run, answering
    /// with its handle: the run's id, the key that reports its status, and the
    /// keys carrying its feedback and result and steering it. The device's
    /// behavior interpreter hosts the run; one that hosts none refuses.
    pub fn spawn(
        &self,
        call: Call,
    ) -> impl Future<Output = Result<TaskHandle, CallError>> + Send + 'static {
        let reply = self.ask(BridgeOp::Call(client::spawn(&call)));
        async move { interpreter_module::decode_spawn_result(&reply.await?.ret).map_err(generic) }
    }

    /// Start `graph`, written in the language `graph_type` names, as a task
    /// run, concurrently with every other run, answering with its handle like
    /// [`spawn`](Self::spawn). The graph is the run's program: the device's
    /// behavior interpreter ticks it every step beside its main behavior until
    /// it ends or is [halted](Self::halt), and an edit through the interpreter
    /// module reaches its nodes. An interpreter that does not read
    /// `graph_type`, or hosts no graph as a run, refuses.
    pub fn spawn_graph(
        &self,
        graph_type: &arora_behavior::GraphType,
        graph: &arora_behavior::Graph,
    ) -> impl Future<Output = Result<TaskHandle, CallError>> + Send + 'static {
        let reply = self.ask(BridgeOp::Call(interpreter_module::encode_spawn_graph(
            graph_type,
            graph,
            arora_behavior::RunPolicy::Concurrent,
        )));
        async move { interpreter_module::decode_spawn_result(&reply.await?.ret).map_err(generic) }
    }

    /// Stop the run `run` names: it ends `Failure` on the step after the halt is
    /// applied. Idempotent — halting a finished or unknown run is a clean no-op.
    pub fn halt(
        &self,
        run: TaskId,
    ) -> impl Future<Output = Result<(), CallError>> + Send + 'static {
        let reply = self.ask(BridgeOp::Call(client::halt(run.0)));
        async move { reply.await.map(|_| ()) }
    }
}

impl Caller for LocalCaller {
    fn call(&self, call: Call) -> arora_bridge::CallFuture<'_> {
        Box::pin(self.ask(BridgeOp::Call(call)))
    }
}

/// What a [`LocalCaller::invoke`] answered.
#[derive(Debug, Clone, PartialEq)]
pub enum Invoked {
    /// A plain method's return value.
    Returned(Value),
    /// A task-shaped method's run, started: follow it on its status key, stop it
    /// with [`LocalCaller::halt`] by its id.
    Started(TaskHandle),
}

/// A guest module as the builder holds it until [`build`](AroraBuilder::build).
enum GuestModule {
    /// Its header and executable, compiled at build for the one device.
    Source(Header, Box<[u8]>),
    /// Compiled ahead, possibly shared with other devices.
    Compiled(CompiledModule),
}

impl GuestModule {
    fn header(&self) -> &Header {
        match self {
            GuestModule::Source(header, _) => header,
            GuestModule::Compiled(compiled) => compiled.header(),
        }
    }
}

fn generic(message: impl Into<String>) -> CallError {
    CallError::Generic {
        message: message.into(),
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
    start_time: Duration,
    bridges: Vec<Box<dyn Bridge>>,
    interpreter: Option<Box<dyn BehaviorInterpreter>>,
    functions: HashMap<Uuid, ModuleFunction>,
    modules: Vec<GuestModule>,
    host_modules: Vec<HostModule>,
    groot: Option<String>,
    #[cfg(feature = "native")]
    step_period: Option<std::time::Duration>,
    #[cfg(feature = "native")]
    frontend: Option<operator::Frontend>,
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

    /// Start the device's clock at `start` instead of zero: the time it reads
    /// before its first step. The first [`step`](Arora::step)`(dt)` then
    /// publishes `arora/time = start + dt` and `arora/dt = dt`, so a device that
    /// joins peers which have already run for `start` takes their timeline with
    /// an ordinary first frame, rather than with one step whose `dt` spans the
    /// whole of `start`. Devices started at the same time and stepped with the
    /// same `dt`s read the same `arora/time` at every step.
    ///
    /// The clock is set here only: a built device has no setter, and only its
    /// steps move it. Default: zero.
    ///
    /// The clock counts `u64` nanoseconds (~584 years): a start beyond that
    /// saturates at `u64::MAX`, where `arora/time` stays.
    pub fn with_start_time(mut self, start: Duration) -> Self {
        self.start_time = start;
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
    /// [`BehaviorTreeInterpreter::load_groot`]) before it is handed here — or,
    /// for a tree whose leaves name loaded modules, into the built device with
    /// [`Arora::load_groot`], which resolves them against the index. Default
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
        self.modules
            .push(GuestModule::Source(header, executable.into()));
        self
    }

    /// Load a module compiled ahead into the device's engine, as
    /// [`with_module`](Self::with_module) loads one from its bytes: the same
    /// checks at [`build`](Self::build), the same dispatch once loaded. The
    /// device instantiates the compiled code — its own instance, with its own
    /// memory — instead of compiling the executable, so devices built from one
    /// [`CompiledModule`] compile it once between them. Repeatable — each call
    /// loads one module.
    ///
    /// ```ignore
    /// let animation = arora::CompiledModule::new(header, &wasm)?;
    /// let devices = (0..n)
    ///     .map(|_| Arora::builder().with_compiled_module(&animation).build())
    ///     .collect::<Result<Vec<_>, _>>()?;
    /// ```
    pub fn with_compiled_module(mut self, module: &CompiledModule) -> Self {
        self.modules.push(GuestModule::Compiled(module.clone()));
        self
    }

    /// Load a module **declared in Rust** as a guest executable: the header
    /// comes from the declaration, and every export joins the method index
    /// with the declaration's frozen signature. Repeatable.
    ///
    /// [`with_module`](Self::with_module) can index only primitive-typed
    /// exports, because a header carries no type versions to freeze a structure
    /// or an enumeration against. The declaration does: `M::exports()` hands
    /// each function's frozen signature, so an export of any type — a
    /// `Status`-returning behavior leaf included — is discoverable and reachable
    /// from a behavior tree, exactly as the same crate registered in-process
    /// with [`with_host_module`](Self::with_host_module)
    /// (`HostModule::of::<M>()`) would be. `executor` names how `executable`
    /// runs — `"wasm"` for the `wasm32-wasip1` build of the declaring crate.
    ///
    /// ```ignore
    /// let device = Arora::builder()
    ///     .with_declared_module::<my_module::Module>(wasm_executor(), WASM_BYTES)
    ///     .build()?;
    /// ```
    pub fn with_declared_module<M: AroraModule>(
        mut self,
        executor: low::Executor,
        executable: impl Into<Box<[u8]>>,
    ) -> Self {
        for function in M::exports() {
            self.functions.insert(
                function.id,
                ModuleFunction {
                    module_id: M::id(),
                    function_id: function.id,
                    function_name: function.name.to_string(),
                    function: function.signature,
                },
            );
        }
        self.modules
            .push(GuestModule::Source(M::header(executor), executable.into()));
        self
    }

    /// Add a host-side module: a set of in-process functions under a module id,
    /// assembled with [`ModuleBuilder`] and finished with
    /// [`ModuleBuilder::build`]. Repeatable — each call adds one module.
    ///
    /// The host-side counterpart to [`with_module`](Self::with_module): where
    /// `with_module` loads a guest executable the engine runs through an
    /// executor, this registers a module whose functions are plain host code
    /// (Rust closures). Either way its functions dispatch through the engine's
    /// `CallBridge` — reachable from a behavior (a graph `ExternalFunction`
    /// node), an in-process caller, or a remote — under the module id and the
    /// function ids it was built with. Loaded at [`build`](Self::build).
    ///
    /// ```ignore
    /// let module = arora::ModuleBuilder::new(module_id)
    ///     .function(step_id, move |call| { /* ... */ })
    ///     .build();
    /// let device = Arora::builder().with_host_module(module).build()?;
    /// ```
    pub fn with_host_module(mut self, module: HostModule) -> Self {
        self.host_modules.push(module);
        self
    }

    /// Install a Groot behavior tree as the device's behavior once it is
    /// built: [`build`](AroraBuilder::build) resolves its tags against the
    /// method index it assembled — the control nodes and every loaded
    /// module's exports — and loads it as [`Arora::load_groot`] does, failing
    /// the build if the tree does not parse, names an unknown function, or
    /// does not load. What a device run through [`run`](AroraBuilder::run)
    /// uses, since `run` builds the device itself.
    pub fn with_groot(mut self, xml: impl Into<String>) -> Self {
        self.groot = Some(xml.into());
        self
    }

    /// The target time between steps for [`run`](AroraBuilder::run); default
    /// [`Arora::DEFAULT_STEP_PERIOD`]. A device whose behavior runs at a fixed
    /// control rate — a control policy sampled at 50 Hz — steps at that rate.
    #[cfg(feature = "native")]
    pub fn with_step_period(mut self, period: std::time::Duration) -> Self {
        self.step_period = Some(period);
        self
    }

    /// Use `frontend` as the operator front end for [`run`](AroraBuilder::run)
    /// instead of the standard pick (terminal UI on an interactive terminal,
    /// headless otherwise) — the seam for an application that brings its own
    /// UI, or for the terminal UI extended with application commands
    /// ([`tui::commands_frontend`]). A binary that needs the front end — and
    /// the log sink it installs — before `run`, say to log what it loads,
    /// takes the standard pick itself from [`standard_frontend`] and injects
    /// it here.
    #[cfg(feature = "native")]
    pub fn with_frontend(mut self, frontend: operator::Frontend) -> Self {
        self.frontend = Some(frontend);
        self
    }

    /// Run the assembled device to completion with the standard operator flow:
    /// pick the front end (the injected one, else terminal UI or headless by
    /// terminal detection), fill the default bridge
    /// when none was injected — Semio Studio under the `studio-bridge` feature
    /// (operator prompt, local-bridge fallback when declined), the open local
    /// bridge otherwise — default any other unset seam (a private
    /// `SimpleDataStore`, the fake HAL, an empty interpreter), and drive the
    /// step loop.
    ///
    /// **Stopping is dropping**: everything the run holds — the front end (and
    /// its terminal takeover), the device, its bridges and their servers —
    /// lives inside the returned future, so dropping it (e.g. losing a
    /// `select!` against your own stop signal) is a complete, synchronous
    /// teardown, after which a host can build and run a fresh device.
    ///
    /// This is the run entrypoint for composed devices: features only pick the
    /// *defaults*, never what you can inject — e.g. a device whose blackboard
    /// is a custom [`DataStore`] runs with
    /// `Arora::builder().with_hal(hal).with_data_store(store).run()`.
    #[cfg(feature = "native")]
    pub async fn run(mut self) -> Result<()> {
        // The front end comes first: building it installs the matching log
        // sink, so everything after (including bridge resolution) is captured.
        let frontend = match self.frontend.take() {
            Some(frontend) => frontend,
            None => run::standard_frontend(),
        };
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

        // The device's method index — what `DescribeMethods` serves. Both the
        // loaded guest modules and the host modules fold their functions in.
        let mut functions = self.functions;

        // Load each guest module into the engine so its exported functions
        // dispatch through the engine's `CallBridge`. Done before the store and
        // seams are wired: a module that fails to load fails the whole build.
        // Its exports whose parameters and return are all primitives or
        // optionals over a scalar primitive also join the method index, so `DescribeMethods`
        // lists them — a guest header carries no type versions, so a signature
        // naming a record (which needs a registry to pin its version), a map
        // or a fixed-length array dispatches but stays undiscoverable.
        //
        // One module id, one module: every source of guest modules (an
        // embedder's `with_module`, the device directory, `--module`) converges
        // here, and the engine answers an already-loaded id with Ok — which
        // would dispatch the first and describe the last.
        let mut guest_modules: HashMap<Uuid, String> = HashMap::new();
        for module in self.modules {
            let header = module.header();
            let module_id = header.id;
            let module_name = header.name.clone();
            if let Some(first) = guest_modules.insert(module_id, module_name.clone()) {
                anyhow::bail!(
                    "guest modules '{first}' and '{module_name}' both have id {module_id}: a \
                     device loads one module per id"
                );
            }
            for export in &header.exports {
                let low::ExportSymbol::Function(function) = export;
                // An export its declaration already indexed
                // (`with_declared_module`) keeps that frozen signature; only a
                // bare header's exports are frozen from their primitives here.
                if functions.contains_key(&function.id) {
                    continue;
                }
                match module_discovery::guest_function_signature(function) {
                    Some(signature) => {
                        functions.insert(
                            function.id,
                            ModuleFunction {
                                module_id,
                                function_id: function.id,
                                function_name: function.name.clone(),
                                function: signature,
                            },
                        );
                    }
                    None => log::warn!(
                        "guest module {module_id} export '{}' ({}) has a parameter or return \
                         type that is neither a primitive nor an optional over a scalar \
                         primitive; it \
                         dispatches but DescribeMethods does not list it",
                        function.name,
                        function.id
                    ),
                }
            }
            let function_count = header.exports.len();
            // A module given as bytes compiles here, for this device alone.
            let compiled = match module {
                GuestModule::Source(header, executable) => {
                    CompiledModule::new(header, &executable).map_err(Into::into)
                }
                GuestModule::Compiled(compiled) => Ok(compiled),
            };
            compiled
                .and_then(|compiled| engine.load_compiled_module(&compiled))
                .map_err(|e| {
                    anyhow::anyhow!("failed to load module '{module_name}' ({module_id}): {e}")
                })?;
            log::info!("loaded module '{module_name}' ({module_id}): {function_count} function(s)");
        }

        // The HAL module: the device's HAL answering as a module
        // (`arora_hal::hal_module`) — its components' models — so a remote
        // reaches it through the same dispatch and finds it in the same method
        // list.
        let hal: Arc<dyn Hal> = match self.hal {
            Some(hal) => Arc::from(hal),
            None => Arc::new(FakeHal::new()),
        };

        // Register each host-side module so its functions dispatch through the
        // engine's `CallBridge`, exactly like a loaded guest module's. Its
        // described functions join the method index, so introspection
        // (`DescribeMethods`) lists them with their signatures.
        let host_modules =
            std::iter::once(hal_module::module(hal.clone())).chain(self.host_modules);
        for module in host_modules {
            for description in module.descriptions() {
                functions.insert(
                    description.id,
                    ModuleFunction {
                        module_id: module.id(),
                        function_id: description.id,
                        function_name: description.name.clone(),
                        function: description.function.clone(),
                    },
                );
            }
            engine.register_module(module.id(), Box::new(module));
        }

        // The methods the behavior interpreter implements itself join the
        // index under its module: a remote spawns one through that module's
        // `SPAWN`, like any task run. A method has one implementation, so one
        // a module describes too fails the build, as does one reusing an id of
        // the interpreter module's own functions.
        let interpreter_methods = self
            .interpreter
            .as_ref()
            .map(|interpreter| interpreter.described_methods())
            .unwrap_or_default();
        let own = [
            interpreter_module::LOAD,
            interpreter_module::EDIT,
            interpreter_module::SPAWN,
            interpreter_module::SPAWN_GRAPH,
            interpreter_module::HALT,
        ];
        for (function_id, export) in &interpreter_methods {
            if let Some(described) = functions.get(function_id) {
                anyhow::bail!(
                    "function {function_id} ('{}') is described by module {} and by the behavior \
                     interpreter",
                    export.name,
                    described.module_id
                );
            }
            if own.contains(function_id) {
                anyhow::bail!(
                    "the behavior interpreter describes '{}' under the id of one of the \
                     interpreter module's own functions ({function_id})",
                    export.name
                );
            }
            let ExportKind::Function(function) = &export.kind;
            functions.insert(
                *function_id,
                ModuleFunction {
                    module_id: interpreter_module::ID,
                    function_id: *function_id,
                    function_name: export.name.clone(),
                    function: function.clone(),
                },
            );
        }

        let store = self
            .store
            .unwrap_or_else(|| Box::new(SimpleDataStore::new()));
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
        let function_index = Rc::new(functions);
        // Default executor: an empty, ready behavior-tree interpreter over the
        // assembled function index. It is injected once here (never swapped); a
        // behavior is loaded into it as a separate step. With none loaded it
        // idles, so an un-configured device ticks a no-op.
        let interpreter = self
            .interpreter
            .unwrap_or_else(|| Box::new(BehaviorTreeInterpreter::new(function_index.clone())));

        // The interpreter as a module: a host module under
        // `interpreter_module::ID` whose `LOAD`/`EDIT`/`SPAWN`/`SPAWN_GRAPH`/
        // `HALT` functions
        // run on the same cell the step loop ticks. A Call to those ids — from a
        // remote or from a behavior — reaches the interpreter through the
        // engine's normal dispatch, like any module function.
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
            .function(interpreter_module::SPAWN, {
                let cell = interpreter.clone();
                move |call| {
                    let (spawned, policy) = interpreter_module::decode_spawn(&call)
                        .map_err(|message| CallError::Guest { message })?;
                    runtime::with_interpreter_value(&cell, |interpreter| {
                        interpreter
                            .spawn(spawned, policy)
                            .map(|handle| interpreter_module::encode_spawn_result(&handle))
                    })
                }
            })
            .function(interpreter_module::SPAWN_GRAPH, {
                let cell = interpreter.clone();
                move |call| {
                    let (graph_type, graph, policy) = interpreter_module::decode_spawn_graph(&call)
                        .map_err(|message| CallError::Guest { message })?;
                    runtime::with_interpreter_value(&cell, |interpreter| {
                        interpreter
                            .spawn_graph(&graph_type, graph, policy)
                            .map(|handle| interpreter_module::encode_spawn_result(&handle))
                    })
                }
            })
            .function(interpreter_module::HALT, {
                let cell = interpreter.clone();
                move |call| {
                    let task = interpreter_module::decode_halt(&call)
                        .map_err(|message| CallError::Guest { message })?;
                    runtime::with_interpreter(&cell, |interpreter| interpreter.halt(task))
                }
            });
        // A method the interpreter implements is a task run, which a direct
        // call has no run to host: the call fails, saying how to reach it.
        let module = interpreter_methods
            .into_iter()
            .fold(module, |module, (function_id, export)| {
                let message = format!(
                    "'{}' is a task run the behavior interpreter implements: spawn it through \
                     the interpreter module",
                    export.name
                );
                module.function(function_id, move |_call| {
                    Err(CallError::Guest {
                        message: message.clone(),
                    })
                })
            })
            .build();
        engine.register_module(module.id(), Box::new(module));

        let mut arora = Arora {
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
            clock: Clock::starting_at(self.start_time),
            behavior_error: watch::Sender::new(None),
        };
        if let Some(xml) = self.groot {
            arora.load_groot(&xml)?;
        }
        Ok(arora)
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

#[cfg(all(not(feature = "native"), target_arch = "wasm32"))]
fn build_engine() -> Result<PinnedEngine> {
    use arora_engine::executor::browser::BrowserExecutor;
    Ok(EngineBuilder::new()
        .add_executor(BrowserExecutor::new())
        .build())
}

/// Without the `native` feature on a native target, the engine carries no
/// executor at all: host modules enter dispatch directly
/// (`Engine::register_module`), so a device that loads no guest executables
/// needs no wasm runtime. This is the slice that builds on every target —
/// including the 32-bit Android ABIs, where wasmtime has no backend. Loading
/// a guest module on it fails at load, loudly.
#[cfg(all(not(feature = "native"), not(target_arch = "wasm32")))]
fn build_engine() -> Result<PinnedEngine> {
    Ok(EngineBuilder::new().build())
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

    const WASM: &[u8] = include_bytes!(env!("CARGO_CDYLIB_FILE_TEST_RUST_WASM_test_rust_wasm"));

    // Function id, as the guest's Rust declaration pins it.
    const SUCCEED: &str = "00cd31a8-2cf4-48e6-a957-69a55de90424"; // () -> bool

    /// The guest's header, from its declaration — what an export step writes
    /// as a `module.yaml`, here handed straight to the engine.
    fn test_module_header() -> Header {
        test_rust_wasm::test_rust_wasm::header(arora_types::module::low::Executor {
            name: "wasm".to_string(),
            min_version: None,
            max_version: None,
        })
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

    /// A module loaded from its Rust declaration indexes every export with the
    /// declaration's own frozen signature — the same entries the crate would
    /// contribute registered in-process — and its functions dispatch to the
    /// guest executable.
    #[test]
    fn with_declared_module_indexes_every_export_from_the_declaration() {
        use test_rust_wasm::test_rust_wasm::Module;

        let mut arora = Arora::builder()
            .with_declared_module::<Module>(
                arora_types::module::low::Executor {
                    name: "wasm".to_string(),
                    min_version: None,
                    max_version: None,
                },
                WASM.to_vec(),
            )
            .build()
            .expect("build a device with a declared wasm module");

        for declared in Module::exports() {
            let indexed = arora
                .function_index
                .get(&declared.id)
                .unwrap_or_else(|| panic!("{} is indexed", declared.name));
            assert_eq!(indexed.module_id, Module::id());
            assert_eq!(indexed.function_name, declared.name);
            assert_eq!(indexed.function, declared.signature);
        }

        let result = arora
            .call(Call {
                module_id: Some(Module::id()),
                id: Uuid::parse_str(SUCCEED).expect("valid uuid"),
                args: Vec::new(),
            })
            .expect("call succeed() on the declared module's guest");
        assert_eq!(result.ret, Value::Boolean(true));
    }

    /// A Groot tree loaded into the built device reaches a declared guest
    /// module's function by name: the tag resolves against the device's own
    /// index, the tree binds its arguments to store keys, and the guest's
    /// return lands in the store through the `_ret` binding.
    #[test]
    fn load_groot_reaches_a_declared_module_function_by_name() {
        use arora_types::data::{Key, StateChange};
        use test_rust_wasm::test_rust_wasm::Module;

        let store = SimpleDataStore::new();
        store
            .write(StateChange::set("angle", Value::F32(0.0)))
            .expect("seed the input");
        let mut arora = Arora::builder()
            .with_data_store(Box::new(store.clone()))
            .with_declared_module::<Module>(
                arora_types::module::low::Executor {
                    name: "wasm".to_string(),
                    min_version: None,
                    max_version: None,
                },
                WASM.to_vec(),
            )
            .build()
            .expect("build a device with a declared wasm module");
        arora
            .load_groot(
                r#"<root main_tree_to_execute="MainTree"><BehaviorTree ID="MainTree">
                     <cos angle="{angle}" res="{cosine}"/>
                   </BehaviorTree></root>"#,
            )
            .expect("the tree names the guest's cos by its declared name");
        arora
            .step(std::time::Duration::from_millis(10))
            .expect("one step ticks the tree");
        assert_eq!(
            store.read(&[Key::from("cosine")]),
            vec![Some(Value::F32(1.0))],
            "cos(0) written back to the bound key"
        );
    }

    /// A tree handed to the builder is resolved against the index `build`
    /// assembled and ticks from the first step — what a device run through
    /// `run` needs, having no built device to load it into.
    #[test]
    fn with_groot_installs_the_tree_at_build() {
        use arora_types::data::{Key, StateChange};
        use test_rust_wasm::test_rust_wasm::Module;

        let store = SimpleDataStore::new();
        store
            .write(StateChange::set("angle", Value::F32(0.0)))
            .expect("seed the input");
        let mut arora = Arora::builder()
            .with_data_store(Box::new(store.clone()))
            .with_declared_module::<Module>(
                arora_types::module::low::Executor {
                    name: "wasm".to_string(),
                    min_version: None,
                    max_version: None,
                },
                WASM.to_vec(),
            )
            .with_groot(
                r#"<root main_tree_to_execute="MainTree"><BehaviorTree ID="MainTree">
                     <cos angle="{angle}" res="{cosine}"/>
                   </BehaviorTree></root>"#,
            )
            .build()
            .expect("the tree resolves against the declared module");
        arora
            .step(std::time::Duration::from_millis(10))
            .expect("one step ticks the tree");
        assert_eq!(
            store.read(&[Key::from("cosine")]),
            vec![Some(Value::F32(1.0))],
            "cos(0) written back to the bound key"
        );
    }

    /// A tree naming a function no module exports fails the build, rather
    /// than leaving a device that idles.
    #[test]
    fn with_groot_fails_the_build_on_an_unknown_leaf() {
        let result = Arora::builder()
            .with_groot(
                r#"<root main_tree_to_execute="MainTree"><BehaviorTree ID="MainTree">
                     <NoSuchLeaf/>
                   </BehaviorTree></root>"#,
            )
            .build();
        assert!(result.is_err(), "an unknown leaf fails the build");
    }

    /// A loaded guest module's exports join the method index with frozen
    /// signatures, so `DescribeMethods` lists them — the guest counterpart of
    /// `described_host_functions_join_the_method_index`. The test module's
    /// exports take and return primitives and optionals over a scalar
    /// primitive only, so all of them are discoverable.
    #[test]
    fn guest_module_exports_join_the_method_index() {
        use arora_types::record::ty::{FrozenOption, FrozenTy, PrimitiveKind};

        let header = test_module_header();
        let module_id = header.id;
        let arora = Arora::builder()
            .with_module(header, WASM.to_vec())
            .build()
            .expect("build a device with a loaded wasm module");

        let by_name = |name: &str| {
            arora
                .function_index
                .values()
                .find(|f| f.function_name == name)
        };

        // `cos(angle: f32) -> f32`: one f32 parameter, f32 return.
        let cos = by_name("cos").expect("cos joined the method index");
        assert_eq!(cos.module_id, module_id);
        assert_eq!(cos.function.parameter_ordering.len(), 1);
        let angle = &cos.function.parameters[&cos.function.parameter_ordering[0]];
        assert_eq!(angle.name, "angle");
        assert_eq!(angle.ty, FrozenTy::from(PrimitiveKind::F32));
        assert_eq!(cos.function.return_ty, FrozenTy::from(PrimitiveKind::F32));

        // `succeed() -> bool`: no parameters, boolean return.
        let succeed = by_name("succeed").expect("succeed joined the method index");
        assert!(succeed.function.parameter_ordering.is_empty());
        assert_eq!(
            succeed.function.return_ty,
            FrozenTy::from(PrimitiveKind::Boolean)
        );

        // `add(a: f32, b: f32) -> f32`: two f32 parameters in order.
        let add = by_name("add").expect("add joined the method index");
        assert_eq!(add.function.parameter_ordering.len(), 2);

        // `window(start_ns: u64, end_ns: Option<u64>) -> Option<u64>`.
        let optional_u64 = FrozenTy::FrozenOption(FrozenOption {
            element: Box::new(FrozenTy::from(PrimitiveKind::U64)),
        });
        let window = by_name("window").expect("window joined the method index");
        let parameter =
            |index: usize| &window.function.parameters[&window.function.parameter_ordering[index]];
        assert_eq!(parameter(0).name, "start_ns");
        assert_eq!(parameter(0).ty, FrozenTy::from(PrimitiveKind::U64));
        assert_eq!(parameter(1).name, "end_ns");
        assert_eq!(parameter(1).ty, optional_u64);
        assert_eq!(window.function.return_ty, optional_u64);

        // `greet(name: Option<String>) -> String`.
        let greet = by_name("greet").expect("greet joined the method index");
        let name = &greet.function.parameters[&greet.function.parameter_ordering[0]];
        assert_eq!(
            name.ty,
            FrozenTy::FrozenOption(FrozenOption {
                element: Box::new(FrozenTy::from(PrimitiveKind::String)),
            })
        );
    }

    /// A guest export is described with the signature its module's record
    /// declares for it: what a host module registering the same declaration
    /// would describe.
    #[test]
    fn a_guest_export_is_described_as_its_module_record_declares_it() {
        use arora_types::record::module::frozen::ExportKind;

        let header = test_module_header();
        let record = test_rust_wasm::test_rust_wasm::record(Uuid::nil());
        assert_eq!(header.exports.len(), record.exports.len());
        for export in &header.exports {
            let low::ExportSymbol::Function(function) = export;
            let ExportKind::Function(declared) = &record.exports[&function.id].kind;
            assert_eq!(
                module_discovery::guest_function_signature(function).as_ref(),
                Some(declared),
                "{}",
                function.name
            );
        }
    }

    /// A guest function taking and returning an optional is listed by
    /// `DescribeMethods` — its signature intact across the value plane — and
    /// `invoke` calls it with the optional present, absent, or explicitly
    /// `None`.
    #[test]
    fn invoke_calls_a_guest_function_with_an_optional_present_or_absent() {
        use arora_bridge::client::method_info;
        use arora_types::record::ty::{FrozenOption, FrozenTy, PrimitiveKind};
        use arora_types::value::Type;

        let mut arora = Arora::builder()
            .with_module(test_module_header(), WASM.to_vec())
            .build()
            .expect("build a device with a loaded wasm module");
        let caller = arora.caller();

        let methods = caller_tests::settle(
            &mut arora,
            caller.describe_methods(Some("window".to_string())),
        )
        .expect("the device describes its methods");
        assert_eq!(methods.len(), 1, "{methods:?}");
        let optional_u64 = FrozenTy::FrozenOption(FrozenOption {
            element: Box::new(FrozenTy::from(PrimitiveKind::U64)),
        });
        assert_eq!(methods[0].function.return_ty, optional_u64);
        let info = method_info(&methods[0]);
        let params: Vec<_> = info
            .params
            .iter()
            .map(|p| (p.name.as_str(), p.param_type.clone(), p.required))
            .collect();
        assert_eq!(
            params,
            vec![
                ("start_ns", Type::U64, true),
                ("end_ns", Type::Option, false)
            ]
        );

        let mut window = |args: Vec<(&str, Value)>| {
            let args = args
                .into_iter()
                .map(|(name, value)| (name.to_string(), value))
                .collect();
            caller_tests::settle(&mut arora, caller.invoke("window", args, None))
                .expect("window answers")
        };
        let some = |value| Value::Option(Some(Box::new(Value::U64(value))));
        assert_eq!(
            window(vec![("start_ns", Value::U64(10)), ("end_ns", some(40))]),
            Invoked::Returned(some(30))
        );
        assert_eq!(
            window(vec![("start_ns", Value::U64(10))]),
            Invoked::Returned(Value::Option(None))
        );
        assert_eq!(
            window(vec![
                ("start_ns", Value::U64(10)),
                ("end_ns", Value::Option(None))
            ]),
            Invoked::Returned(Value::Option(None))
        );
        assert_eq!(
            window(vec![
                ("start_ns", Value::U64(10)),
                ("end_ns", Value::U64(40))
            ]),
            Invoked::Returned(some(30)),
            "a present optional may also be sent bare"
        );

        let mut greet = |args: HashMap<String, Value>| {
            caller_tests::settle(&mut arora, caller.invoke("greet", args, None))
                .expect("greet answers")
        };
        assert_eq!(
            greet(HashMap::from([(
                "name".to_string(),
                Value::Option(Some(Box::new(Value::String("Ada".to_string()))))
            )])),
            Invoked::Returned(Value::String("hello, Ada".to_string()))
        );
        assert_eq!(
            greet(HashMap::new()),
            Invoked::Returned(Value::String("hello".to_string()))
        );
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

    /// A call is enqueued when [`Caller::call`] returns, not when its future
    /// is first polled: firing and stepping in the same breath — a JS Promise
    /// is first polled a microtask later — still lands on that step.
    #[tokio::test]
    async fn a_call_is_enqueued_before_its_future_is_polled() {
        let mut arora = Arora::builder().build().expect("build the default device");
        let caller = arora.caller();
        let call = caller.call(interpreter_module::encode_load(
            &arora_behavior::Graph::empty(),
        ));
        arora
            .step(std::time::Duration::from_millis(10))
            .expect("step");
        let result = call.await.expect("the load call succeeds");
        assert_eq!(result.ret, arora_types::value::Value::Unit);
    }

    /// A failing behavior does not stop the device: the step succeeds, the
    /// standing error is readable through [`Arora::behavior_error`], and the
    /// next successful tick clears it. Nothing reaches the store.
    #[tokio::test]
    async fn a_failing_behavior_is_state_not_a_stop() {
        struct Flaky {
            failures_left: u32,
        }
        impl BehaviorInterpreter for Flaky {
            fn tick(
                &mut self,
                _ctx: &mut arora_behavior::BehaviorContext,
            ) -> Result<arora_behavior::BehaviorStatus, arora_behavior::BehaviorError> {
                if self.failures_left > 0 {
                    self.failures_left -= 1;
                    return Err(arora_behavior::BehaviorError {
                        message: "the input is not there yet".to_string(),
                    });
                }
                Ok(arora_behavior::BehaviorStatus::Running)
            }
        }

        let mut arora = Arora::builder()
            .with_behavior_interpreter(Box::new(Flaky { failures_left: 2 }))
            .build()
            .expect("build the device");
        // The receiver is taken before anything steps — the way an embedder
        // takes one before `run` owns the device.
        let mut errors = arora.behavior_error();
        assert_eq!(*errors.borrow_and_update(), None);

        let changes = arora.store.subscribe();
        arora
            .step(std::time::Duration::from_millis(10))
            .expect("a failing behavior must not fail the step");
        assert!(errors.has_changed().expect("the device is alive"));
        assert_eq!(
            errors.borrow_and_update().as_deref(),
            Some("the input is not there yet"),
            "the standing error is observable"
        );
        for _ in 0..3 {
            arora
                .step(std::time::Duration::from_millis(10))
                .expect("a failing behavior must not fail the step");
        }
        assert_eq!(
            *errors.borrow_and_update(),
            None,
            "recovery clears the standing error"
        );

        // The failure never reaches the store: the steps wrote the clock and
        // nothing else.
        while let Some(change) = changes.try_recv() {
            for key in change.set.keys() {
                assert!(
                    key.path == arora_behavior::built_in::TIME
                        || key.path == arora_behavior::built_in::DT,
                    "unexpected store write: {}",
                    key.path
                );
            }
            assert!(change.unset.is_empty(), "unexpected store unset");
        }
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

    /// Two guest modules with one id fail the build, naming both: the engine
    /// would load the first and the method index describe the last.
    #[test]
    fn two_guest_modules_with_one_id_fail_the_build() {
        let first = test_module_header();
        let mut second = test_module_header();
        second.name = "test-rust-wasm-again".to_string();
        let error = Arora::builder()
            .with_module(first, WASM.to_vec())
            .with_module(second, WASM.to_vec())
            .build()
            .err()
            .expect("one id, two modules is refused");
        assert!(
            error
                .to_string()
                .contains("guest modules 'test-rust-wasm' and 'test-rust-wasm-again' both have id"),
            "{error}"
        );
    }

    /// Devices built from one compiled module each instantiate it and dispatch
    /// it on their own: dropping one leaves the others running, and the
    /// compiled module still builds new devices afterwards.
    #[test]
    fn devices_share_one_compiled_module() {
        let header = test_module_header();
        let module_id = header.id;
        let compiled = CompiledModule::new(header, WASM).expect("the test guest compiles");
        let build = || {
            Arora::builder()
                .with_compiled_module(&compiled)
                .build()
                .expect("build a device from the compiled module")
        };
        let succeed = |arora: &mut Arora| {
            arora
                .call(Call {
                    module_id: Some(module_id),
                    id: Uuid::parse_str(SUCCEED).expect("valid uuid"),
                    args: Vec::new(),
                })
                .expect("call succeed() on the compiled module")
                .ret
        };

        let first = build();
        let mut second = build();
        drop(first);
        assert_eq!(succeed(&mut second), Value::Boolean(true));
        let mut third = build();
        assert_eq!(succeed(&mut third), Value::Boolean(true));
        assert_eq!(succeed(&mut second), Value::Boolean(true));
    }

    /// A compiled module goes through the checks a module given as bytes does:
    /// its exports join the method index, and its id may not repeat another
    /// guest module's.
    #[test]
    fn a_compiled_module_is_checked_as_a_module_given_as_bytes() {
        let compiled =
            CompiledModule::new(test_module_header(), WASM).expect("the test guest compiles");
        let arora = Arora::builder()
            .with_compiled_module(&compiled)
            .build()
            .expect("build a device from the compiled module");
        assert!(
            arora
                .function_index
                .values()
                .any(|f| f.function_name == "cos"),
            "the compiled module's exports join the method index"
        );

        let mut again = test_module_header();
        again.name = "test-rust-wasm-again".to_string();
        let error = Arora::builder()
            .with_compiled_module(&compiled)
            .with_module(again, WASM.to_vec())
            .build()
            .err()
            .expect("one id, two modules is refused");
        assert!(
            error
                .to_string()
                .contains("guest modules 'test-rust-wasm' and 'test-rust-wasm-again' both have id"),
            "{error}"
        );
    }

    /// A compiled module loads into a device built on another thread than the
    /// one that compiled it.
    #[test]
    fn a_compiled_module_loads_on_another_thread() {
        let compiled =
            CompiledModule::new(test_module_header(), WASM).expect("the test guest compiles");
        std::thread::spawn(move || {
            Arora::builder()
                .with_compiled_module(&compiled)
                .build()
                .expect("build a device from the compiled module");
        })
        .join()
        .expect("the device builds on its own thread");
    }

    /// A module whose header declares a function its executable does not
    /// export fails the build, naming the missing export.
    #[test]
    fn a_header_declaring_a_missing_export_fails_the_build() {
        let mut header = test_module_header();
        let low::ExportSymbol::Function(mut missing) = header.exports[0].clone();
        missing.id = Uuid::from_u128(0x278);
        header.exports.push(low::ExportSymbol::Function(missing));
        let error = Arora::builder()
            .with_module(header, WASM.to_vec())
            .build()
            .err()
            .expect("a missing export is refused");
        assert!(
            error.to_string().contains("failed to find function export"),
            "{error}"
        );
    }

    /// An executable that is not WebAssembly fails to compile, before any
    /// device is built from it.
    #[test]
    fn a_malformed_executable_does_not_compile() {
        let error = CompiledModule::new(test_module_header(), &[0xDE, 0xAD, 0xBE, 0xEF])
            .expect_err("not a valid wasm binary");
        assert!(
            matches!(
                error,
                arora_engine::executor::LoadModuleError::MalformedExecutable
            ),
            "{error}"
        );
    }

    /// A module whose executable cannot load fails the whole build, naming the
    /// module, rather than silently yielding a device with a broken module.
    #[test]
    fn a_module_that_fails_to_load_fails_the_build() {
        let header = test_module_header();
        let error = Arora::builder()
            .with_module(header, vec![0xDE, 0xAD, 0xBE, 0xEF]) // not a valid wasm binary
            .build()
            .err()
            .expect("build must fail when a module's executable cannot load");
        assert!(
            error.to_string().contains("module 'test-rust-wasm'"),
            "{error}"
        );
    }
}

/// Host modules enter dispatch directly (no executor), so these hold under
/// every feature slice — including the no-executor engine of a
/// `--no-default-features` native build.
#[cfg(test)]
mod host_module_tests {
    use super::*;
    use arora_types::call::Call;
    use arora_types::record::module::frozen;
    use arora_types::value::Value;

    /// `with_host_module` registers a host-side module built from
    /// [`ModuleBuilder`], and its functions dispatch through [`Arora::call`]
    /// like a loaded module's — the in-process counterpart to `with_module`.
    #[test]
    fn with_host_module_registers_a_native_module_reachable_through_call() {
        use arora_types::call::CallResult;
        use arora_types::value::StructureField;

        let module_id = Uuid::from_u128(0xA11CE);
        let echo = Uuid::from_u128(0xEC40);
        // A host module whose one function echoes its first argument back.
        let module = ModuleBuilder::new(module_id)
            .function(echo, |call| {
                let ret = call
                    .args
                    .into_iter()
                    .next()
                    .map(|field| *field.value)
                    .unwrap_or(Value::Unit);
                Ok(CallResult {
                    ret,
                    mutated: Vec::new(),
                })
            })
            .build();
        let mut arora = Arora::builder()
            .with_host_module(module)
            .build()
            .expect("build a device with a host module");

        let result = arora
            .call(Call {
                module_id: Some(module_id),
                id: echo,
                args: vec![StructureField {
                    id: Uuid::from_u128(1),
                    value: Box::new(Value::U32(42)),
                }],
            })
            .expect("call echo() on the host module");
        assert_eq!(result.ret, Value::U32(42));
    }

    /// A described host function joins the method index with its signature —
    /// what introspection (`DescribeMethods`) serves — while an undescribed
    /// one dispatches but stays out of the index.
    #[test]
    fn described_host_functions_join_the_method_index() {
        use arora_types::call::CallResult;
        use arora_types::record::module::frozen::{Function, Parameter};
        use arora_types::record::ty::{FrozenTy, PrimitiveKind};
        use std::collections::HashMap;

        let module_id = Uuid::from_u128(0x6761);
        let described = Uuid::from_u128(0x6c61);
        let undescribed = Uuid::from_u128(0x6c62);
        let param = Uuid::from_u128(0x7801);

        let signature = Function {
            parameters: HashMap::from([(
                param,
                Parameter {
                    name: "x".to_string(),
                    ty: FrozenTy::from(PrimitiveKind::F64),
                    mutable: false,
                },
            )]),
            parameter_ordering: vec![param],
            return_ty: FrozenTy::from(PrimitiveKind::Unit),
        };
        let unit = |_call: Call| {
            Ok(CallResult {
                ret: Value::Unit,
                mutated: Vec::new(),
            })
        };
        let module = ModuleBuilder::new(module_id)
            .described_function(described, "look_at", signature.clone(), unit)
            .function(undescribed, unit)
            .build();

        let arora = Arora::builder()
            .with_host_module(module)
            .build()
            .expect("build a device with a described host module");

        let entry = arora
            .function_index
            .get(&described)
            .expect("the described function is indexed");
        assert_eq!(entry.module_id, module_id);
        assert_eq!(entry.function_name, "look_at");
        assert_eq!(entry.function, signature);
        assert!(arora.function_index.get(&undescribed).is_none());
    }

    /// An interpreter hosting `look_at` as a task run of its own.
    struct Describing;

    const LOOK_AT: Uuid = Uuid::from_u128(0x6c6f6f6b);

    impl BehaviorInterpreter for Describing {
        fn tick(
            &mut self,
            _ctx: &mut arora_behavior::BehaviorContext,
        ) -> Result<arora_behavior::BehaviorStatus, arora_behavior::BehaviorError> {
            Ok(arora_behavior::BehaviorStatus::Running)
        }

        fn described_methods(&self) -> HashMap<Uuid, frozen::Export> {
            HashMap::from([(
                LOOK_AT,
                frozen::Export {
                    name: "look_at".to_string(),
                    kind: frozen::ExportKind::Function(unit_signature()),
                },
            )])
        }
    }

    fn unit_signature() -> frozen::Function {
        frozen::Function {
            parameters: HashMap::new(),
            parameter_ordering: Vec::new(),
            return_ty: arora_types::record::ty::FrozenTy::from(
                arora_types::record::ty::PrimitiveKind::Unit,
            ),
        }
    }

    /// A method the interpreter describes joins the method index under the
    /// interpreter module, and a direct call to it says to spawn it.
    #[test]
    fn the_interpreter_s_methods_join_the_method_index() {
        let mut arora = Arora::builder()
            .with_behavior_interpreter(Box::new(Describing))
            .build()
            .expect("build a device whose interpreter describes a method");

        let entry = arora
            .function_index
            .get(&LOOK_AT)
            .expect("the interpreter's method is indexed");
        assert_eq!(entry.module_id, interpreter_module::ID);
        assert_eq!(entry.function_name, "look_at");
        assert_eq!(entry.function, unit_signature());

        let error = arora
            .call(Call {
                module_id: Some(interpreter_module::ID),
                id: LOOK_AT,
                args: Vec::new(),
            })
            .expect_err("a task run is not called directly");
        assert!(
            error
                .to_string()
                .contains("spawn it through the interpreter module"),
            "{error}"
        );
    }

    /// A method has one implementation: a module and the interpreter both
    /// describing one function id fail the build.
    #[test]
    fn a_method_described_by_a_module_and_the_interpreter_fails_the_build() {
        let module = ModuleBuilder::new(Uuid::from_u128(0x6761))
            .described_function(LOOK_AT, "look_at", unit_signature(), |_call| {
                Ok(CallResult {
                    ret: Value::Unit,
                    mutated: Vec::new(),
                })
            })
            .build();
        let error = Arora::builder()
            .with_host_module(module)
            .with_behavior_interpreter(Box::new(Describing))
            .build()
            .err()
            .expect("the build is refused");
        assert!(error.to_string().contains("described by module"), "{error}");
    }
}

#[cfg(all(test, feature = "native"))]
mod stepping_tests;

/// The [`LocalCaller`]'s client operations on a device built and stepped in
/// process: each is enqueued when the method returns and answered by the steps
/// that follow.
#[cfg(test)]
mod caller_tests {
    use super::*;
    use arora_behavior::Status;
    use arora_types::data::Key;
    use arora_types::record::module::frozen::{Function, Parameter};
    use arora_types::record::ty::{FrozenScalar, FrozenTy, PrimitiveKind};
    use arora_types::record::{FrozenReference, Version};
    use std::pin::pin;
    use std::task::{Context, Poll};
    use std::time::Duration;

    const TOOLS: Uuid = Uuid::from_u128(0x7001);
    const DOUBLE: Uuid = Uuid::from_u128(0x7002);
    const DOUBLE_X: Uuid = Uuid::from_u128(0x7003);
    const WAVE: Uuid = Uuid::from_u128(0x7004);
    const TOOLS_STOP: Uuid = Uuid::from_u128(0x7005);
    const PLAYER: Uuid = Uuid::from_u128(0x7101);
    const PLAYER_STOP: Uuid = Uuid::from_u128(0x7102);

    /// Step `arora` until `future` resolves, polling it after each step.
    pub(super) fn settle<T>(arora: &mut Arora, future: impl Future<Output = T>) -> T {
        let mut future = pin!(future);
        let mut cx = Context::from_waker(futures::task::noop_waker_ref());
        for _ in 0..8 {
            arora.step(Duration::from_millis(10)).expect("step");
            if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
                return output;
            }
        }
        panic!("no answer after eight steps");
    }

    fn signature(parameters: &[(Uuid, &str)], return_ty: FrozenTy) -> Function {
        Function {
            parameters: parameters
                .iter()
                .map(|(id, name)| {
                    (
                        *id,
                        Parameter {
                            name: name.to_string(),
                            ty: FrozenTy::from(PrimitiveKind::F64),
                            mutable: false,
                        },
                    )
                })
                .collect(),
            parameter_ordering: parameters.iter().map(|(id, _)| *id).collect(),
            return_ty,
        }
    }

    fn status() -> FrozenTy {
        FrozenTy::FrozenScalar(FrozenScalar {
            reference: FrozenReference {
                id: arora_behavior::STATUS_ENUMERATION_ID,
                version: Version::parse("1.0.0").expect("a valid version"),
            },
        })
    }

    fn answer(ret: Value) -> Result<CallResult, CallError> {
        Ok(CallResult {
            ret,
            mutated: Vec::new(),
        })
    }

    /// A device whose `tools` module exports `double(x: f64) -> f64`, the
    /// task-shaped `wave() -> Status` (running until halted) and `stop()`, and
    /// whose `player` module exports a `stop()` of its own.
    fn device() -> Arora {
        let tools = ModuleBuilder::new(TOOLS)
            .described_function(
                DOUBLE,
                "double",
                signature(&[(DOUBLE_X, "x")], FrozenTy::from(PrimitiveKind::F64)),
                |call| match call.args.first().map(|field| field.value.as_ref()) {
                    Some(Value::F64(x)) => answer(Value::F64(2.0 * x)),
                    other => Err(generic(format!("double takes an f64, not {other:?}"))),
                },
            )
            .described_function(WAVE, "wave", signature(&[], status()), |_call| {
                answer(Status::Running.into())
            })
            .described_function(
                TOOLS_STOP,
                "stop",
                signature(&[], FrozenTy::from(PrimitiveKind::Unit)),
                |_call| answer(Value::String("tools stopped".to_string())),
            )
            .build();
        let player = ModuleBuilder::new(PLAYER)
            .described_function(
                PLAYER_STOP,
                "stop",
                signature(&[], FrozenTy::from(PrimitiveKind::Unit)),
                |_call| answer(Value::String("player stopped".to_string())),
            )
            .build();
        Arora::builder()
            .with_host_module(tools)
            .with_host_module(player)
            .build()
            .expect("build the device")
    }

    #[test]
    fn list_keys_reports_a_key_with_its_meta() {
        let mut arora = device();
        let caller = arora.caller();
        let key = Key::from("face/mouth");
        arora
            .store()
            .set_meta(HashMap::from([(
                key.clone(),
                KeyMeta::new().editable().in_unit("fraction"),
            )]))
            .expect("describe the key");

        let keys = settle(&mut arora, caller.list_keys(Some("face/".to_string())))
            .expect("the device lists its keys");
        assert_eq!(keys.len(), 1, "{keys:?}");
        let (path, meta) = &keys[0];
        assert_eq!(path, "face/mouth");
        assert!(meta.editable);
        assert_eq!(meta.unit.as_deref(), Some("fraction"));
    }

    #[test]
    fn describe_methods_lists_a_host_module_s_described_function() {
        let mut arora = device();
        let caller = arora.caller();
        let methods = settle(&mut arora, caller.describe_methods(Some("dou".to_string())))
            .expect("the device describes its methods");
        assert_eq!(methods.len(), 1, "{methods:?}");
        assert_eq!(methods[0].module_id, TOOLS);
        assert_eq!(methods[0].id, DOUBLE);
        assert_eq!(methods[0].name, "double");
        assert_eq!(methods[0].function.parameter_ordering, vec![DOUBLE_X]);
    }

    #[test]
    fn invoking_a_plain_function_returns_its_value() {
        let mut arora = device();
        let caller = arora.caller();
        let invoked = settle(
            &mut arora,
            caller.invoke(
                "double",
                HashMap::from([("x".to_string(), Value::F64(2.5))]),
                None,
            ),
        )
        .expect("double answers");
        assert_eq!(invoked, Invoked::Returned(Value::F64(5.0)));

        let error = settle(
            &mut arora,
            caller.invoke(
                "double",
                HashMap::from([("y".to_string(), Value::F64(2.5))]),
                None,
            ),
        )
        .expect_err("double has no parameter y");
        assert!(error.to_string().contains("'y'"), "{error}");
    }

    /// A task-shaped method starts a run: its status key reads Running once a
    /// step has ticked it, and a halt ends it.
    #[test]
    fn invoking_a_task_shaped_method_starts_a_run_a_halt_ends() {
        let mut arora = device();
        let caller = arora.caller();
        let Invoked::Started(handle) =
            settle(&mut arora, caller.invoke("wave", HashMap::new(), None)).expect("wave starts")
        else {
            panic!("wave is task-shaped");
        };
        arora.step(Duration::from_millis(10)).expect("step");
        let running: Value = Status::Running.into();
        assert_eq!(
            arora.store().read(std::slice::from_ref(&handle.status)),
            vec![Some(running)]
        );

        settle(&mut arora, caller.halt(handle.id)).expect("the halt is applied");
        arora.step(Duration::from_millis(10)).expect("step");
        let failure: Value = Status::Failure.into();
        assert_eq!(
            arora.store().read(std::slice::from_ref(&handle.status)),
            vec![Some(failure)],
            "a halted run ends Failure"
        );
    }

    /// `spawn_graph` starts a graph as a run: each step it calls a module
    /// function and writes the return under a key, until a halt ends it.
    #[test]
    fn spawn_graph_runs_a_graph_every_step_until_halted() {
        use arora_behavior::graph::{Graph, Io, Link, LinkSource, Node, Port};
        use arora_behavior_tree::nodes::{PARALLEL_FUNCTION_ID, RUN_FUNCTION_ID};
        use arora_behavior_tree::schema::_RET_PARAM_ID;

        let [parallel, double, run] = [0x1, 0x2, 0x3].map(Uuid::from_u128);
        let out = Uuid::from_u128(0x4);
        let mut graph = Graph::empty();
        graph.root = Some(parallel);
        graph.nodes.insert(
            parallel,
            Node {
                id: parallel,
                function: PARALLEL_FUNCTION_ID,
                children: Some(vec![double, run]),
                ..Node::default()
            },
        );
        graph.nodes.insert(
            double,
            Node {
                id: double,
                function: DOUBLE,
                inputs: vec![Io::new(DOUBLE_X), Io::new(_RET_PARAM_ID)],
                ..Node::default()
            },
        );
        graph.nodes.insert(
            run,
            Node {
                id: run,
                function: RUN_FUNCTION_ID,
                ..Node::default()
            },
        );
        graph.links = vec![
            Link::new(
                Port::new(double, DOUBLE_X),
                LinkSource::Literal(Value::F64(2.0)),
            ),
            Link::new(Port::new(double, _RET_PARAM_ID), LinkSource::Variable(out)),
        ];
        graph.variables.insert(out, "tools/doubled".to_string());

        let mut arora = device();
        let caller = arora.caller();
        let graph_type = arora_behavior_tree::behavior::graph_type();
        let handle = settle(&mut arora, caller.spawn_graph(&graph_type, &graph))
            .expect("the graph starts as a run");
        arora.step(Duration::from_millis(10)).expect("step");
        let running: Value = Status::Running.into();
        assert_eq!(
            arora
                .store()
                .read(&[handle.status.clone(), Key::new("tools/doubled")]),
            vec![Some(running), Some(Value::F64(4.0))],
            "the run is running and its module call wrote its key"
        );

        settle(&mut arora, caller.halt(handle.id)).expect("the halt is applied");
        arora.step(Duration::from_millis(10)).expect("step");
        let failure: Value = Status::Failure.into();
        assert_eq!(
            arora.store().read(std::slice::from_ref(&handle.status)),
            vec![Some(failure)],
            "a halted run ends Failure"
        );
    }

    /// `spawn` takes a call by ids and answers with the run's handle.
    #[test]
    fn spawn_starts_a_run_from_a_call() {
        let mut arora = device();
        let caller = arora.caller();
        let handle = settle(
            &mut arora,
            caller.spawn(Call {
                module_id: Some(TOOLS),
                id: WAVE,
                args: Vec::new(),
            }),
        )
        .expect("the run starts");
        arora.step(Duration::from_millis(10)).expect("step");
        let running: Value = Status::Running.into();
        assert_eq!(
            arora.store().read(std::slice::from_ref(&handle.status)),
            vec![Some(running)]
        );
    }

    /// A name two modules export calls neither until the module is named, and
    /// the refusal names both.
    #[test]
    fn invoking_a_shared_name_needs_its_module() {
        let mut arora = device();
        let caller = arora.caller();
        let error = settle(&mut arora, caller.invoke("stop", HashMap::new(), None))
            .expect_err("stop is ambiguous");
        let message = error.to_string();
        assert!(message.contains(&TOOLS.to_string()), "{message}");
        assert!(message.contains(&PLAYER.to_string()), "{message}");

        let invoked = settle(
            &mut arora,
            caller.invoke("stop", HashMap::new(), Some(PLAYER)),
        )
        .expect("the module disambiguates");
        assert_eq!(
            invoked,
            Invoked::Returned(Value::String("player stopped".to_string()))
        );
    }
}
