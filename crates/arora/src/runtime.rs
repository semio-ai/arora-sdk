//! The Arora runtime loop — studio-bridge's `engine`, library side.
//!
//! # Portable, step-dispatched, single state owner
//!
//! Several things want to change the blackboard: the **bridge** (commands and
//! state from the remote), the **HAL** (sensor readings), and the **behavior
//! tree** (intent it writes while ticking). Rather than share the state behind a
//! lock and race, [`Runtime`] gives it a single owner and dispatches the others
//! as serialized **steps** of one loop ([`step`](Runtime::step)):
//!
//! 1. drain inbound bridge/HAL updates → apply to the state;
//! 2. tick the behavior tree → it reads/writes the state;
//! 3. flush the resulting state changes out to the remote / hardware.
//!
//! Only one step touches the state at a time, so there is never concurrent
//! access — and no dedicated engine thread, just a dedicated *step*.
//!
//! ## Why it is built this way (web first)
//!
//! `step()` is **synchronous and non-blocking**, and the [`Runtime`] itself
//! spawns no threads, owns no async runtime, and never sleeps. The asynchronous
//! bridge/HAL I/O lives in a separate [`io`] pump (built from `futures` only)
//! that talks to the loop through channels. The embedder drives both:
//!
//! - **native**: spawn [`io`] on Tokio and call [`run`](Runtime::run) (a thin
//!   `step` loop) on a thread;
//! - **web**: `spawn_local` the [`io`] pump and drive `step()` from
//!   `requestAnimationFrame` — or run the whole thing inside a Web Worker.
//!
//! Because the loop only ever exchanges messages over channels, it moves into a
//! Web Worker unchanged: the channels become the worker boundary.

use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use arora_behavior::{Behavior, BehaviorContext, BehaviorStatus};
use arora_behavior_tree::{
    behavior::TreeBehavior, schema_groot, tree_node::TreeNode, BehaviorTree, ModuleFunction,
};
use arora_bridge::{Bridge, BridgeCommand, BridgeOp, BridgeResult, DeviceInfo};
use arora_engine::engine::PinnedEngine;
use arora_hal::Hal;
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::CallResult;
use arora_types::data::{DataStore, Key, StateChange, Subscription};
use arora_types::value::Value;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures::{Future, StreamExt};
use uuid::Uuid;

use crate::Arora;

/// Inbound events the async [`io`] pump forwards to the sync [`Runtime`].
enum Inbound {
    /// A command from the remote, with its reply channel.
    Command(BridgeCommand),
    /// A device-info update (`Ok(None)` = the device was unregistered).
    DeviceInfo(BridgeResult<Option<DeviceInfo>>),
    /// A client claimed/released interest in the data.
    DataRequested(bool),
}

/// Outbound work the [`Runtime`] hands to the async [`io`] pump.
enum Outbound {
    /// A local state change to mirror to the remote and the hardware.
    StateChanged(StateChange),
}

/// What a [`step`](Runtime::step) concluded.
#[derive(Debug, PartialEq, Eq)]
pub enum StepOutcome {
    /// The runtime is live; keep stepping.
    Live,
    /// The device was unregistered from the remote; stop stepping.
    Unregistered,
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

/// The Arora runtime: the state's single owner plus the engine and behavior
/// trees, advanced one [`step`](Runtime::step) at a time. Build it with
/// [`with_io`](Runtime::with_io).
pub struct Runtime {
    // Owned and touched ONLY by the stepping thread (single-threaded state).
    // Held behind `dyn DataStore` so a wrapping store (e.g. a
    // `NamespacedStore` over one mutualized backend) can be injected via
    // [`with_io_in`](Runtime::with_io_in).
    store: Arc<dyn DataStore>,
    engine: PinnedEngine,
    function_index: Rc<HashMap<Uuid, ModuleFunction>>,
    behaviors: VecDeque<Box<dyn Behavior>>,
    // Channels to/from the async io pump, plus the HAL's sensor feed:
    hal_updates: Subscription,
    inbound: UnboundedReceiver<Inbound>,
    outbound: UnboundedSender<Outbound>,
    store_changes: Subscription,
}

impl Runtime {
    /// Wire an [`Arora`] (engine + behavior-tree module) to a HAL and a bridge,
    /// with a fresh, private [`SimpleDataStore`].
    ///
    /// Returns the synchronous `Runtime` plus the asynchronous [`io`] future that
    /// pumps the bridge/HAL. The embedder spawns the future on its executor
    /// (`tokio::spawn` on native, `spawn_local` on the web) and drives
    /// [`step`](Runtime::step) on its own cadence — the two communicate only over
    /// channels.
    ///
    /// To share one store across several runtimes (and keep a handle to it for
    /// direct access), use [`with_io_in`](Runtime::with_io_in).
    pub fn with_io(
        arora: Arora,
        hal: Arc<dyn Hal>,
        bridge: Arc<dyn Bridge>,
    ) -> (Self, impl Future<Output = ()>) {
        Self::with_io_in(arora, hal, bridge, Arc::new(SimpleDataStore::new()))
    }

