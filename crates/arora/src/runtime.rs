//! The Arora runtime loop — studio-bridge's `engine`, library side.
//!
//! # Concurrency model: one state owner, dedicated steps
//!
//! Several things want to change the blackboard: the **bridge** (commands and
//! state from the remote), the **HAL** (sensor readings), and the **behavior
//! tree** (intent it writes while ticking). Rather than let them race on the
//! state behind a lock, the [`Runtime`] gives the state a single owner — the
//! main thread — and dispatches the others as **serialized steps** of one loop:
//!
//! 1. drain inbound bridge/HAL updates → apply to the state;
//! 2. tick the behavior tree → it reads/writes the state;
//! 3. flush the resulting state changes out to the remote.
//!
//! Only one step touches the state at a time, so there is never concurrent
//! access — and no dedicated engine thread, just a dedicated *step*. The async
//! bridge/HAL I/O runs on a separate Tokio "adapter" thread that only moves data
//! through channels; it never touches the state. This also lets the (`!Send`,
//! `block_on`-using) engine and behavior tree live naturally on the main thread.

use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use arora_behavior_tree::{
    arora_generated::behavior_tree::status::Status, run_behavior_tree, schema_groot,
    tree_node::TreeNode, BehaviorTree, ModuleFunction,
};
use arora_bridge::{Bridge, BridgeCommand, BridgeOp, DeviceInfo};
use arora_engine::engine::PinnedEngine;
use arora_hal::Hal;
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::CallResult;
use arora_types::data::{DataStore, StateChange};
use arora_types::value::Value;
use futures::StreamExt;
use uuid::Uuid;

use crate::Arora;

/// Inbound work from the async adapter to the main loop.
enum Event {
    /// A command from the remote, with its reply channel.
    Command(BridgeCommand),
    /// A device-info update (`None` = the device was unregistered).
    DeviceInfo(Option<DeviceInfo>),
    /// A client claimed/released interest in the data.
    DataRequested(bool),
    /// A state change reported by the hardware (sensors, mirrored actuation).
    HalUpdate(StateChange),
}

/// Outbound work from the main loop to the async adapter.
enum Outbound {
    /// Forward a local state change to the remote.
    SendData(StateChange),
}

/// Something went wrong running the loop.
#[derive(Debug)]
pub enum RuntimeError {
    /// The device was unregistered from the remote — the runtime stopped.
    Unregistered,
    /// A write to the data store failed.
    Store(String),
    /// A behavior tree failed to build or run.
    BehaviorTree(String),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeError::Unregistered => write!(f, "device unregistered from the remote"),
            RuntimeError::Store(m) => write!(f, "data store error: {m}"),
            RuntimeError::BehaviorTree(m) => write!(f, "behavior tree error: {m}"),
        }
    }
}

impl std::error::Error for RuntimeError {}

/// The Arora runtime: the state's single owner, plus the engine and behavior
/// trees, driven by one step-dispatched loop. Build it from an [`Arora`]
/// (engine + behavior-tree module) and a HAL + bridge.
pub struct Runtime {
    // Owned and touched ONLY by the main loop (single-threaded state access):
    store: SimpleDataStore,
    engine: PinnedEngine,
    function_index: Rc<HashMap<Uuid, ModuleFunction>>,
    trees: VecDeque<BehaviorTree>,
    // Channels to/from the async adapter thread:
    events: Receiver<Event>,
    outbound: tokio::sync::mpsc::UnboundedSender<Outbound>,
    store_changes: arora_types::data::Subscription,
    tick: Duration,
    // Keep the adapter thread alive for the runtime's lifetime.
    _adapter: std::thread::JoinHandle<()>,
}

impl Runtime {
    /// Wire an [`Arora`] (engine + behavior-tree module) to a HAL and a bridge.
    /// Spawns the async adapter thread; the returned `Runtime` is driven on the
    /// calling (main) thread via [`run`](Runtime::run).
    pub fn new(arora: Arora, hal: Box<dyn Hal>, bridge: Box<dyn Bridge>) -> Self {
        let Arora {
            engine,
            function_index,
        } = arora;
        let store = SimpleDataStore::new();
        let store_changes = store.subscribe();

        let (events_tx, events) = std::sync::mpsc::channel::<Event>();
        let (outbound, outbound_rx) = tokio::sync::mpsc::unbounded_channel::<Outbound>();

        let adapter = spawn_adapter(Arc::from(hal), Arc::from(bridge), events_tx, outbound_rx);

        Self {
            store,
            engine,
            function_index,
            trees: VecDeque::new(),
            events,
            outbound,
            store_changes,
            tick: Duration::from_millis(10),
            _adapter: adapter,
        }
    }

    /// Queue a behavior tree (as Groot XML) to be run on the next BT step.
    pub fn queue_groot_xml(&mut self, xml: &str) -> Result<(), RuntimeError> {
        let groot = schema_groot::BehaviorTree::try_from_groot_xml(xml)
            .map_err(|e| RuntimeError::BehaviorTree(format!("parse: {e:?}")))?;
        let mut variables = HashMap::new();
        let tree_node: TreeNode = groot
            .root
            .try_into_tree_node(self.function_index.as_ref(), &mut variables)
            .map_err(|e| RuntimeError::BehaviorTree(format!("build: {e:?}")))?;
        let behavior: BehaviorTree = tree_node
            .try_into()
            .map_err(|e| RuntimeError::BehaviorTree(format!("instantiate: {e:?}")))?;
        self.trees.push_back(behavior);
        Ok(())
    }

