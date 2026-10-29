//! Opinionated Arora runtime.
//!
//! Where [`arora_engine`] is the bare, unopinionated runtime, this crate wires
//! a ready-to-use [`Arora`]: the whole device in one object — the engine (with
//! the component and native executors), the shared data store, the HAL and
//! bridge I/O seams, one behavior interpreter, and the step loop that drives
//! them. The core behavior-tree nodes — the control nodes and the string nodes
//! (`SetString`, `UnsetString`, `IsStringSet`, `WaitStringSet`, `RegexMatch`)
//! — are wired natively into [`arora_behavior_tree`], so no module needs to be
//! loaded to run a tree of them.
//!
//! Build one with the [`builder`](Arora::builder): pick a data store, a HAL,
//! zero or more bridges, the modules whose functions behaviors may call, and
//! the behavior interpreter — each with a sensible default, so
//! `Arora::builder().build()` yields a self-contained in-process device (fake
//! HAL, no bridge, an empty behavior, a private [`SimpleDataStore`]). Then
//! drive it with [`step`](Arora::step) (once per frame) or
//! [`run`](Arora::run) (the visible loop over `step`).

/// The device a run is: its name, its directory, its local bridge's port.
#[cfg(feature = "native")]
pub mod device;
/// A device's directory: what the device keeps of its own from one run to the
/// next, each use in a subdirectory of its own.
#[cfg(feature = "native")]
pub mod device_dir;
mod hal_module;
mod interpreter_signatures;
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
pub use device::Device;
#[cfg(feature = "native")]
pub use run::{
    local_ws_bridge, local_ws_bridge_with, run, run_with, run_with_frontend, run_with_hal,
    serve_local_ws_bridge, standard_frontend, DeviceCli, RunnerArgs,
};
pub use runtime::{list_keys, RuntimeError};

/// Re-exported so an embedder can name the device's method index — what a
/// Groot tree's tags resolve against and what each tick hands the
/// interpreter — and its entries.
pub use arora_behavior::{FunctionIndex, ModuleFunction};
/// Re-exported so embedders can construct the default behavior executor — an
/// empty, ready [`BehaviorTreeInterpreter`] — and load a behavior into it before
/// injecting it with [`AroraBuilder::with_behavior_interpreter`].
pub use arora_behavior_tree::behavior::BehaviorTreeInterpreter;

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
use arora_engine::engine::{
    EngineBuilder, LoadModuleError, PinnedEngine, ReloadModuleError, UnloadModuleError,
};
#[cfg(feature = "native")]
use arora_engine::executor::{component::ComponentExecutor, native::NativeExecutor};
/// Re-exported so an embedder can set what a guest module may spend
/// ([`AroraBuilder::with_guest_limits`], [`Arora::set_guest_limits`]).
pub use arora_engine::limits::GuestLimits;
/// Re-exported so an embedder can assemble a host-side module — a set of
/// in-process functions under a module id — and inject it with
/// [`AroraBuilder::with_host_module`].
pub use arora_engine::module::{FunctionDescription, HostModule, ModuleBuilder};
use arora_hal::{FakeHal, Hal, KeyDescriptions, UpdatesStream};
/// Re-exported so an embedder can fill the registry it hands the device with
/// [`AroraBuilder::with_registry`].
pub use arora_registry::local::LocalRegistry;
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::{Call, CallBridge, CallError, CallResult};
use arora_types::data::{DataStore, Key, KeyMeta, StateChange, Subscription};
use arora_types::module::declared::AroraModule;
use arora_types::module::low::{self, Header};
use arora_types::record::module::frozen::ExportKind;
use arora_types::record::FrozenReference;
use arora_types::value::Value;
use futures::channel::{mpsc, oneshot};
use futures::stream::{self, Fuse, SelectAll};
use futures::StreamExt;
use runtime::{Clock, Pending};
use std::cell::{Cell, RefCell};
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
    /// The device's method index: each described function of its modules —
    /// guest, host and the interpreter's — by function id. It is what
    /// `DescribeMethods` lists, what a Groot tree's tags resolve against, and
    /// what each tick hands the interpreter. The core nodes (control and
    /// string nodes) are native and not in it. It changes only between steps,
    /// when a module is loaded, reloaded or unloaded.
    pub(crate) function_index: FunctionIndex,
    /// The guest modules the device loaded, by module id, each with the ids of
    /// the functions its header exports: what [`Arora::reload_module`] and
    /// [`Arora::unload_module`] act on. Host modules (the HAL's, the
    /// interpreter's, an embedder's) are not here: they leave with the device.
    pub(crate) guest_modules: HashMap<Uuid, Vec<Uuid>>,
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
    /// The task runs started through the interpreter module that have not yet
    /// ended, each step reading whether they did, to log how.
    pub(crate) task_runs: runtime::TaskRuns,
    /// Set by [`load_groot`](Arora::load_groot) around its load, so the
    /// interpreter module's `LOAD` logs that the behavior came from a Groot
    /// tree.
    pub(crate) loading_groot: Rc<Cell<bool>>,
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
    // What the HAL says of its keys (`Hal::describe_keys`), owned by this
    // device: what it held at build is already in the store, and each step
    // states what it has since delivered. Fused: the HAL may end it.
    pub(crate) hal_keys: Fuse<KeyDescriptions>,
    // The meta the device stated from the HAL's descriptions, per key: a later
    // description replaces it, and never meta a host stated itself.
    pub(crate) hal_stated: HashMap<Key, KeyMeta>,
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
    // behavior ticks; those writes travel outbound like any other change, so a
    // remote can derive the device's step rate from them.
    pub(crate) clock: Clock,
}

impl Arora {
    /// Start assembling an [`Arora`]. Every seam has a default, so the shortest
    /// device is `Arora::builder().build()`.
    pub fn builder() -> AroraBuilder {
        AroraBuilder::default()
    }