    /// Like [`with_io`](Runtime::with_io), but runs against the caller-provided
    /// `store` (any [`DataStore`]) rather than a private one.
    ///
    /// The store is a trait object, so the caller chooses the backend: a plain
    /// shared [`SimpleDataStore`] (cheaply cloneable, clones share storage), or a
    /// wrapping store such as a `NamespacedStore` that prefixes every key with a
    /// device namespace before delegating to one mutualized backend. This is how
    /// Studio mutualizes one store across every spawned device (namespaced by
    /// device key) while keeping its own handle for direct access and
    /// subscription.
    pub fn with_io_in(
        arora: Arora,
        hal: Arc<dyn Hal>,
        bridge: Arc<dyn Bridge>,
        store: Arc<dyn DataStore>,
    ) -> (Self, impl Future<Output = ()>) {
        let Arora {
            engine,
            function_index,
        } = arora;
        let store_changes = store.subscribe();
        let hal_updates = hal.updates();

        let (inbound_tx, inbound) = futures::channel::mpsc::unbounded::<Inbound>();
        let (outbound, outbound_rx) = futures::channel::mpsc::unbounded::<Outbound>();

        let pump = io(bridge, hal, inbound_tx, outbound_rx);

        let runtime = Self {
            store,
            engine,
            function_index,
            behaviors: VecDeque::new(),
            hal_updates,
            inbound,
            outbound,
            store_changes,
        };
        (runtime, pump)
    }

    /// Queue a behavior tree (as Groot XML) to be run on the next BT step.
    ///
    /// Each Groot `{var}` is bound to the data store under its own name — the
    /// Direct convention (variable name == store key) — so a behavior reading or
    /// writing `{var}` reads/writes the store directly during the tick. STEP 2's
    /// single-writer guarantee makes that race-free (no copy/sync needed).
    pub fn queue_groot_xml(&mut self, xml: &str) -> Result<(), RuntimeError> {
        let groot = schema_groot::BehaviorTree::try_from_groot_xml(xml)
            .map_err(|e| RuntimeError::BehaviorTree(format!("parse: {e:?}")))?;
        // `try_into_tree_node` fills `variables` as name → variable id.
        let mut variables = HashMap::new();
        let tree_node: TreeNode = groot
            .root
            .try_into_tree_node(self.function_index.as_ref(), &mut variables)
            .map_err(|e| RuntimeError::BehaviorTree(format!("build: {e:?}")))?;
        // Invert to variable id → name for the BT builder.
        let id_to_name: HashMap<Uuid, String> =
            variables.into_iter().map(|(name, id)| (id, name)).collect();
        // Direct convention: a variable resolves to the store slot under its name.
        let store = self.store.clone();
        let resolver = move |name: &str| Some(store.slot(&Key::from(name)));
        let behavior: BehaviorTree = tree_node
            .into_behavior_tree(&resolver, &id_to_name)
            .map_err(|e| RuntimeError::BehaviorTree(format!("instantiate: {e:?}")))?;
        self.queue_behavior(Box::new(TreeBehavior::new(
            behavior,
            self.function_index.clone(),
        )));
        Ok(())
    }

    /// Queue any [`Behavior`] — a behavior tree, a node graph, or another
    /// interpreter — to be ticked on the next step. The runtime ticks it each
    /// step while it reports [`BehaviorStatus::Running`] and drops it once it
    /// reports [`BehaviorStatus::Done`].
    pub fn queue_behavior(&mut self, behavior: Box<dyn Behavior>) {
        self.behaviors.push_back(behavior);
    }