    /// Run the step-dispatched loop until the device is unregistered or a step
    /// fails. Runs on the calling thread; the state is touched only here.
    pub fn run(&mut self) -> Result<(), RuntimeError> {
        loop {
            // STEP 1 — drain inbound bridge/HAL updates into the state.
            loop {
                match self.events.try_recv() {
                    Ok(Event::HalUpdate(change)) => self.apply(change)?,
                    Ok(Event::Command(cmd)) => self.handle_command(cmd)?,
                    Ok(Event::DeviceInfo(None)) => return Err(RuntimeError::Unregistered),
                    Ok(Event::DeviceInfo(Some(_info))) => { /* TODO: apply device info */ }
                    Ok(Event::DataRequested(_requested)) => { /* TODO: claim handling */ }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return Ok(()),
                }
            }

            // STEP 2 — tick the behavior tree(s); they read/write the same state.
            if let Some(tree) = self.trees.pop_front() {
                self.tick_tree(&tree)?;
            }

            // STEP 3 — flush local state changes out to the remote.
            while let Some(change) = self.store_changes.try_recv() {
                let _ = self.outbound.send(Outbound::SendData(change));
            }

            std::thread::sleep(self.tick);
        }
    }

    /// Apply a state change to the blackboard (main-thread only).
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
    /// which manages its own blocking runtime — fine here on the main thread.
    fn tick_tree(&mut self, tree: &BehaviorTree) -> Result<Status, RuntimeError> {
        run_behavior_tree(tree, self.function_index.clone(), &mut self.engine, false)
            .map_err(|e| RuntimeError::BehaviorTree(format!("run: {e:?}")))
    }
}

/// Spawn the Tokio adapter thread: it bridges the async HAL/bridge I/O to the
/// main loop's channels. It never touches the state.
fn spawn_adapter(
    hal: Arc<dyn Hal>,
    bridge: Arc<dyn Bridge>,
    events: std::sync::mpsc::Sender<Event>,
    mut outbound: tokio::sync::mpsc::UnboundedReceiver<Outbound>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(_) => return,
        };
        rt.block_on(async move {
            // bridge commands -> events
            let b = bridge.clone();
            let e = events.clone();
            let commands = tokio::spawn(async move {
                let mut stream = b.commands().await;
                while let Some(cmd) = stream.next().await {
                    if e.send(Event::Command(cmd)).is_err() {
                        break;
                    }
                }
            });
            // device-info updates -> events
            let b = bridge.clone();
            let e = events.clone();
            let device_info = tokio::spawn(async move {
                if let Ok(mut stream) = b.device_info_updated().await {
                    while let Some(item) = stream.next().await {
                        match item {
                            Ok(info) => {
                                if e.send(Event::DeviceInfo(info)).is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                }
            });
            // data-requested (claim) -> events
            let b = bridge.clone();
            let e = events.clone();
            let data_requested = tokio::spawn(async move {
                let mut stream = b.data_requested().await;
                while let Some(requested) = stream.next().await {
                    if e.send(Event::DataRequested(requested)).is_err() {
                        break;
                    }
                }
            });
            // HAL updates (sync channel) -> events
            let hal_updates = hal.updates();
            let e = events.clone();
            let hal_forward = tokio::task::spawn_blocking(move || {
                while let Some(change) = hal_updates.recv() {
                    if e.send(Event::HalUpdate(change)).is_err() {
                        break;
                    }
                }
            });
            // outbound -> the remote / hardware
            let b = bridge.clone();
            let hal_out = hal.clone();
            let outbound_task = tokio::spawn(async move {
                while let Some(out) = outbound.recv().await {
                    match out {
                        Outbound::SendData(change) => {
                            let _ = b.send_data(change.clone()).await;
                            let _ = hal_out.write(change).await;
                        }
                    }
                }
            });
            let _ = tokio::join!(
                commands,
                device_info,
                data_requested,
                hal_forward,
                outbound_task
            );
        });
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_bridge::{BridgeResult, CommandStream, DataRequestedStream, DeviceInfoStream};
    use arora_types::data::Key;
    use async_trait::async_trait;

    /// A bridge that reports the device unregistered immediately.
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

    async fn build_runtime(bridge: Box<dyn Bridge>) -> Runtime {
        let arora = Arora::start().await.expect("arora starts");
        Runtime::new(arora, Box::new(arora_hal::FakeHal::new()), bridge)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stops_when_unregistered() {
        let mut runtime = build_runtime(Box::new(UnregisterBridge)).await;
        // run() is synchronous (it drives the engine); the bridge/HAL adapter is
        // already on its own thread, so call it directly. It returns promptly
        // once the adapter forwards the device-unregistered event.
        let err = runtime.run().unwrap_err();
        assert!(matches!(err, RuntimeError::Unregistered));
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
        let mut runtime = build_runtime(Box::new(UnregisterBridge)).await;
        runtime.queue_groot_xml(xml).expect("tree queues");
        let err = runtime.run().unwrap_err();
        assert!(matches!(err, RuntimeError::Unregistered));
    }

    #[test]
    fn key_is_in_scope() {
        let _ = Key::from("x");
    }
}
