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

use arora_behavior_tree::{
    arora_generated::behavior_tree::status::Status, run_behavior_tree, schema_groot,
    tree_node::TreeNode, BehaviorTree, ModuleFunction,
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
    // Owned and touched ONLY by the stepping thread (single-threaded state):
    store: SimpleDataStore,
    engine: PinnedEngine,
    function_index: Rc<HashMap<Uuid, ModuleFunction>>,
    trees: VecDeque<BehaviorTree>,
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
        Self::with_io_in(arora, hal, bridge, SimpleDataStore::new())
    }

    /// Like [`with_io`](Runtime::with_io), but runs against the caller-provided
    /// `store` rather than a private one.
    ///
    /// Because [`SimpleDataStore`] is cheaply cloneable and clones share the same
    /// storage, one instance handed to several runtimes is a single shared
    /// blackboard — and the embedder keeps its own clone for direct,
    /// high-performance access and subscription. This is how Studio mutualizes one
    /// store across every spawned device (namespaced by device key), reading and
    /// writing it directly.
    pub fn with_io_in(
        arora: Arora,
        hal: Arc<dyn Hal>,
        bridge: Arc<dyn Bridge>,
        store: SimpleDataStore,
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
            trees: VecDeque::new(),
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
        self.trees.push_back(behavior);
        Ok(())
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
        // STEP 2 — tick the behavior tree(s); they read/write the same state.
        if let Some(tree) = self.trees.pop_front() {
            self.tick_tree(&tree)?;
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
        };
        cmd.reply(result);
        Ok(())
    }

    /// Tick a behavior tree to a terminal status. The tree drives the engine,
    /// which manages its own blocking runtime — fine on the stepping thread.
    fn tick_tree(&mut self, tree: &BehaviorTree) -> Result<Status, RuntimeError> {
        run_behavior_tree(tree, self.function_index.clone(), &mut self.engine, false)
            .map_err(|e| RuntimeError::BehaviorTree(format!("run: {e:?}")))
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
}