    /// Advance one step: drain inbound bridge/HAL updates into the state, tick the
    /// behavior tree, then flush the resulting state changes out. Non-blocking;
    /// touches the state from this (single) thread only.
    pub fn step(&mut self) -> Result<StepOutcome, RuntimeError> {
        // STEP 1a — HAL sensor updates (a synchronous subscription).
        while let Some(change) = self.hal_updates.try_recv() {
            self.apply(change)?;
        }
        // STEP 1b — bridge events forwarded by the io pump.
        while let Ok(event) = self.inbound.try_recv() {
            match event {
                Inbound::Command(cmd) => self.handle_command(cmd)?,
                Inbound::DeviceInfo(Ok(None)) => return Ok(StepOutcome::Unregistered),
                Inbound::DeviceInfo(Ok(Some(_info))) => { /* TODO: apply device info */ }
                Inbound::DeviceInfo(Err(_e)) => { /* TODO: surface bridge error */ }
                Inbound::DataRequested(_requested) => { /* TODO: claim handling */ }
            }
        }
        // STEP 2 — tick the active behavior (tree, node graph, …) against the
        // shared store; keep it queued while it is still running.
        if let Some(mut behavior) = self.behaviors.pop_front() {
            let mut ctx = BehaviorContext {
                store: &*self.store,
                caller: &mut self.engine,
                dt: 0.0,
            };
            let status = behavior
                .tick(&mut ctx)
                .map_err(|e| RuntimeError::BehaviorTree(e.to_string()))?;
            if status == BehaviorStatus::Running {
                self.behaviors.push_back(behavior);
            }
        }
        // STEP 3 — flush local state changes out to the remote / hardware.
        // Coalesce everything drained this step into ONE StateChange so the
        // remote/hardware see a single, consistent update per step. Changes are
        // drained in order, so later ones win: a set overrides an earlier unset of
        // the same key (and vice versa).
        let mut merged = StateChange::new();
        while let Some(change) = self.store_changes.try_recv() {
            for (key, value) in change.set {
                merged.unset.remove(&key);
                merged.set.insert(key, value);
            }
            for key in change.unset {
                merged.set.remove(&key);
                merged.unset.insert(key);
            }
        }
        if !merged.is_empty() {
            let _ = self.outbound.unbounded_send(Outbound::StateChanged(merged));
        }
        Ok(StepOutcome::Live)
    }

    /// Apply a state change to the blackboard (stepping thread only).
    fn apply(&self, change: StateChange) -> Result<(), RuntimeError> {
        self.store
            .write(change)
            .map_err(|e| RuntimeError::Store(e.to_string()))
    }