    /// Set what the guest module `module` may spend, in place of the device's
    /// [`GuestLimits`] ([`AroraBuilder::with_guest_limits`]): its budget holds
    /// from its next call, its memory cap from its next load or reload.
    pub fn set_guest_limits(&mut self, module: Uuid, limits: GuestLimits) {
        self.engine.set_guest_limits(module, limits);
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
    /// core nodes and every loaded module's exports — and the lowered graph
    /// is loaded through the interpreter module, the path a remote's load
    /// takes. Call it between steps, after [`build`](AroraBuilder::build); the
    /// tree binds to the store at its first tick.
    pub fn load_groot(&mut self, xml: &str) -> Result<()> {
        let tree = arora_behavior_tree::schema_groot::BehaviorTree::try_from_groot_xml(xml)
            .map_err(|e| anyhow::anyhow!("the Groot tree does not parse: {e:?}"))?;
        let graph = tree
            .into_graph(&self.function_index)
            .map_err(|e| anyhow::anyhow!("the Groot tree does not lower: {e:?}"))?;
        self.loading_groot.set(true);
        let loaded = self.call(interpreter_module::encode_load(&graph));
        self.loading_groot.set(false);
        loaded.map_err(|e| anyhow::anyhow!("the behavior did not load: {e}"))?;
        Ok(())
    }

    /// Load a guest module into the running device: its functions dispatch
    /// from the next call, and those [`with_module`](AroraBuilder::with_module)
    /// would describe join the method index. Refused when a module — guest or
    /// host — is already loaded under its id (one module id, one module:
    /// [`reload_module`](Self::reload_module) replaces one), when it exports a
    /// function id another module's function has, or when it does not load.
    ///
    /// Call it between steps, like [`call`](Self::call).
    pub fn load_module(&mut self, module: &CompiledModule) -> Result<(), ModuleError> {
        let header = module.header();
        let id = header.id;
        if self.engine.contains_module(&id) {
            return Err(ModuleError::AlreadyLoaded { id });
        }
        self.check_function_ids(header)?;
        self.engine
            .load_compiled_module(module)
            .map_err(|error| ModuleError::Load { id, error })?;
        self.function_index
            .extend(freeze_exports(header, |_| false).map(|f| (f.function_id, f)));
        self.guest_modules.insert(id, export_ids(header));
        log::info!(
            "loaded module '{}' ({id}): {} function(s)",
            header.name,
            header.exports.len()
        );
        Ok(())
    }

    /// Replace the guest module loaded under `module`'s id with `module` — a
    /// new version, or the same one rebuilt — in one step: from the next call,
    /// its functions run the new code in a fresh instance, and the method
    /// index describes the new header's exports in place of the old one's, as
    /// [`with_module`](AroraBuilder::with_module) describes a header's: an
    /// export whose types a header cannot freeze dispatches but is not
    /// described, even when the module was first loaded with
    /// [`with_declared_module`](AroraBuilder::with_declared_module).
    ///
    /// All or nothing: refused, with the old module still loaded, serving and
    /// described, when no guest module is loaded under the id, when the loaded
    /// module's executor cannot unload it (a native library: restart the
    /// device instead), when the new version exports a function id another
    /// module's function has, or when it does not load.
    ///
    /// Once replaced:
    /// - the behavior keeps running as it was: a tree resolved the module when
    ///   it was lowered, and its next call reaches the new instance, which
    ///   holds none of the old one's state. Nothing checks that the new
    ///   version still has what the tree calls: a function it dropped fails
    ///   the node calling it, as any missing function does.
    /// - what a remote or an in-process caller asked of the module and the
    ///   device has not finished is interrupted: a call to it received before
    ///   the reload and not yet applied fails, and so does a request to start
    ///   a task run calling it; a task run calling it is halted, its status
    ///   key reporting it ended.
    ///
    /// Call it between steps, like [`call`](Self::call).
    pub fn reload_module(&mut self, module: &CompiledModule) -> Result<(), ModuleError> {
        let header = module.header();
        let id = header.id;
        let Some(old) = self.guest_modules.get(&id).cloned() else {
            return Err(ModuleError::NotLoaded { id });
        };
        self.check_function_ids(header)?;
        self.engine.reload_module(module).map_err(|e| match e {
            ReloadModuleError::ModuleNotFound => ModuleError::NotLoaded { id },
            ReloadModuleError::Unsupported { executor } => {
                ModuleError::Unsupported { id, executor }
            }
            ReloadModuleError::Load(error) => ModuleError::Load { id, error },
        })?;
        self.function_index
            .retain(|_, function| function.module_id != id);
        self.function_index
            .extend(freeze_exports(header, |_| false).map(|f| (f.function_id, f)));
        self.guest_modules.insert(id, export_ids(header));
        log::info!(
            "reloaded module '{}' ({id}): {} function(s)",
            header.name,
            header.exports.len()
        );
        self.interrupt(
            id,
            &old,
            &format!("module '{}' ({id}) was reloaded", header.name),
        );
        Ok(())
    }

    /// Unload the guest module loaded under `id`: its functions leave the
    /// method index and dispatch, and its instance is dropped. A call to it
    /// then fails as a call to a module the device does not have, and so does
    /// a tree node calling it.
    ///
    /// Refused for a module the device did not load as a guest — the HAL's,
    /// the interpreter's, and an embedder's host modules stay for the
    /// device's life — and for one whose executor cannot unload it (a native
    /// library: restart the device instead). What a remote or an in-process
    /// caller asked of the module and the device has not finished is
    /// interrupted, as by [`reload_module`](Self::reload_module).
    ///
    /// Call it between steps, like [`call`](Self::call).
    pub fn unload_module(&mut self, id: Uuid) -> Result<(), ModuleError> {
        let Some(exports) = self.guest_modules.get(&id).cloned() else {
            return Err(ModuleError::NotLoaded { id });
        };
        self.engine.unload_module(&id).map_err(|e| match e {
            UnloadModuleError::Unsupported { executor } => {
                ModuleError::Unsupported { id, executor }
            }
            UnloadModuleError::ModuleNotFound | UnloadModuleError::Internal(_) => {
                ModuleError::NotLoaded { id }
            }
        })?;
        self.function_index
            .retain(|_, function| function.module_id != id);
        self.guest_modules.remove(&id);
        log::info!("unloaded module {id}");
        self.interrupt(id, &exports, &format!("module {id} was unloaded"));
        Ok(())
    }

    /// Interrupt what callers asked of module `id`, whose functions were
    /// `exports`, because of `why`: fail each command calling it that the
    /// device received and has not applied, and halt each task run calling
    /// it.
    fn interrupt(&mut self, id: Uuid, exports: &[Uuid], why: &str) {
        runtime::sweep_now(&mut self.hal_feed, &mut self.inbound, &mut self.pending);
        let calls_module = |call: &Call| runtime::calls_module(call, id, exports);
        let events = std::mem::take(&mut self.pending.events);
        for (endpoint, event) in events {
            match event {
                Inbound::Command(command) if matches!(&command.op, BridgeOp::Call(call) if calls_module(call)) =>
                {
                    command.reply(Err(format!("call interrupted: {why}")));
                }
                event => self.pending.events.push((endpoint, event)),
            }
        }
        let mut interpreter = self.interpreter.borrow_mut();
        for run in self.task_runs.borrow_mut().iter_mut() {
            if run.interrupted.is_some() || !run.calls_module(id, exports) {
                continue;
            }
            if let Some(interpreter) = interpreter.as_mut() {
                if let Err(e) = interpreter.halt(run.id) {
                    log::warn!("task {} could not be halted: {e}", run.id.0);
                    continue;
                }
            }
            run.halted = true;
            run.interrupted = Some(why.to_string());
        }
    }

    /// Refuse a header exporting a function id another module's function has:
    /// a call names its function by id, so one id is one function.
    fn check_function_ids(&self, header: &Header) -> Result<(), ModuleError> {
        for function in export_ids(header) {
            let owner = self
                .function_index
                .get(&function)
                .map(|f| f.module_id)
                .or_else(|| {
                    self.guest_modules
                        .iter()
                        .find(|(_, exports)| exports.contains(&function))
                        .map(|(module, _)| *module)
                });
            if let Some(owner) = owner.filter(|owner| *owner != header.id) {
                return Err(ModuleError::FunctionTaken {
                    module: header.id,
                    function,
                    owner,
                });
            }
        }
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

    /// The device's callable methods whose name starts with `prefix` (all of
    /// them when `None`), sorted by name, each with its full signature: what
    /// a `DescribeMethods` from a remote or a [`LocalCaller`] answers, read
    /// now rather than at a step. The method index changes only through
    /// [`load_module`](Self::load_module), [`reload_module`](Self::reload_module)
    /// and [`unload_module`](Self::unload_module), so what this reads holds
    /// until one of them is called.
    pub fn describe_methods(&self, prefix: Option<&str>) -> Vec<MethodSignature> {
        runtime::describe_methods(&self.function_index, prefix)
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

/// Why [`Arora::load_module`], [`Arora::reload_module`] or
/// [`Arora::unload_module`] refused: the device's modules are as they were.
#[derive(Debug, Clone)]
pub enum ModuleError {
    /// A module — guest or host — is already loaded under the id.
    AlreadyLoaded { id: Uuid },
    /// No guest module is loaded under the id.
    NotLoaded { id: Uuid },
    /// Module `module` exports `function`, which module `owner` has: a call
    /// names its function by id, so one id is one function.
    FunctionTaken {
        module: Uuid,
        function: Uuid,
        owner: Uuid,
    },
    /// The executor that loaded the module cannot unload it (a native
    /// library): restarting the device is how it changes.
    Unsupported { id: Uuid, executor: &'static str },
    /// The module did not load.
    Load { id: Uuid, error: LoadModuleError },
}

impl std::fmt::Display for ModuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModuleError::AlreadyLoaded { id } => {
                write!(f, "a module is already loaded under id {id}")
            }
            ModuleError::NotLoaded { id } => write!(f, "no guest module is loaded under id {id}"),
            ModuleError::FunctionTaken {
                module,
                function,
                owner,
            } => write!(
                f,
                "module {module} exports function {function}, which module {owner} has"
            ),
            ModuleError::Unsupported { id, executor } => write!(
                f,
                "module {id} runs on the {executor} executor, which cannot unload it: restart \
                 the device to change it"
            ),
            ModuleError::Load { id, error } => write!(f, "module {id} did not load: {error}"),
        }
    }
}

impl std::error::Error for ModuleError {}

/// The in-process client of a device: everything a remote client asks of it
/// over a bridge — writing its inputs, calling a function, listing its keys,
/// describing its methods, invoking one by name, starting and halting a run —
/// from the same process, including while [`run`](Arora::run) owns it. Obtained from
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

    /// Write `change` to the device's store, as a remote's write: checked
    /// against the store's key meta like every bridge's, so a key that is not
    /// one of the device's inputs (`KeyMeta::editable`), or a value not of its
    /// key's declared type, refuses the whole change and writes nothing.
    pub fn update(
        &self,
        change: StateChange,
    ) -> impl Future<Output = Result<(), CallError>> + Send + 'static {
        let reply = self.ask(BridgeOp::Update(change));
        async move { reply.await.map(|_| ()) }
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
    registry: LocalRegistry,
    /// The records the module directories carry (`with_module_dirs`), kept
    /// apart from `registry` and merged into it at build, so the order of
    /// `with_registry` and `with_module_dirs` does not matter.
    #[cfg(feature = "native")]
    module_dir_records: LocalRegistry,
    host_modules: Vec<HostModule>,
    guest_limits: GuestLimits,
    module_limits: Vec<(Uuid, GuestLimits)>,
    groot: Option<String>,
    #[cfg(feature = "native")]
    step_period: Option<std::time::Duration>,
    #[cfg(feature = "native")]
    frontend: Option<operator::Frontend>,
    /// The device a runner said the run is.
    #[cfg(feature = "native")]
    device: Option<Device>,
    /// What a runner said of Semio Studio; none serves the open local bridge.
    #[cfg(feature = "studio-bridge")]
    studio: Option<studio::Choice>,
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
    /// for a tree whose leaves name loaded modules, with
    /// [`with_groot`](Self::with_groot) or, once built, [`Arora::load_groot`],
    /// which resolve them against the device's index. Default
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
    /// expects: a `.wasm` component for the component executor, or a native dynamic
    /// library for the native executor. The engine selects the executor by the
    /// name the header announces, so this one seam loads either format. The
    /// module is loaded at [`build`](Self::build); once loaded, its exported
    /// functions dispatch to guest code through the engine's `CallBridge` —
    /// what a behavior reaches to call them.
    ///
    /// Its exports join the method index from the module's record when the
    /// device's registry holds it ([`with_registry`](Self::with_registry)),
    /// every export with its frozen signature; without one, those whose
    /// parameters and return are primitives, frozen from the header.
    ///
    /// [`build`](Self::build) fails, naming both, when another module — guest
    /// or host — has the header's id.
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
    /// [`with_module`](Self::with_module) alone can index only primitive-typed
    /// exports, because a header carries no type versions to freeze a structure
    /// or an enumeration against; the module's record, in the device's
    /// registry ([`with_registry`](Self::with_registry)), carries them. The
    /// declaration does too: `M::exports()` hands
    /// each function's frozen signature, so an export of any type — a
    /// `Status`-returning behavior leaf included — is discoverable and reachable
    /// from a behavior tree, exactly as the same crate registered in-process
    /// with [`with_host_module`](Self::with_host_module)
    /// (`HostModule::of::<M>()`) would be. `executor` names how `executable`
    /// runs — `"wasm-component"` for the `wasm32-wasip2` build of the
    /// declaring crate.
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

    /// Give the device `registry`: the records its guest modules are
    /// described by. Looked up by exact id and version only, from memory —
    /// never tagged, resolved by path, or fetched.
    ///
    /// A guest module whose module record the registry holds — under the
    /// module's id, at the version its header carries — has every export
    /// indexed with the record's frozen signature, as
    /// [`with_declared_module`](Self::with_declared_module) indexes a
    /// declaration's: an export taking or returning a structure or an
    /// enumeration is described, and reachable from a behavior tree. The
    /// build fails, naming the module, when
    /// - the record does not describe the header — each export with the same
    ///   id and name, the same parameters in the same order (ids, names and
    ///   mutability), each type the header's frozen (a primitive id as that
    ///   primitive, a `Key` as the string, any other id as a reference to it),
    ///   and no other export — naming the export;
    /// - the registry lacks a record the module record depends on,
    ///   transitively, naming each: a device holds the whole description of
    ///   what a module it describes takes and returns.
    ///
    /// A guest module without a record is indexed from its header, as
    /// [`with_module`](Self::with_module) says. A module loaded or reloaded
    /// on the built device ([`Arora::load_module`], [`Arora::reload_module`])
    /// is indexed from its header. Default: an empty registry.
    ///
    /// Given again, `registry` replaces the one given before. The records of
    /// the device's module directories (`with_module_dirs`) join it at build,
    /// whichever of the two is given first.
    ///
    /// ```ignore
    /// let mut records = arora::LocalRegistry::new();
    /// arora_registry::local_yaml::load_records_from_yaml_dir(dir.join("records"), &mut records).await?;
    /// let device = Arora::builder()
    ///     .with_module(header, wasm)
    ///     .with_registry(records)
    ///     .build()?;
    /// ```
    pub fn with_registry(mut self, registry: LocalRegistry) -> Self {
        self.registry = registry;
        self
    }

    /// Load the modules a device carries as module directories
    /// ([`module_dir`]): every one under `device_dir`'s `modules/`, in name
    /// order, then each of `dirs`. A device runner passes its device's
    /// directory ([`Device::dir`]) and its `--module` directories.
    ///
    /// Each module loads at [`build`](Self::build) as
    /// [`with_module`](Self::with_module) loads one, and at build the records
    /// its directory carries in `records/` join the device's registry
    /// ([`with_registry`](Self::with_registry), given before or after): a
    /// module whose record they hold has every export described, record-typed
    /// ones included.
    ///
    /// What the directories hold is read here: this fails, naming the module,
    /// when a directory cannot be read as a module, when one module is in two
    /// directories, or when a directory's records do not read or differ from
    /// a record another directory carries under the same id and version. A
    /// device directory without `modules/` carries no module. What the engine
    /// and the registry decide fails [`build`](Self::build): an artifact the
    /// engine rejects (a native module where the platform loads none, a wasm
    /// it does not compile), a record that does not describe its module's
    /// header, or one that differs from a record of the registry given with
    /// `with_registry`, naming the record.
    ///
    /// ```no_run
    /// # async fn runner(module_dirs: &[std::path::PathBuf]) -> anyhow::Result<()> {
    /// let device = arora::Device::from_env()?;
    /// arora::Arora::builder()
    ///     .with_hal(Box::new(arora_hal::FakeHal::new()))
    ///     .with_module_dirs(device.dir(), module_dirs)
    ///     .await?
    ///     .with_device(device)
    ///     .run()
    ///     .await
    /// # }
    /// ```
    #[cfg(feature = "native")]
    pub async fn with_module_dirs(
        mut self,
        device_dir: &std::path::Path,
        dirs: &[std::path::PathBuf],
    ) -> Result<Self> {
        let modules = module_dir::read_device_modules(device_dir, dirs)?;
        module_dir::load_records(&modules, &mut self.module_dir_records).await?;
        for module in modules {
            self = self.with_module(module.header, module.executable);
        }
        Ok(self)
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
    /// function ids it was built with. Loaded at [`build`](Self::build),
    /// which fails, naming both, when another module has its id: a guest
    /// module, another host module, the HAL module
    /// ([`arora_hal::hal_module::ID`]) or the interpreter module
    /// ([`interpreter_module::ID`]).
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
    /// method index it assembled — the core nodes and every loaded
    /// module's exports — and loads it as [`Arora::load_groot`] does, failing
    /// the build if the tree does not parse, names an unknown function, or
    /// does not load. What a device run through [`run`](AroraBuilder::run)
    /// uses, since `run` builds the device itself. The tree loads into the
    /// device's interpreter, which ticks its leaves against its own function
    /// index: the default interpreter's is the device's.
    pub fn with_groot(mut self, xml: impl Into<String>) -> Self {
        self.groot = Some(xml.into());
        self
    }

    /// What every guest module may spend, unless
    /// [`with_module_limits`](Self::with_module_limits) sets its own: the time
    /// of a call outside a step, and its instance's memory
    /// ([`arora_engine::limits`]). Default [`GuestLimits::default`]: one
    /// second, 256 MiB. A step's guest calls end by the step's period whatever
    /// the budget.
    pub fn with_guest_limits(mut self, limits: GuestLimits) -> Self {
        self.guest_limits = limits;
        self
    }

    /// What the guest module `module` may spend, in place of
    /// [`with_guest_limits`](Self::with_guest_limits)'.
    pub fn with_module_limits(mut self, module: Uuid, limits: GuestLimits) -> Self {
        self.module_limits.push((module, limits));
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

    /// The device the run is, as a runner's options give it
    /// ([`RunnerArgs::device`]): [`run`](AroraBuilder::run) serves the open
    /// local bridge on its port, and connects to Semio Studio as it, when it
    /// fills the default bridge. Without it, the local bridge serves on
    /// [`device::DEFAULT_BRIDGE_PORT`], and a run that connects is the device
    /// the environment configures ([`Device::from_env`]).
    #[cfg(feature = "native")]
    pub fn with_device(mut self, device: Device) -> Self {
        self.device = Some(device);
        self
    }

    /// What the run does about Semio Studio when [`run`](AroraBuilder::run)
    /// fills the default bridge: a runner built with Studio passes what its
    /// command line says ([`StudioArgs::choice`](studio::StudioArgs::choice)),
    /// Studio unless `--no-studio`. Without it, the run serves the open local
    /// bridge, as a runner built without Studio does in a build where another
    /// crate enables `studio-bridge`. See
    /// [whether a run connects](studio#whether-a-run-connects).
    #[cfg(feature = "studio-bridge")]
    pub fn with_studio(mut self, choice: studio::Choice) -> Self {
        self.studio = Some(choice);
        self
    }

    /// Run the assembled device to completion with the standard operator flow:
    /// pick the front end (the injected one, else terminal UI or headless by
    /// terminal detection), fill the default bridge when none was injected,
    /// default any other unset seam (a private `SimpleDataStore`, the fake
    /// HAL, an empty interpreter), and drive the step loop.
    ///
    /// The default bridge is the open local bridge, on the port of the device
    /// [`with_device`](Self::with_device) gives, unless the `studio-bridge`
    /// feature is on and the runner hands Studio to `with_studio`, as
    /// `arora::studio` says. A run that connects enrolls a device that is not
    /// enrolled with Studio: under the terminal UI, the operator names it and
    /// is shown the code a person approves it with in Studio.
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
                // The device reports to Studio the model family and versions
                // its HAL describes.
                let description = match &self.hal {
                    Some(hal) => hal.describe().await,
                    None => arora_hal::HalDescription::default(),
                };
                let bridge = studio::default_bridge(
                    &frontend,
                    &description,
                    self.studio.unwrap_or(studio::Choice::Local),
                    self.device.take(),
                )
                .await?;
                self = self.with_bridge(bridge);
            }
            #[cfg(not(feature = "studio-bridge"))]
            {
                let bridge = run::local_ws_bridge_on(self.device.as_ref()).await?;
                self = self.with_bridge(bridge);
            }
        }
        run::run_builder_with_frontend(self, frontend).await
    }