    /// Handle a command from the remote against the state / engine, then reply.
    fn handle_command(&mut self, cmd: BridgeCommand) -> Result<(), RuntimeError> {
        let result = match &cmd.op {
            BridgeOp::Get(keys) => {
                let values = self.store.read(keys);
                let array = values
                    .into_iter()
                    .map(|v| Value::Option(v.map(Box::new)))
                    .collect();
                Ok(CallResult {
                    ret: Value::ArrayValue(array),
                    mutated: Vec::new(),
                })
            }
            BridgeOp::Update(change) => match self.store.write(change.clone()) {
                Ok(()) => Ok(CallResult {
                    ret: Value::Unit,
                    mutated: Vec::new(),
                }),
                Err(e) => Err(e.to_string()),
            },
            BridgeOp::Call(_call) => {
                // TODO(next slice): dispatch the call through the engine.
                Err("call handling is not yet wired".to_string())
            }
            BridgeOp::ListKeys { prefix } => {
                // Introspection: enumerate the live (set) key paths, optionally
                // filtered by prefix, sorted for a deterministic reply.
                let snapshot = self.store.snapshot();
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
                // Introspection: enumerate registered module method names,
                // optionally filtered by prefix, sorted and deduped.
                let mut names: Vec<String> = self
                    .function_index
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
}

/// Native convenience: drive [`step`](Runtime::step) in a loop until the device
/// is unregistered, pacing with a short sleep. On the web, drive `step` from
/// `requestAnimationFrame` instead (this method would block the event loop).
#[cfg(not(target_arch = "wasm32"))]
impl Runtime {
    pub fn run(&mut self) -> Result<(), RuntimeError> {
        loop {
            if self.step()? == StepOutcome::Unregistered {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

/// The async I/O pump: forwards the bridge's command/device-info/data-requested
/// streams to the runtime, and drains the runtime's outbound changes to the
/// remote and the hardware. Built from `futures` only — no Tokio, no threads —
/// so it runs equally on a Tokio task or a browser `spawn_local`.
async fn io(
    bridge: Arc<dyn Bridge>,
    hal: Arc<dyn Hal>,
    inbound_tx: UnboundedSender<Inbound>,
    mut outbound_rx: UnboundedReceiver<Outbound>,
) {
    let forward = async {
        let commands = bridge.commands().await.map(Inbound::Command);
        let device_info = bridge
            .device_info_updated()
            .await
            .unwrap_or_else(|_| Box::pin(futures::stream::empty()))
            .map(Inbound::DeviceInfo);
        let data_requested = bridge.data_requested().await.map(Inbound::DataRequested);
        let mut merged = futures::stream::select(
            commands,
            futures::stream::select(device_info, data_requested),
        );
        while let Some(event) = merged.next().await {
            if inbound_tx.unbounded_send(event).is_err() {
                break; // the runtime was dropped
            }
        }
    };
    let drain = async {
        while let Some(out) = outbound_rx.next().await {
            match out {
                Outbound::StateChanged(change) => {
                    // Mirror local changes to the remote and the hardware.
                    let _ = bridge.send_data(change.clone()).await;
                    let _ = hal.write(change).await;
                }
            }
        }
    };
    futures::future::join(forward, drain).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_bridge::{CommandStream, DataRequestedStream, DeviceInfoStream};
    use arora_simple_data_store::NamespacedStore;
    use arora_types::data::{Key, StateChange};
    use async_trait::async_trait;
    use futures::channel::oneshot;

    /// A bridge that reports the device unregistered immediately and is otherwise
    /// silent.
    struct UnregisterBridge;

    #[async_trait]
    impl Bridge for UnregisterBridge {
        async fn get_device_info(&self) -> BridgeResult<Option<DeviceInfo>> {
            Ok(None)
        }
        async fn device_info_updated(&self) -> BridgeResult<DeviceInfoStream> {
            Ok(Box::pin(futures::stream::once(async { Ok(None) })))
        }
        async fn update_device_info(
            &self,
            info: Option<DeviceInfo>,
        ) -> BridgeResult<Option<DeviceInfo>> {
            Ok(info)
        }
        async fn data_requested(&self) -> DataRequestedStream {
            Box::pin(futures::stream::empty())
        }
        async fn send_data(&self, _data: StateChange) -> BridgeResult<()> {
            Ok(())
        }
        async fn commands(&self) -> CommandStream {
            Box::pin(futures::stream::empty())
        }
    }

    async fn build(bridge: Arc<dyn Bridge>) -> (Runtime, impl Future<Output = ()>) {
        let arora = Arora::start().await.expect("arora starts");
        Runtime::with_io(arora, Arc::new(arora_hal::FakeHal::new()), bridge)
    }

    /// Like [`build`], but runs the runtime against a caller-provided store.
    async fn build_in(
        bridge: Arc<dyn Bridge>,
        store: Arc<dyn DataStore>,
    ) -> (Runtime, impl Future<Output = ()>) {
        let arora = Arora::start().await.expect("arora starts");
        Runtime::with_io_in(arora, Arc::new(arora_hal::FakeHal::new()), bridge, store)
    }

    /// Drive `step` until the runtime reports unregistered, with a safety bound.
    async fn drive_until_unregistered(mut runtime: Runtime) {
        for _ in 0..1000 {
            match runtime.step().expect("step ok") {
                StepOutcome::Unregistered => return,
                // Sleep, not just yield: the io pump runs as a separate spawned
                // task, and `yield_now` is cooperative (CPU-time, not wall-clock)
                // — this tight loop can burn all 1000 iterations in microseconds,
                // before the pump is scheduled to deliver the unregister event on
                // a loaded runner. A small sleep gives the pump real time; the
                // loop still returns as soon as the event arrives (typically ms).
                StepOutcome::Live => tokio::time::sleep(std::time::Duration::from_millis(1)).await,
            }
        }
        panic!("runtime never reported unregistered");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stops_when_unregistered() {
        let (runtime, pump) = build(Arc::new(UnregisterBridge)).await;
        tokio::spawn(pump);
        drive_until_unregistered(runtime).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn runs_a_queued_tree() {
        let xml = r#"<root main_tree_to_execute="MainTree">
  <BehaviorTree ID="MainTree">
    <Sequence name="11111111-1111-4111-8111-111111111111">
      <Succeed name="22222222-2222-4222-8222-222222222222" />
    </Sequence>
  </BehaviorTree>
</root>"#;
        let (mut runtime, pump) = build(Arc::new(UnregisterBridge)).await;
        runtime.queue_groot_xml(xml).expect("tree queues");
        tokio::spawn(pump);
        drive_until_unregistered(runtime).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn get_and_update_commands_round_trip() {
        let (mut runtime, _pump) = build(Arc::new(UnregisterBridge)).await;
        let key = Key::from("greeting");

        // Update writes a value into the store.
        let (tx, rx) = oneshot::channel();
        let mut set = std::collections::HashMap::new();
        set.insert(key.clone(), Some(Value::String("hi".into())));
        let change = StateChange {
            set,
            unset: std::collections::HashSet::new(),
        };
        runtime
            .handle_command(BridgeCommand::new(BridgeOp::Update(change), tx))
            .unwrap();
        assert!(rx.await.unwrap().is_ok(), "update should succeed");

        // Get reads it back, wrapped as Option inside an ArrayValue.
        let (tx, rx) = oneshot::channel();
        runtime
            .handle_command(BridgeCommand::new(BridgeOp::Get(vec![key]), tx))
            .unwrap();
        let result = rx.await.unwrap().expect("get should succeed");
        assert_eq!(
            result.ret,
            Value::ArrayValue(vec![Value::Option(Some(Box::new(Value::String(
                "hi".into()
            ))))])
        );
    }

    /// A bridge that stays silent and never unregisters, so `step` keeps
    /// returning `Live` and a queued behavior ticks deterministically.
    struct SilentBridge;

    #[async_trait]
    impl Bridge for SilentBridge {
        async fn get_device_info(&self) -> BridgeResult<Option<DeviceInfo>> {
            Ok(None)
        }
        async fn device_info_updated(&self) -> BridgeResult<DeviceInfoStream> {
            Ok(Box::pin(futures::stream::empty()))
        }
        async fn update_device_info(
            &self,
            info: Option<DeviceInfo>,
        ) -> BridgeResult<Option<DeviceInfo>> {
            Ok(info)
        }
        async fn data_requested(&self) -> DataRequestedStream {
            Box::pin(futures::stream::empty())
        }
        async fn send_data(&self, _data: StateChange) -> BridgeResult<()> {
            Ok(())
        }
        async fn commands(&self) -> CommandStream {
            Box::pin(futures::stream::empty())
        }
    }

    /// A non-tree [`Behavior`]: writes one key through the shared store, done.
    struct WriteOnce;

    impl Behavior for WriteOnce {
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

    /// The runtime ticks a non-tree behavior just like a tree: swapping the
    /// interpreter is all it takes.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn runs_a_queued_non_tree_behavior() {
        let (mut runtime, pump) = build(Arc::new(SilentBridge)).await;
        tokio::spawn(pump);
        runtime.queue_behavior(Box::new(WriteOnce));

        // One step ticks the behavior, which writes through the shared store.
        runtime.step().expect("step");
        let (tx, rx) = oneshot::channel();
        runtime
            .handle_command(BridgeCommand::new(
                BridgeOp::Get(vec![Key::from("from_behavior")]),
                tx,
            ))
            .unwrap();
        let result = rx.await.unwrap().expect("get ok");
        assert_eq!(
            result.ret,
            Value::ArrayValue(vec![Value::Option(Some(Box::new(Value::String(
                "hi".into()
            ))))])
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn list_keys_enumerates_the_store_by_prefix() {
        let (mut runtime, _pump) = build(Arc::new(UnregisterBridge)).await;

        // Seed three keys across two prefixes.
        let mut set = std::collections::HashMap::new();
        set.insert(Key::from("face/mouth"), Some(Value::F32(0.5)));
        set.insert(Key::from("face/eyes"), Some(Value::F32(0.1)));
        set.insert(Key::from("body/hand"), Some(Value::F32(0.9)));
        let (tx, _rx) = oneshot::channel();
        runtime
            .handle_command(BridgeCommand::new(
                BridgeOp::Update(StateChange {
                    set,
                    unset: std::collections::HashSet::new(),
                }),
                tx,
            ))
            .unwrap();

        // ListKeys with a prefix returns only that subtree, sorted.
        let (tx, rx) = oneshot::channel();
        runtime
            .handle_command(BridgeCommand::new(
                BridgeOp::ListKeys {
                    prefix: Some("face".into()),
                },
                tx,
            ))
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
        runtime
            .handle_command(BridgeCommand::new(
                BridgeOp::ListMethods { prefix: None },
                tx,
            ))
            .unwrap();
        let methods = rx.await.unwrap().expect("list_methods ok");
        assert!(
            matches!(methods.ret, Value::ArrayValue(_)),
            "list_methods returns an array"
        );
    }

    /// A `Runtime` built over a `NamespacedStore` writes through `step()` under
    /// the device namespace: a write driven through the runtime's own store
    /// pipeline (here the bridge `Update` path, which `step()` dispatches) lands
    /// as `robotA/<key>` in the shared backend.
    ///
    /// This exercises the `Arc<dyn DataStore>` injection end-to-end: the runtime
    /// holds the namespaced view and never sees the prefix, while the mutualized
    /// `SimpleDataStore` ends up holding only the namespaced key. (A *behavior*
    /// writing a real leaf node under the namespace is covered by the
    /// `behavior_writes_store` integration test, which loads the
    /// test-behavior-tree-nodes module; `Arora` exposes no module-load API to run
    /// such a leaf through `step()` here.)
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn runtime_over_namespaced_store_writes_under_namespace() {
        let shared = SimpleDataStore::new();
        let store: Arc<dyn DataStore> =
            Arc::new(NamespacedStore::new(Arc::new(shared.clone()), "robotA"));
        let (mut runtime, _pump) = build_in(Arc::new(UnregisterBridge), store.clone()).await;

        // Drive a write through the runtime's store pipeline.
        let (tx, rx) = oneshot::channel();
        let mut set = std::collections::HashMap::new();
        set.insert(Key::from("greeting"), Some(Value::String("hi".into())));
        runtime
            .handle_command(BridgeCommand::new(
                BridgeOp::Update(StateChange {
                    set,
                    unset: std::collections::HashSet::new(),
                }),
                tx,
            ))
            .unwrap();
        assert!(rx.await.unwrap().is_ok(), "update should succeed");
        // Let the change flow out through a step (the flush stage drains the
        // store's change feed).
        assert_eq!(runtime.step().expect("step ok"), StepOutcome::Live);

        // In the shared backend the key lives under the device namespace…
        assert_eq!(
            shared.read(&[Key::from("robotA/greeting")]),
            vec![Some(Value::String("hi".into()))],
            "the runtime's write landed under the device namespace"
        );
        // …and NOT under the bare key.
        assert_eq!(
            shared.read(&[Key::from("greeting")]),
            vec![None],
            "the bare key must not be set in the shared store"
        );
    }

    /// A behavior that writes one key/value and is then `Done` — the minimal
    /// store-writing behavior, value-parameterized so two of them differ.
    struct WriteKey {
        key: &'static str,
        value: Value,
    }

    impl Behavior for WriteKey {
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

    /// ARORA-39 acceptance, end to end through `step()`: a queued behavior writes
    /// a key into the device-namespaced store, and *switching* to a different
    /// behavior changes what gets written. Driven by a real `Behavior` (not a
    /// Groot literal), so it sidesteps the typed-literal coercion of ARORA-43.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn behavior_writes_then_switching_changes_the_namespaced_store() {
        let shared = SimpleDataStore::new();
        let store: Arc<dyn DataStore> =
            Arc::new(NamespacedStore::new(Arc::new(shared.clone()), "robotA"));
        // Pump intentionally not spawned: with no inbound events the device never
        // unregisters, so `step()` stays `Live` and ticks the queued behavior.
        let (mut runtime, _pump) = build_in(Arc::new(UnregisterBridge), store).await;

        // A behavior writes greeting = "hi"; one step lands it under the namespace.
        runtime.queue_behavior(Box::new(WriteKey {
            key: "greeting",
            value: Value::String("hi".into()),
        }));
        assert_eq!(runtime.step().expect("step"), StepOutcome::Live);
        assert_eq!(
            shared.read(&[Key::from("robotA/greeting")]),
            vec![Some(Value::String("hi".into()))],
            "the behavior's write landed under the device namespace"
        );

        // Switch to a different behavior; the next step changes the stored value.
        runtime.queue_behavior(Box::new(WriteKey {
            key: "greeting",
            value: Value::String("bye".into()),
        }));
        assert_eq!(runtime.step().expect("step"), StepOutcome::Live);
        assert_eq!(
            shared.read(&[Key::from("robotA/greeting")]),
            vec![Some(Value::String("bye".into()))],
            "switching the queued behavior changed what was written"
        );
    }
}