    /// Wire the engine, apply the defaults for any unset seam, take each
    /// endpoint's inbound stream and merge them, subscribe to the HAL and store
    /// feeds, and return the finished [`Arora`].
    ///
    /// It states the runtime's own keys — the `arora` subtree of
    /// [`arora_behavior::built_in`], the clock and the task runs' keys — closed
    /// to remote writers, so no wider prefix the host opened reaches them; a
    /// host opens one of them by stating that key's own meta, before the build
    /// or after it.
    ///
    /// Fails, naming what it refuses, when the device's parts do not fit
    /// together — a guest module that does not load or that its record does
    /// not describe, two modules (guest or host) under one id, a method both a
    /// module and the behavior interpreter implement, a Groot tree that does
    /// not load — or when the engine's executor host cannot be created. Fully
    /// synchronous: there is nothing to spawn — the HAL and bridges own any
    /// async internally, behind their stream/push seams.
    pub fn build(self) -> Result<Arora> {
        let mut engine = build_engine(self.guest_limits)?;
        for (module, limits) in &self.module_limits {
            engine.set_guest_limits(*module, *limits);
        }

        // The device's method index — what `DescribeMethods` serves. Both the
        // loaded guest modules and the host modules fold their functions in.
        let mut functions = self.functions;

        // The records the device's module directories carry join the
        // registry the embedder gave, whichever was given first.
        #[cfg(feature = "native")]
        let registry = module_dir::merge_records(self.registry, &self.module_dir_records)?;
        #[cfg(not(feature = "native"))]
        let registry = self.registry;

        // Load each guest module into the engine so its exported functions
        // dispatch through the engine's `CallBridge`. Done before the store and
        // seams are wired: a module that fails to load fails the whole build.
        // Its exports also join the method index, so `DescribeMethods` lists
        // them: every one from the module's record when the registry holds
        // it, else those whose parameters and return are all primitives or
        // optionals over a scalar primitive — a header carries no type
        // versions, so a signature naming a record, a map or a fixed-length
        // array dispatches but stays undiscoverable.
        //
        // One module id, one module. The engine answers a guest under an
        // already-loaded id with Ok, and registering a host module under a
        // taken id replaces the module there: either way the device would
        // dispatch one module and describe another's functions. Every source
        // of modules converges in this build — an embedder's `with_module` and
        // `with_host_module`, the device directory, `--module`, the HAL
        // module and the interpreter module — so each claims its id here, and
        // a second claim fails the build, naming both modules.
        let mut module_ids: HashMap<Uuid, String> = HashMap::new();
        claim_module_id(
            &mut module_ids,
            interpreter_module::ID,
            "the interpreter module".to_string(),
        )?;
        let mut guest_exports: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
        for module in self.modules {
            let header = module.header();
            let module_id = header.id;
            let module_name = header.name.clone();
            claim_module_id(
                &mut module_ids,
                module_id,
                format!("guest module '{module_name}'"),
            )?;
            // An export its declaration already indexed
            // (`with_declared_module`) keeps that frozen signature.
            let frozen = index_exports(header, &registry, |id| functions.contains_key(id))?;
            functions.extend(frozen.into_iter().map(|f| (f.function_id, f)));
            guest_exports.insert(module_id, export_ids(header));
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
        let host_modules = std::iter::once((
            "the HAL module".to_string(),
            hal_module::module(hal.clone()),
        ))
        .chain(
            self.host_modules
                .into_iter()
                .map(|module| (host_module_name(&module), module)),
        );
        for (name, module) in host_modules {
            claim_module_id(&mut module_ids, module.id(), name)?;
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
        // The interpreter module's task-run functions join the index too, so a
        // remote finds them by name.
        let own = interpreter_signatures::all();
        let reserved = [
            interpreter_module::LOAD,
            interpreter_module::EDIT,
            interpreter_module::SPAWN,
            interpreter_module::SPAWN_GRAPH,
            interpreter_module::HALT,
        ];
        for (function_id, name, function) in &own {
            if let Some(described) = functions.get(function_id) {
                anyhow::bail!(
                    "function {function_id} ('{}') of module {} has the id of the interpreter \
                     module's '{name}'",
                    described.function_name,
                    described.module_id
                );
            }
            functions.insert(
                *function_id,
                ModuleFunction {
                    module_id: interpreter_module::ID,
                    function_id: *function_id,
                    function_name: name.to_string(),
                    function: function.clone(),
                },
            );
        }
        for (function_id, export) in &interpreter_methods {
            if reserved.contains(function_id) {
                anyhow::bail!(
                    "the behavior interpreter describes '{}' under the id of one of the \
                     interpreter module's own functions ({function_id})",
                    export.name
                );
            }
            if let Some(described) = functions.get(function_id) {
                anyhow::bail!(
                    "function {function_id} ('{}') is described by module {} and by the behavior \
                     interpreter",
                    export.name,
                    described.module_id
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
        let hal_keys = hal.describe_keys().fuse();

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
        // Default executor: an empty, ready behavior-tree interpreter, which
        // each tick hands the device's function index. It is injected once
        // here (never swapped); a behavior is loaded into it as a separate
        // step. With none loaded it idles, so an un-configured device ticks a
        // no-op.
        let interpreter = self
            .interpreter
            .unwrap_or_else(|| Box::new(BehaviorTreeInterpreter::new()));

        // The interpreter as a module: a host module under
        // `interpreter_module::ID` whose `LOAD`/`EDIT`/`SPAWN`/`SPAWN_GRAPH`/
        // `HALT` functions
        // run on the same cell the step loop ticks. A Call to those ids — from a
        // remote or from a behavior — reaches the interpreter through the
        // engine's normal dispatch, like any module function.
        //
        // Each one that succeeds says so in the log, once per call: what the
        // device now runs is what an operator watching it asks first.
        let interpreter: runtime::InterpreterCell = Rc::new(RefCell::new(Some(interpreter)));
        let task_runs: runtime::TaskRuns = Rc::default();
        let loading_groot: Rc<Cell<bool>> = Rc::default();
        let module = ModuleBuilder::new(interpreter_module::ID)
            .function(interpreter_module::LOAD, {
                let cell = interpreter.clone();
                let loading_groot = loading_groot.clone();
                move |call| {
                    let graph = interpreter_module::decode_load(&call)
                        .map_err(|message| CallError::Guest { message })?;
                    let nodes = graph.nodes.len();
                    let result =
                        runtime::with_interpreter(&cell, |interpreter| interpreter.load(graph))?;
                    let source = if loading_groot.get() {
                        " from a Groot tree"
                    } else {
                        ""
                    };
                    log::info!(
                        "behavior loaded{source}: {nodes} node(s), running from the next step"
                    );
                    Ok(result)
                }
            })
            .function(interpreter_module::EDIT, {
                let cell = interpreter.clone();
                move |call| {
                    let diff = interpreter_module::decode_edit(&call)
                        .map_err(|message| CallError::Guest { message })?;
                    let summary = format!(
                        "+{} -{} node(s), +{} -{} link(s)",
                        diff.add_nodes.len(),
                        diff.remove_nodes.len(),
                        diff.add_links.len(),
                        diff.remove_links.len()
                    );
                    let result =
                        runtime::with_interpreter(&cell, |interpreter| interpreter.apply(diff))?;
                    log::info!("behavior edited: {summary}, running from the next step");
                    Ok(result)
                }
            })
            .function(interpreter_module::SPAWN, {
                let cell = interpreter.clone();
                let task_runs = task_runs.clone();
                move |call| {
                    let (spawned, policy) = interpreter_module::decode_spawn(&call)
                        .map_err(|message| CallError::Guest { message })?;
                    let calls = runtime::RunCalls::Call {
                        module: spawned.module_id,
                        function: spawned.id,
                    };
                    let mut handle = None;
                    let result = runtime::with_interpreter_value(&cell, |interpreter| {
                        interpreter.spawn(spawned, policy).map(|spawned| {
                            let value = interpreter_module::encode_spawn_result(&spawned);
                            handle = Some(spawned);
                            value
                        })
                    })?;
                    if let Some(handle) = handle {
                        task_runs
                            .borrow_mut()
                            .push(runtime::TaskRun::started(&handle, calls, policy));
                    }
                    Ok(result)
                }
            })
            .function(interpreter_module::SPAWN_GRAPH, {
                let cell = interpreter.clone();
                let task_runs = task_runs.clone();
                move |call| {
                    let (graph_type, graph, policy) = interpreter_module::decode_spawn_graph(&call)
                        .map_err(|message| CallError::Guest { message })?;
                    let graph_functions = graph.nodes.len();
                    let calls = runtime::RunCalls::Graph {
                        functions: graph.nodes.values().map(|node| node.function).collect(),
                    };
                    let mut handle = None;
                    let result = runtime::with_interpreter_value(&cell, |interpreter| {
                        interpreter
                            .spawn_graph(&graph_type, graph, policy)
                            .map(|spawned| {
                                let value = interpreter_module::encode_spawn_result(&spawned);
                                handle = Some(spawned);
                                value
                            })
                    })?;
                    if let Some(handle) = handle {
                        let mut runs = task_runs.borrow_mut();
                        match runs.iter_mut().find(|run| run.id == handle.id) {
                            // The graph's root is a live run's: that run now
                            // runs the graph.
                            Some(run) => {
                                log::info!(
                                    "task {} ({}) taken over: it runs the graph spawned with \
                                     its root, naming {} function(s)",
                                    handle.id.0,
                                    run.label.as_deref().unwrap_or("just started"),
                                    graph_functions
                                );
                                run.calls = calls;
                            }
                            None => runs.push(runtime::TaskRun::started(&handle, calls, policy)),
                        }
                    }
                    Ok(result)
                }
            })
            .function(interpreter_module::HALT, {
                let cell = interpreter.clone();
                let task_runs = task_runs.clone();
                move |call| {
                    let task = interpreter_module::decode_halt(&call)
                        .map_err(|message| CallError::Guest { message })?;
                    let result =
                        runtime::with_interpreter(&cell, |interpreter| interpreter.halt(task))?;
                    let mut runs = task_runs.borrow_mut();
                    match runs.iter_mut().find(|run| run.id == task) {
                        Some(run) => {
                            run.halted = true;
                            log::info!(
                                "task {} ({}) halting: it stops at the next step",
                                task.0,
                                run.label.as_deref().unwrap_or("just started")
                            );
                        }
                        // Halting a finished or unknown run is a no-op.
                        None => log::info!("task {} halt requested: no such running task", task.0),
                    }
                    Ok(result)
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
            function_index: functions,
            guest_modules: guest_exports,
            interpreter,
            hal,
            bridges,
            hal_feed,
            hal_keys,
            hal_stated: HashMap::new(),
            inbound,
            pending: Pending::default(),
            data_requested: vec![false; endpoints],
            caller_tx,
            store_changes,
            clock: Clock::starting_at(self.start_time),
            behavior_error: watch::Sender::new(None),
            task_runs,
            loading_groot,
        };
        if let Some(xml) = self.groot {
            arora.load_groot(&xml)?;
        }
        // The runtime's own keys — its clock, its task runs' — are closed to
        // remote writers whatever wider prefix the host opens: a remote that
        // set them would rewrite the device's time and its runs' state. The
        // statement is on their subtree, which a prefix names without the
        // trailing `/`, so a key's own meta still wins; and it goes through
        // the device's store, so over a `NamespacedStore` it covers that
        // device's subtree alone.
        let built_in = arora_behavior::built_in::PREFIX.trim_end_matches('/');
        if let Err(e) = arora
            .store
            .set_prefix_meta(HashMap::from([(built_in.to_string(), KeyMeta::new())]))
        {
            log::warn!("the store does not keep the runtime's keys closed: {e}");
        }
        // What the HAL says of its keys, stated to the store once nothing can
        // fail the build, and before anything can write: a device's inputs are
        // open, and listed, from its first step.
        runtime::describe_hal_keys(&*arora.store, &mut arora.hal_keys, &mut arora.hal_stated);
        Ok(arora)
    }
}

/// Claim `id` for the module `name` names, among the ids `claimed` holds:
/// refused, naming both modules, when another module claimed it first.
fn claim_module_id(claimed: &mut HashMap<Uuid, String>, id: Uuid, name: String) -> Result<()> {
    if let Some(first) = claimed.get(&id) {
        anyhow::bail!("{first} and {name} both have id {id}: a device loads one module per id");
    }
    claimed.insert(id, name);
    Ok(())
}

/// How a build error names a host module, which carries no name of its own:
/// by the functions it describes, as a remote finds them.
fn host_module_name(module: &HostModule) -> String {
    let functions: Vec<String> = module
        .descriptions()
        .iter()
        .map(|description| format!("'{}'", description.name))
        .collect();
    if functions.is_empty() {
        "a host module describing no function".to_string()
    } else {
        format!("the host module describing {}", functions.join(", "))
    }
}

/// The ids of the functions `header` exports.
fn export_ids(header: &Header) -> Vec<Uuid> {
    header.exports.iter().map(|export| *export.id()).collect()
}

/// The method-index entries of a guest module's exports: from the module's
/// record when `registry` holds it at the header's id and version — checked
/// against the header, with every record it depends on — else from the
/// header ([`freeze_exports`]). An export `described` says is already
/// indexed is skipped.
fn index_exports(
    header: &Header,
    registry: &LocalRegistry,
    described: impl Fn(&Uuid) -> bool,
) -> Result<Vec<ModuleFunction>> {
    let reference = FrozenReference {
        id: header.id,
        version: header.version.clone().into(),
    };
    let Some(record) = registry.module_at(&reference.id, &reference.version.0) else {
        if registry.find_frozen_by_id(&reference.id).is_some() {
            log::warn!(
                "the device holds a record of module '{}' ({}) at another version than its \
                 header's ({}): it is described from its header",
                header.name,
                reference.id,
                reference.version
            );
        }
        return Ok(freeze_exports(header, described).collect());
    };
    let module = format!("module '{}' ({reference})", header.name);
    module_discovery::check_record(header, record)
        .map_err(|why| anyhow::anyhow!("{module}: its record does not match its header: {why}"))?;
    let missing = registry.missing_dependencies(&reference);
    if !missing.is_empty() {
        let missing: Vec<String> = missing.iter().map(ToString::to_string).collect();
        anyhow::bail!(
            "{module}: its record depends on records the device does not hold: {}",
            missing.join(", ")
        );
    }
    log::info!("{module}: its exports are described by its record");
    Ok(header
        .exports
        .iter()
        .filter(|export| !described(export.id()))
        .map(|export| {
            let ExportKind::Function(function) = &record.exports[export.id()].kind;
            ModuleFunction {
                module_id: header.id,
                function_id: *export.id(),
                function_name: export.name().clone(),
                function: function.clone(),
            }
        })
        .collect())
}

/// The method-index entries a guest module's header gives its exports: each
/// export whose parameters and return are all primitives or optionals over a
/// scalar primitive, frozen from them. A header carries no type versions, so
/// an export naming a record (which needs a registry to pin its version), a
/// map or a fixed-length array dispatches but is not described; each one is
/// logged. An export `described` says is already indexed is skipped.
fn freeze_exports<'a>(
    header: &'a Header,
    described: impl Fn(&Uuid) -> bool + 'a,
) -> impl Iterator<Item = ModuleFunction> + 'a {
    let module_id = header.id;
    header.exports.iter().filter_map(move |export| {
        let low::ExportSymbol::Function(function) = export;
        if described(&function.id) {
            return None;
        }
        match module_discovery::guest_function_signature(function) {
            Some(signature) => Some(ModuleFunction {
                module_id,
                function_id: function.id,
                function_name: function.name.clone(),
                function: signature,
            }),
            None => {
                log::warn!(
                    "guest module {module_id} export '{}' ({}) has a parameter or return type \
                     that is neither a primitive nor an optional over a scalar primitive; it \
                     dispatches but DescribeMethods does not list it: the module's record, \
                     in the device's registry, would describe it",
                    function.name,
                    function.id
                );
                None
            }
        }
    })
}

/// Build the engine with the right executor hosts for the target: components
/// on the browser's `WebAssembly` runtime on wasm, or on wasmtime, with the
/// native (dynamic library) host, otherwise.
#[cfg(feature = "native")]
fn build_engine(limits: GuestLimits) -> Result<PinnedEngine> {
    Ok(EngineBuilder::new()
        .with_guest_limits(limits)
        .add_executor(
            ComponentExecutor::new()
                .map_err(|e| anyhow::anyhow!("failed to create the component executor: {e}"))?,
        )
        .add_executor(NativeExecutor::new())
        .build())
}

#[cfg(all(not(feature = "native"), target_arch = "wasm32"))]
fn build_engine(limits: GuestLimits) -> Result<PinnedEngine> {
    use arora_engine::executor::browser::{BrowserComponentExecutor, SharedLoader};
    Ok(EngineBuilder::new()
        .with_guest_limits(limits)
        .add_executor(BrowserComponentExecutor::new(SharedLoader::new()))
        .build())
}

/// Without the `native` feature on a native target, the engine carries no
/// executor at all: host modules enter dispatch directly
/// (`Engine::register_module`), so a device that loads no guest executables
/// needs no wasm runtime. This is the slice that builds on every target —
/// including the 32-bit Android ABIs, where wasmtime has no backend. Loading
/// a guest module on it fails at load, loudly.
#[cfg(all(not(feature = "native"), not(target_arch = "wasm32")))]
fn build_engine(limits: GuestLimits) -> Result<PinnedEngine> {
    Ok(EngineBuilder::new().with_guest_limits(limits).build())
}

/// Loading a guest wasm module through the builder and dispatching it. Needs
/// the `native` feature: the module runs on the component (wasmtime)
/// executor. `test-rust-wasm` is a small guest built as a `wasm32-wasip2`
/// cdylib artifact dependency, a component; Cargo hands its `.wasm` bytes to
/// the test, and its header comes from its declaration.
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
            name: "wasm-component".to_string(),
            min_version: None,
            max_version: None,
        })
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
                    name: "wasm-component".to_string(),
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
                .cloned()
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

    /// A guest's optional parameter bound to a store key that is not set
    /// receives `None` — rather than the call failing on `()` — and the
    /// key's value once it is written; an optional return maps onto its
    /// key's presence the same way.
    #[test]
    fn an_unset_key_binds_none_to_a_guest_optional_parameter() {
        use arora_types::data::{Key, StateChange};
        use test_rust_wasm::test_rust_wasm::Module;

        let store = SimpleDataStore::new();
        store
            .write(StateChange::from(vec![
                ("start", Value::U64(10)),
                // A stale length: window(10, None) unsets it, a failed call
                // would leave it.
                ("length", Value::U64(99)),
            ]))
            .expect("seed the start and a stale length");
        let mut arora = Arora::builder()
            .with_data_store(Box::new(store.clone()))
            .with_declared_module::<Module>(
                arora_types::module::low::Executor {
                    name: "wasm-component".to_string(),
                    min_version: None,
                    max_version: None,
                },
                WASM.to_vec(),
            )
            .with_groot(
                r#"<root main_tree_to_execute="MainTree"><BehaviorTree ID="MainTree">
                     <window start_ns="{start}" end_ns="{end}" res="{length}"/>
                   </BehaviorTree></root>"#,
            )
            .build()
            .expect("window resolves against the declared module");
        let length = || store.read(&[Key::from("length")]).remove(0);

        arora
            .step(std::time::Duration::from_millis(10))
            .expect("window(10, None) runs");
        assert_eq!(length(), None, "no end: window ran and returned None");

        store
            .write(StateChange::set("end", Value::U64(40)))
            .expect("write the end");
        arora
            .step(std::time::Duration::from_millis(10))
            .expect("window(10, Some(40)) runs");
        assert_eq!(length(), Some(Value::U64(30)));
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
                    name: "wasm-component".to_string(),
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
                    name: "wasm-component".to_string(),
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
    /// `described_host_functions_join_the_method_index`. Without the module's
    /// record, those are the exports taking and returning primitives and
    /// optionals over a scalar primitive; `reading`, which returns a
    /// structure, is not among them (`module_records_tests`).
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
                .cloned()
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

    /// A guest export frozen from its header alone is described with the
    /// signature its module's record declares for it: what a host module
    /// registering the same declaration would describe. An export naming a
    /// record does not freeze from the header.
    #[test]
    fn a_guest_export_is_described_as_its_module_record_declares_it() {
        use arora_types::record::module::frozen::ExportKind;

        let header = test_module_header();
        let record = test_rust_wasm::test_rust_wasm::record(Uuid::nil());
        assert_eq!(header.exports.len(), record.exports.len());
        for export in &header.exports {
            let low::ExportSymbol::Function(function) = export;
            let ExportKind::Function(declared) = &record.exports[&function.id].kind;
            let frozen = module_discovery::guest_function_signature(function);
            if ["reading", "readings"].contains(&function.name.as_str()) {
                assert_eq!(frozen, None, "{} names a record", function.name);
            } else {
                assert_eq!(frozen.as_ref(), Some(declared), "{}", function.name);
            }
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
                .contains("guest module 'test-rust-wasm' and guest module 'test-rust-wasm-again' both have id"),
            "{error}"
        );
    }

    /// A guest module and a host module with one id fail the build, naming
    /// both: the host module would take the guest's place in dispatch while
    /// the method index still describes the guest's exports, a call to any of
    /// them failing with `FunctionNotFound`.
    #[test]
    fn a_guest_and_a_host_module_with_one_id_fail_the_build() {
        let header = test_module_header();
        let id = header.id;
        let error = Arora::builder()
            .with_module(header, WASM.to_vec())
            .with_host_module(ModuleBuilder::new(id).build())
            .build()
            .err()
            .expect("one id, a guest and a host module is refused");
        assert_eq!(
            error.to_string(),
            format!(
                "guest module 'test-rust-wasm' and a host module describing no function both \
                 have id {id}: a device loads one module per id"
            )
        );
    }

    /// A guest module under the HAL module's id fails the build: every device
    /// registers the HAL module, which would take the guest's place.
    #[test]
    fn a_guest_module_under_the_hal_module_s_id_fails_the_build() {
        let mut header = test_module_header();
        header.id = arora_hal::hal_module::ID;
        let error = Arora::builder()
            .with_module(header, WASM.to_vec())
            .build()
            .err()
            .expect("a guest under the HAL module's id is refused");
        assert_eq!(
            error.to_string(),
            format!(
                "guest module 'test-rust-wasm' and the HAL module both have id {}: a device \
                 loads one module per id",
                arora_hal::hal_module::ID
            )
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
                .contains("guest module 'test-rust-wasm' and guest module 'test-rust-wasm-again' both have id"),
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
/// The core behavior-tree nodes — the control nodes and the string nodes —
/// are native: a device runs a tree of them with no module loaded.
#[cfg(test)]
mod core_node_tests {
    use super::*;
    use arora_types::data::{Key, StateChange};
    use arora_types::value::Value;
    use std::time::Duration;

    /// `IsStringSet → SetString → UnsetString` on a key written from outside
    /// the tree, installed with `with_groot` on a device with no module: the
    /// branch runs once per write of the key, and the key's absence before
    /// the first write is a failed condition, not an error.
    #[test]
    fn a_string_branch_runs_once_per_write_with_no_module_loaded() {
        let store = SimpleDataStore::new();
        let mut arora = Arora::builder()
            .with_data_store(Box::new(store.clone()))
            .with_groot(
                r#"<root main_tree_to_execute="MainTree"><BehaviorTree ID="MainTree">
                     <Sequence>
                       <IsStringSet value="{command}"/>
                       <SetString variable="{last_command}" value="{command}"/>
                       <UnsetString variable="{command}"/>
                     </Sequence>
                   </BehaviorTree></root>"#,
            )
            .build()
            .expect("the core nodes need no module");
        let read = |key: &str| store.read(&[Key::from(key)]).remove(0);
        let unset = |key: &str| StateChange {
            set: HashMap::new(),
            unset: std::collections::HashSet::from([Key::from(key)]),
        };

        arora
            .step(Duration::from_millis(10))
            .expect("no command yet");
        assert_eq!(read("last_command"), None);

        for command in ["sit", "stand"] {
            store
                .write(StateChange::set("command", Value::String(command.into())))
                .expect("write a command");
            arora
                .step(Duration::from_millis(10))
                .expect("the branch runs");
            assert_eq!(read("last_command"), Some(Value::String(command.into())));
            assert_eq!(read("command"), None, "the branch consumed the command");

            // No new write: the branch does not run again.
            store.write(unset("last_command")).expect("clear the trace");
            arora.step(Duration::from_millis(10)).expect("idle step");
            arora.step(Duration::from_millis(10)).expect("idle step");
            assert_eq!(read("last_command"), None, "once per write");
        }
    }

    /// `SetString` creates a key the store does not hold yet.
    #[test]
    fn set_string_creates_a_key() {
        let store = SimpleDataStore::new();
        let mut arora = Arora::builder()
            .with_data_store(Box::new(store.clone()))
            .with_groot(
                r#"<root main_tree_to_execute="MainTree"><BehaviorTree ID="MainTree">
                     <SetString variable="{mood}" value="happy"/>
                   </BehaviorTree></root>"#,
            )
            .build()
            .expect("the core nodes need no module");
        assert_eq!(store.read(&[Key::from("mood")]), vec![None]);
        arora.step(Duration::from_millis(10)).expect("one step");
        assert_eq!(
            store.read(&[Key::from("mood")]),
            vec![Some(Value::String("happy".into()))]
        );
    }
}

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
            .cloned()
            .expect("the described function is indexed");
        assert_eq!(entry.module_id, module_id);
        assert_eq!(entry.function_name, "look_at");
        assert_eq!(entry.function, signature);
        assert!(!arora.function_index.contains_key(&undescribed));
    }

    /// Two host modules with one id fail the build, naming both by the
    /// functions they describe: the second would replace the first in
    /// dispatch while the method index still describes the first's functions.
    #[test]
    fn two_host_modules_with_one_id_fail_the_build() {
        let id = Uuid::from_u128(0x6761);
        let module = |function: u128, name: &str| {
            ModuleBuilder::new(id)
                .described_function(Uuid::from_u128(function), name, unit_signature(), |_call| {
                    Ok(CallResult {
                        ret: Value::Unit,
                        mutated: Vec::new(),
                    })
                })
                .build()
        };
        let error = Arora::builder()
            .with_host_module(module(0x6c61, "look_at"))
            .with_host_module(module(0x6c62, "wave"))
            .build()
            .err()
            .expect("one id, two host modules is refused");
        assert_eq!(
            error.to_string(),
            format!(
                "the host module describing 'look_at' and the host module describing 'wave' \
                 both have id {id}: a device loads one module per id"
            )
        );
    }

    /// A host module under the id of a module every device registers, the
    /// HAL module or the interpreter module, fails the build, naming it.
    #[test]
    fn a_host_module_under_the_hal_or_the_interpreter_module_s_id_fails_the_build() {
        for (id, owner) in [
            (arora_hal::hal_module::ID, "the HAL module"),
            (interpreter_module::ID, "the interpreter module"),
        ] {
            let error = Arora::builder()
                .with_host_module(ModuleBuilder::new(id).build())
                .build()
                .err()
                .expect("a host module under a reserved id is refused");
            assert_eq!(
                error.to_string(),
                format!(
                    "{owner} and a host module describing no function both have id {id}: a \
                     device loads one module per id"
                )
            );
        }
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
            .cloned()
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

#[cfg(all(test, feature = "native"))]
mod test_components;

#[cfg(all(test, feature = "native"))]
mod module_reload_tests;

#[cfg(all(test, feature = "native"))]
mod module_records_tests;

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

    /// What the device answers a `ListKeys` and a `DescribeMethods` with is
    /// readable without a step: `list_keys` over a sibling handle of its
    /// store, `describe_methods` on the device.
    #[test]
    fn keys_and_methods_read_without_a_step_as_the_device_answers_them() {
        let mut arora = device();
        let caller = arora.caller();
        let store = arora.store().clone_box();
        store
            .set_meta(HashMap::from([(
                Key::from("face/brow"),
                KeyMeta::new().editable(),
            )]))
            .expect("describe a key");
        store
            .write(StateChange::set("face/mouth", Value::F64(0.5)))
            .expect("write a key");

        let read = list_keys(&*store, Some("face/"));
        assert_eq!(
            read.iter()
                .map(|(path, _)| path.as_str())
                .collect::<Vec<_>>(),
            ["face/brow", "face/mouth"],
            "a described key and a held one, sorted: {read:?}"
        );
        let answered = settle(&mut arora, caller.list_keys(Some("face/".to_string())))
            .expect("the device lists its keys");
        assert_eq!(read, answered);

        let read = arora.describe_methods(Some("stop"));
        assert_eq!(read.len(), 2, "both modules' stop: {read:?}");
        let answered = settle(
            &mut arora,
            caller.describe_methods(Some("stop".to_string())),
        )
        .expect("the device describes its methods");
        assert_eq!(read, answered);
        assert_eq!(
            arora.describe_methods(None).len(),
            arora.function_index.len(),
            "no prefix describes the whole index"
        );
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
        assert_eq!(
            arora.task_runs.borrow().len(),
            1,
            "the device follows the running task, to log its end"
        );

        settle(&mut arora, caller.halt(handle.id)).expect("the halt is applied");
        arora.step(Duration::from_millis(10)).expect("step");
        let failure: Value = Status::Failure.into();
        assert_eq!(
            arora.store().read(std::slice::from_ref(&handle.status)),
            vec![Some(failure)],
            "a halted run ends Failure"
        );
        assert!(
            arora.task_runs.borrow().is_empty(),
            "the ended run is reported once and no longer followed"
        );
    }

    /// The device follows each run it started until the run ends, marking the
    /// one it was asked to halt, so the log says once how each ended.
    #[test]
    fn the_device_follows_a_run_to_its_end() {
        let mut arora = device();
        let spawned = arora
            .call(interpreter_module::encode_spawn(
                &Call {
                    module_id: Some(TOOLS),
                    id: WAVE,
                    args: Vec::new(),
                },
                arora_behavior::RunPolicy::Concurrent,
            ))
            .expect("the run starts");
        let handle = interpreter_module::decode_spawn_result(&spawned.ret).expect("a handle");
        {
            let runs = arora.task_runs.borrow();
            assert_eq!(runs.len(), 1);
            assert_eq!(runs[0].id, handle.id);
            assert!(!runs[0].halted);
        }
        arora.step(Duration::from_millis(10)).expect("step");
        {
            let runs = arora.task_runs.borrow();
            assert_eq!(runs.len(), 1, "still running");
            let label = runs[0].label.as_deref().expect("its start is said");
            assert!(label.contains("'wave'"), "{label}");
        }

        arora
            .call(interpreter_module::encode_halt(handle.id))
            .expect("the halt is accepted");
        assert!(arora.task_runs.borrow()[0].halted, "marked halted");
        arora.step(Duration::from_millis(10)).expect("step");
        assert!(
            arora.task_runs.borrow().is_empty(),
            "the ended run is reported once and no longer followed"
        );
    }

    /// A graph that calls `double` with `x` each step, writing the return
    /// under `tools/doubled`, and never ends.
    fn doubling_graph(x: f64) -> arora_behavior::graph::Graph {
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
                LinkSource::Literal(Value::F64(x)),
            ),
            Link::new(Port::new(double, _RET_PARAM_ID), LinkSource::Variable(out)),
        ];
        graph.variables.insert(out, "tools/doubled".to_string());
        graph
    }

    /// `spawn_graph` starts a graph as a run: each step it calls a module
    /// function and writes the return under a key, until a halt ends it.
    #[test]
    fn spawn_graph_runs_a_graph_every_step_until_halted() {
        let mut arora = device();
        let caller = arora.caller();
        let graph_type = arora_behavior_tree::behavior::graph_type();
        let handle = settle(
            &mut arora,
            caller.spawn_graph(&graph_type, &doubling_graph(2.0)),
        )
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

    /// A graph spawned again with the root of a live run answers that run,
    /// which runs the graph from then on: a client that lost the handle takes
    /// the run over and halts it. The device follows the run once.
    #[test]
    fn a_graph_spawned_again_answers_the_run_that_runs_it() {
        let mut arora = device();
        let caller = arora.caller();
        let graph_type = arora_behavior_tree::behavior::graph_type();
        let first = settle(
            &mut arora,
            caller.spawn_graph(&graph_type, &doubling_graph(2.0)),
        )
        .expect("the graph starts as a run");
        arora.step(Duration::from_millis(10)).expect("step");

        let again = settle(
            &mut arora,
            caller.spawn_graph(&graph_type, &doubling_graph(3.0)),
        )
        .expect("the graph spawned again is answered");
        assert_eq!(again, first, "the answer is the live run's handle");
        assert_eq!(arora.task_runs.borrow().len(), 1, "one run is followed");
        arora.step(Duration::from_millis(10)).expect("step");
        let running: Value = Status::Running.into();
        assert_eq!(
            arora
                .store()
                .read(&[first.status.clone(), Key::new("tools/doubled")]),
            vec![Some(running), Some(Value::F64(6.0))],
            "the run runs the graph spawned again"
        );

        settle(&mut arora, caller.halt(again.id)).expect("the halt is applied");
        arora.step(Duration::from_millis(10)).expect("step");
        let failure: Value = Status::Failure.into();
        assert_eq!(
            arora.store().read(std::slice::from_ref(&first.status)),
            vec![Some(failure)],
            "the run taken over is halted"
        );
        assert!(
            arora.task_runs.borrow().is_empty(),
            "the ended run is reported once and no longer followed"
        );
    }

    /// The interpreter module's functions are described: a client finds them
    /// by name and invokes them with arguments by name.
    #[test]
    fn the_interpreter_module_s_functions_are_described_and_invoked_by_name() {
        use arora_behavior::graph::{Graph, Node};
        use arora_behavior_tree::nodes::RUN_FUNCTION_ID;

        let mut arora = device();
        let caller = arora.caller();
        let described = settle(&mut arora, caller.describe_methods(None)).expect("described");
        for name in ["spawn", "spawn_graph", "halt"] {
            let method = described
                .iter()
                .find(|method| method.name == name)
                .unwrap_or_else(|| panic!("{name} is described"));
            assert_eq!(method.module_id, interpreter_module::ID, "{name}");
        }
        assert!(
            !described
                .iter()
                .any(|method| method.module_id == interpreter_module::ID
                    && ["load", "edit"].contains(&method.name.as_str())),
            "load and edit are reached by id"
        );

        // `spawn`, its call a key-value as described.
        let call = Call {
            module_id: Some(TOOLS),
            id: WAVE,
            args: Vec::new(),
        };
        let args = HashMap::from([
            (
                "call".to_string(),
                arora_types::value_serde::to_value(&call).expect("a call converts"),
            ),
            ("policy".to_string(), Value::from("Concurrent")),
        ]);
        let Invoked::Returned(spawned) =
            settle(&mut arora, caller.invoke("spawn", args, None)).expect("spawned")
        else {
            panic!("spawn answers with a handle");
        };
        let waving = interpreter_module::decode_spawn_result(&spawned).expect("a handle");
        assert!(
            matches!(spawned, Value::KeyValue(_)),
            "the handle is a key-value"
        );
        settle(&mut arora, caller.halt(waving.id)).expect("halted");

        let run = Uuid::from_u128(0x1);
        let mut graph = Graph::empty();
        graph.root = Some(run);
        graph.nodes.insert(
            run,
            Node {
                id: run,
                function: RUN_FUNCTION_ID,
                ..Node::default()
            },
        );
        let args = HashMap::from([
            ("graph_type".to_string(), Value::from("behavior-tree")),
            ("graph_version".to_string(), Value::from("1.0")),
            (
                "graph".to_string(),
                arora_types::value_serde::to_value(&graph).expect("a graph converts"),
            ),
            ("policy".to_string(), Value::from("Concurrent")),
        ]);
        let Invoked::Returned(spawned) =
            settle(&mut arora, caller.invoke("spawn_graph", args, None)).expect("spawned")
        else {
            panic!("spawn_graph answers with a handle, not a run of its own");
        };
        let handle = interpreter_module::decode_spawn_result(&spawned).expect("a handle");
        arora.step(Duration::from_millis(10)).expect("step");
        let running: Value = Status::Running.into();
        assert_eq!(
            arora.store().read(std::slice::from_ref(&handle.status)),
            vec![Some(running)]
        );

        let halt = HashMap::from([("task".to_string(), Value::Uuid(handle.id.0))]);
        settle(&mut arora, caller.invoke("halt", halt, None)).expect("halted");
        arora.step(Duration::from_millis(10)).expect("step");
        let failure: Value = Status::Failure.into();
        assert_eq!(
            arora.store().read(std::slice::from_ref(&handle.status)),
            vec![Some(failure)]
        );
    }

    /// A module function under the id of one of the interpreter module's
    /// functions fails the build.
    #[test]
    fn a_module_function_under_an_interpreter_function_id_fails_the_build() {
        let clash = ModuleBuilder::new(Uuid::from_u128(0x7201))
            .described_function(
                interpreter_module::HALT,
                "stop_all",
                signature(&[], FrozenTy::from(PrimitiveKind::Unit)),
                |_call| answer(Value::Unit),
            )
            .build();
        let Err(error) = Arora::builder().with_host_module(clash).build() else {
            panic!("the build fails");
        };
        assert!(error.to_string().contains("'halt'"), "{error}");
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

/// A HAL's key descriptions as the device's key meta: what `ListKeys` lists,
/// and what a remote write is checked against.
#[cfg(test)]
mod hal_key_tests {
    use super::*;
    use arora_behavior::built_in;
    use arora_simple_data_store::NamespacedStore;
    use arora_types::value::{Type, Value};
    use caller_tests::settle;

    fn device_with(hal: FakeHal, store: SimpleDataStore) -> Arora {
        Arora::builder()
            .with_hal(Box::new(hal))
            .with_data_store(Box::new(store))
            .build()
            .expect("build the device")
    }

    fn fake_with(joints: &[&str]) -> FakeHal {
        let hal = FakeHal::new();
        hal.add_joints(joints.iter().copied());
        hal
    }

    /// A remote's write of `value` to `key`, through the path every bridge's
    /// `Update` takes.
    fn update(arora: &mut Arora, key: &str, value: Value) -> Result<(), CallError> {
        let caller = arora.caller();
        settle(arora, caller.update(StateChange::set(key, value)))
    }

    fn refused(arora: &mut Arora, key: &str, value: Value) -> String {
        update(arora, key, value)
            .expect_err("the write is refused")
            .to_string()
    }

    #[test]
    fn the_hal_s_inputs_are_listed_open_and_its_outputs_closed() {
        let mut arora = device_with(fake_with(&["joint1"]), SimpleDataStore::new());
        let caller = arora.caller();
        let keys = settle(&mut arora, caller.list_keys(Some("joint1".to_string())))
            .expect("the device lists its keys");
        let input = KeyMeta::new().editable().of_type(Type::F64);
        let output = KeyMeta::new().of_type(Type::F64);
        assert_eq!(
            keys,
            vec![
                ("joint1.position".to_string(), output),
                ("joint1.target_position".to_string(), input.clone()),
                ("joint1/target.position".to_string(), input),
            ]
        );
    }

    #[test]
    fn a_remote_writes_the_hal_s_inputs_and_nothing_else() {
        let mut arora = device_with(fake_with(&["joint1"]), SimpleDataStore::new());
        update(&mut arora, "joint1/target.position", Value::F64(0.5))
            .expect("an input in the entity form");
        update(&mut arora, "joint1.target_position", Value::F64(0.25))
            .expect("an input in the prefixed form");
        arora.step(Duration::from_millis(10)).expect("step");
        assert_eq!(
            arora.store().read(&[Key::from("joint1.position")]),
            vec![Some(Value::F64(0.25))],
            "the fake senses the target back"
        );

        let output = refused(&mut arora, "joint1.position", Value::F64(1.0));
        assert!(output.contains("not an input"), "{output}");
        let unlisted = refused(&mut arora, "joint2.target_position", Value::F64(1.0));
        assert!(unlisted.contains("not an input"), "{unlisted}");
        let ill_typed = refused(&mut arora, "joint1.target_position", Value::I64(1));
        assert!(ill_typed.contains("declared type"), "{ill_typed}");
    }

    #[test]
    fn keys_the_hal_describes_later_are_inputs_from_the_next_step() {
        let hal = FakeHal::new();
        let mut arora = device_with(hal.clone(), SimpleDataStore::new());
        refused(&mut arora, "joint1.target_position", Value::F64(1.0));
        hal.add_joints(["joint1"]);
        update(&mut arora, "joint1.target_position", Value::F64(1.0))
            .expect("the joint is described");
    }

    /// `arora --open` opens every key with the empty prefix: a key the HAL
    /// describes takes the HAL's meta, which is more specific, and every other
    /// key outside the runtime's own stays open.
    #[test]
    fn open_keeps_every_key_the_hal_does_not_describe_open() {
        let store = SimpleDataStore::new();
        store
            .set_prefix_meta(HashMap::from([(String::new(), KeyMeta::new().editable())]))
            .expect("open everything");
        let mut arora = device_with(fake_with(&["joint1"]), store);
        update(&mut arora, "joint2.target_position", Value::I64(1)).expect("undescribed");
        update(&mut arora, "anything", Value::String("x".into())).expect("undescribed");
        update(&mut arora, "joint1.target_position", Value::F64(1.0)).expect("an input");
        let output = refused(&mut arora, "joint1.position", Value::F64(1.0));
        assert!(output.contains("not an input"), "{output}");
        let ill_typed = refused(&mut arora, "joint1/target.position", Value::I64(1));
        assert!(ill_typed.contains("declared type"), "{ill_typed}");
    }

    /// A task run's key, as the behavior-tree interpreter names it:
    /// `arora/tasks/<module>/<function>/<run>/…`.
    const TASK_RUN_STATUS: &str = "arora/tasks/m/f/1/status";

    /// The runtime's own keys stay closed under the opened empty prefix: a
    /// remote sets neither the device's clock nor a task run's keys.
    #[test]
    fn open_leaves_the_runtime_s_keys_closed() {
        let store = SimpleDataStore::new();
        store
            .set_prefix_meta(HashMap::from([(String::new(), KeyMeta::new().editable())]))
            .expect("open everything");
        let mut arora = device_with(FakeHal::new(), store);
        for key in [built_in::TIME, built_in::DT, TASK_RUN_STATUS] {
            let closed = refused(&mut arora, key, Value::U64(0));
            assert!(closed.contains("not an input"), "{key}: {closed}");
        }
        update(&mut arora, "anything", Value::U64(1)).expect("an ordinary key");
    }

    /// A host opens one of the runtime's keys by stating that key's own meta,
    /// before the build or after it; the others stay closed.
    #[test]
    fn a_host_opens_one_of_the_runtime_s_keys_by_its_own_meta() {
        let store = SimpleDataStore::new();
        store
            .set_meta(HashMap::from([(
                Key::from(built_in::TIME),
                KeyMeta::new().editable(),
            )]))
            .expect("open the clock");
        let mut arora = device_with(FakeHal::new(), store);
        update(&mut arora, built_in::TIME, Value::U64(0)).expect("opened before the build");
        refused(&mut arora, TASK_RUN_STATUS, Value::U64(0));
        arora
            .store()
            .set_meta(HashMap::from([(
                Key::from(TASK_RUN_STATUS),
                KeyMeta::new().editable(),
            )]))
            .expect("open a task run's key");
        update(&mut arora, TASK_RUN_STATUS, Value::U64(0)).expect("opened after the build");
        refused(&mut arora, built_in::DT, Value::U64(0));
    }

    /// Over a shared store, a device closes its own runtime keys alone: its
    /// statement lands under its namespace, where a neighbour's keys and the
    /// shared store's other keys keep what the store says of them.
    #[test]
    fn a_device_over_a_shared_store_closes_its_own_runtime_keys() {
        let shared = SimpleDataStore::new();
        shared
            .set_prefix_meta(HashMap::from([(String::new(), KeyMeta::new().editable())]))
            .expect("open everything");
        let view = NamespacedStore::new(Arc::new(shared.clone()), "robotA");
        let mut arora = Arora::builder()
            .with_data_store(Box::new(view))
            .build()
            .expect("build the device");
        refused(&mut arora, built_in::TIME, Value::U64(0));
        update(&mut arora, "anything", Value::U64(1)).expect("an ordinary key");
        let open = |path: &str| {
            shared.meta(&[Key::from(path)])[0]
                .as_ref()
                .is_some_and(|meta| meta.editable)
        };
        assert!(!open("robotA/arora/time"));
        assert!(open("robotB/arora/time"), "a neighbour's");
        assert!(open("arora/time"), "the shared store's own");
    }

    /// A key's own meta the host stated before the build is the host's: the
    /// HAL's description does not replace it, and fills the HAL's other keys.
    #[test]
    fn a_host_s_own_meta_for_a_described_key_wins() {
        let store = SimpleDataStore::new();
        store
            .set_meta(HashMap::from([(
                Key::from("joint1/target.position"),
                KeyMeta::new(),
            )]))
            .expect("close one input");
        let hal = fake_with(&["joint1"]);
        let mut arora = device_with(hal.clone(), store);
        let closed = refused(&mut arora, "joint1/target.position", Value::F64(1.0));
        assert!(closed.contains("not an input"), "{closed}");
        update(&mut arora, "joint1.target_position", Value::F64(1.0))
            .expect("the other form is the HAL's");
        hal.add_joints(["joint2"]);
        update(&mut arora, "joint2.target_position", Value::F64(1.0))
            .expect("a joint described later");
    }

    /// A device that opens a subtree keeps its inputs there: the HAL's inputs
    /// under it stay open, an undescribed key under it too.
    #[test]
    fn a_prefix_opened_subtree_keeps_its_inputs() {
        let store = SimpleDataStore::new();
        store
            .set_prefix_meta(HashMap::from([(
                "arm".to_string(),
                KeyMeta::new().editable(),
            )]))
            .expect("open the arm");
        let mut arora = device_with(fake_with(&["arm/elbow"]), store);
        update(&mut arora, "arm/elbow/target.position", Value::F64(1.0)).expect("an input");
        update(&mut arora, "arm/elbow.target_position", Value::F64(1.0)).expect("an input");
        update(&mut arora, "arm/wrist.target_position", Value::F64(1.0)).expect("undescribed");
        let output = refused(&mut arora, "arm/elbow.position", Value::F64(1.0));
        assert!(output.contains("not an input"), "{output}");
    }
}
