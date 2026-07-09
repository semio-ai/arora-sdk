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
//! spawns no threads, owns no async runtime, and never sleeps. It touches its
//! I/O seams — the [`Bridge`] and the [`Hal`] — through their synchronous
//! poll/push surface: `bridge.try_recv()` / `bridge.try_send()` and the HAL's
//! [`updates`](Hal::updates) subscription / [`try_send`](Hal::try_send). Any
//! real async work (a WebSocket, Zenoh, DDS) lives *inside* those
//! implementations, each owning its own task; the runtime never sees it.
//!
//! The embedder just drives `step`:
//!
//! - **native**: call [`run`](Runtime::run) (a thin `step` loop) on a thread;
//! - **web**: drive `step()` from `requestAnimationFrame` — or run the whole
//!   thing inside a Web Worker.
//!
//! Because the loop owns no async runtime and only pokes synchronous seams, it
//! moves into a Web Worker unchanged: the worker boundary is the seam's problem.

use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::Arc;

use arora_behavior::{golden, BehaviorContext, BehaviorInterpreter, BehaviorStatus};
use arora_behavior_tree::{
    behavior::BehaviorTreeInterpreter, schema_groot, tree_node::TreeNode, BehaviorTree,
    ModuleFunction,
};
use arora_bridge::{Bridge, BridgeCommand, BridgeOp, Inbound};
use arora_engine::engine::PinnedEngine;
use arora_hal::Hal;
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::CallResult;
use arora_types::data::{DataStore, Key, Slot, StateChange, Subscription};
use arora_types::value::Value;
use uuid::Uuid;

use crate::Arora;

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

/// A point-in-time copy of the runtime's live indicators.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TelemetrySnapshot {
    /// Measured step-loop frequency in Hz. `None` until the embedder's loop
    /// measures it (the native [`run`](Runtime::run) does; a custom `step`
    /// driver may not).
    pub loop_hz: Option<f32>,
    /// Whether a remote client currently claims the device (asks for data).
    pub claimed: bool,
    /// Name of the behavior currently being ticked, when one is queued and
    /// was given a name.
    pub behavior: Option<String>,
}

/// Shared, read-only view over the runtime's live indicators — loop frequency,
/// claim state, current behavior. Cloneable and thread-safe: the step loop
/// writes, observers (a TUI, a GUI, a metrics exporter) read
/// [`snapshot`](Telemetry::snapshot) at their own pace.
#[derive(Clone, Default)]
pub struct Telemetry {
    inner: Arc<std::sync::RwLock<TelemetrySnapshot>>,
}

impl Telemetry {
    /// Copy the current indicator values.
    pub fn snapshot(&self) -> TelemetrySnapshot {
        self.inner.read().expect("telemetry lock poisoned").clone()
    }

    fn update(&self, apply: impl FnOnce(&mut TelemetrySnapshot)) {
        apply(&mut self.inner.write().expect("telemetry lock poisoned"));
    }
}

/// A queued behavior interpreter plus the display name it was queued under (if
/// any).
struct QueuedBehavior {
    name: Option<String>,
    behavior: Box<dyn BehaviorInterpreter>,
}

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
    behaviors: VecDeque<QueuedBehavior>,
    telemetry: Telemetry,
    // The synchronous I/O seams the step drives directly. Each owns its own
    // async internally; the runtime only pokes their non-blocking poll/push.
    hal: Arc<dyn Hal>,
    bridge: Arc<dyn Bridge>,
    // The HAL's sensor feed, a sync subscription the step drains each frame.
    hal_updates: Subscription,
    store_changes: Subscription,
    // The golden clock: monotonic nanoseconds since start, advanced by each
    // step's `dt`, and direct slot handles to publish it into the store before
    // ticking. Kept local — STEP 3 never forwards the golden namespace outbound.
    time_ns: u64,
    time_slot: Box<dyn Slot>,
    dt_slot: Box<dyn Slot>,
}

impl Runtime {
    /// Wire an [`Arora`] (engine + behavior-tree module) to a HAL and a bridge,
    /// with a fresh, private [`SimpleDataStore`].
    ///
    /// Returns just the synchronous `Runtime` — there is no async pump to spawn.
    /// The bridge and HAL own any async internally, behind their synchronous
    /// poll/push seams; the embedder only drives [`step`](Runtime::step) on its
    /// own cadence.
    ///
    /// To share one store across several runtimes (and keep a handle to it for
    /// direct access), use [`with_io_in`](Runtime::with_io_in).
    pub fn with_io(arora: Arora, hal: Arc<dyn Hal>, bridge: Arc<dyn Bridge>) -> Self {
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
    ) -> Self {
        let Arora {
            engine,
            function_index,
        } = arora;
        let store_changes = store.subscribe();
        let time_slot = store.slot(&Key::from(golden::TIME));
        let dt_slot = store.slot(&Key::from(golden::DT));
        let hal_updates = hal.updates();

        Self {
            store,
            engine,
            function_index,
            behaviors: VecDeque::new(),
            telemetry: Telemetry::default(),
            hal,
            bridge,
            hal_updates,
            store_changes,
            time_ns: 0,
            time_slot,
            dt_slot,
        }
    }

    /// A shared handle over the runtime's live indicators, for observers such
    /// as an operator UI. Clone it freely; it stays readable after the
    /// runtime stops (values simply freeze).
    pub fn telemetry(&self) -> Telemetry {
        self.telemetry.clone()
    }

    /// Queue a behavior tree (as Groot XML) to be run on the next BT step.
    ///
    /// Each Groot `{var}` is bound to the data store under its own name — the
    /// Direct convention (variable name == store key) — so a behavior reading or
    /// writing `{var}` reads/writes the store directly during the tick. STEP 2's
    /// single-writer guarantee makes that race-free (no copy/sync needed).
    pub fn queue_groot_xml(&mut self, xml: &str) -> Result<(), RuntimeError> {
        self.queue_groot_xml_as(None, xml)
    }

    /// Like [`queue_groot_xml`](Runtime::queue_groot_xml), with a display name
    /// the runtime reports through [`telemetry`](Runtime::telemetry) while the
    /// tree runs (e.g. the tree's file stem or its Groot tree id).
    pub fn queue_named_groot_xml(&mut self, name: &str, xml: &str) -> Result<(), RuntimeError> {
        self.queue_groot_xml_as(Some(name.to_string()), xml)
    }

    fn queue_groot_xml_as(&mut self, name: Option<String>, xml: &str) -> Result<(), RuntimeError> {
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
        self.behaviors.push_back(QueuedBehavior {
            name,
            behavior: Box::new(BehaviorTreeInterpreter::new(
                behavior,
                self.function_index.clone(),
            )),
        });
        Ok(())
    }

    /// Queue any [`BehaviorInterpreter`] — a behavior-tree interpreter, a
    /// node-graph interpreter, or another executor — to be ticked on the next
    /// step. The runtime ticks it each step while it reports
    /// [`BehaviorStatus::Running`] and drops it once it reports
    /// [`BehaviorStatus::Done`].
    pub fn queue_behavior(&mut self, behavior: Box<dyn BehaviorInterpreter>) {
        self.behaviors.push_back(QueuedBehavior {
            name: None,
            behavior,
        });
    }

    /// Like [`queue_behavior`](Runtime::queue_behavior), with a display name
    /// the runtime reports through [`telemetry`](Runtime::telemetry) while the
    /// behavior runs.
    pub fn queue_named_behavior(&mut self, name: &str, behavior: Box<dyn BehaviorInterpreter>) {
        self.behaviors.push_back(QueuedBehavior {
            name: Some(name.to_string()),
            behavior,
        });
    }

    /// Advance one step: drain inbound bridge/HAL updates into the state, publish
    /// the frame clock into the golden keys, tick the behavior, then flush the
    /// resulting state changes out. Non-blocking; touches the state from this
    /// (single) thread only.
    ///
    /// `dt_ns` is the **nanoseconds** elapsed since the previous step, measured
    /// by the caller's driver ([`run`](Runtime::run) natively,
    /// `requestAnimationFrame` on the web). The runtime owns what it does with
    /// it: it advances the monotonic clock and publishes both under the golden
    /// keys ([`golden::TIME`], [`golden::DT`]) so behaviors read timing from the
    /// store rather than as a tick argument.
    pub fn step(&mut self, dt_ns: u64) -> Result<StepOutcome, RuntimeError> {
        // STEP 1a — HAL sensor updates (a synchronous subscription).
        while let Some(change) = self.hal_updates.try_recv() {
            self.apply(change)?;
        }
        // STEP 1b — bridge events, drained synchronously from the bridge itself
        // (it buffers them off its own transport task).
        while let Some(event) = self.bridge.try_recv() {
            match event {
                Inbound::Command(cmd) => self.handle_command(cmd)?,
                Inbound::DeviceInfo(Ok(None)) => return Ok(StepOutcome::Unregistered),
                Inbound::DeviceInfo(Ok(Some(_info))) => { /* TODO: apply device info */ }
                Inbound::DeviceInfo(Err(_e)) => { /* TODO: surface bridge error */ }
                // Claim state is surfaced through telemetry; publishing is not
                // (yet) gated on it — the bridge implementations gate delivery
                // themselves today.
                Inbound::DataRequested(requested) => {
                    self.telemetry.update(|t| t.claimed = requested);
                }
            }
        }
        // STEP 1c — publish the frame clock into the golden keys, before any
        // behavior ticks, so a behavior reads this step's `dt`/time straight from
        // the store. The runtime owns the clock: it advances the monotonic
        // accumulator by `dt` and writes both. These writes go into the store's
        // change feed like any other, but STEP 3 filters the golden namespace out
        // of what it forwards outbound.
        self.time_ns = self.time_ns.saturating_add(dt_ns);
        self.dt_slot
            .set(Some(Value::U64(dt_ns)))
            .map_err(|e| RuntimeError::Store(e.to_string()))?;
        self.time_slot
            .set(Some(Value::U64(self.time_ns)))
            .map_err(|e| RuntimeError::Store(e.to_string()))?;
        // STEP 2 — tick the active behavior (tree, node graph, …) against the
        // shared store; keep it queued while it is still running.
        if let Some(mut queued) = self.behaviors.pop_front() {
            self.telemetry.update(|t| {
                if t.behavior != queued.name {
                    t.behavior = queued.name.clone();
                }
            });
            let mut ctx = BehaviorContext {
                store: &*self.store,
                caller: &mut self.engine,
            };
            let status = queued
                .behavior
                .tick(&mut ctx)
                .map_err(|e| RuntimeError::BehaviorTree(e.to_string()))?;
            if status == BehaviorStatus::Running {
                self.behaviors.push_back(queued);
            } else {
                self.telemetry.update(|t| t.behavior = None);
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
                // The golden clock keys are runtime-local: drop them so the
                // remote/hardware never see the wall-clock churning every frame.
                if golden::is_golden(key.get_path()) {
                    continue;
                }
                merged.unset.remove(&key);
                merged.set.insert(key, value);
            }
            for key in change.unset {
                if golden::is_golden(key.get_path()) {
                    continue;
                }
                merged.set.remove(&key);
                merged.unset.insert(key);
            }
        }
        if !merged.is_empty() {
            // Mirror the coalesced change to the remote and the hardware through
            // their synchronous, non-blocking push seams. Each buffers/flushes
            // on its own task; neither blocks the step.
            self.bridge.try_send(&merged);
            self.hal.try_send(&merged);
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
        // Measure the achieved step frequency over ~1 s windows and publish it
        // through the telemetry handle.
        let mut window_start = std::time::Instant::now();
        let mut steps_in_window: u32 = 0;
        // Wall-clock delta between steps, in nanoseconds, fed to `step` as the
        // frame `dt`. `as_nanos()` is u128; a single step's delta is far under
        // u64 range, so the cast is lossless in practice.
        let mut last_step = std::time::Instant::now();
        loop {
            let now = std::time::Instant::now();
            let dt_ns = now.duration_since(last_step).as_nanos() as u64;
            last_step = now;
            if self.step(dt_ns)? == StepOutcome::Unregistered {
                return Ok(());
            }
            steps_in_window += 1;
            let elapsed = window_start.elapsed();
            if elapsed >= std::time::Duration::from_secs(1) {
                let hz = steps_in_window as f32 / elapsed.as_secs_f32();
                self.telemetry.update(|t| t.loop_hz = Some(hz));
                window_start = std::time::Instant::now();
                steps_in_window = 0;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_bridge::{BridgeResult, DeviceInfo};
    use arora_simple_data_store::NamespacedStore;
    use arora_types::data::{Key, StateChange};
    use async_trait::async_trait;
    use futures::channel::oneshot;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// A bridge that reports the device unregistered on its first poll and is
    /// otherwise silent.
    #[derive(Default)]
    struct UnregisterBridge {
        done: AtomicBool,
    }

    #[async_trait]
    impl Bridge for UnregisterBridge {
        fn try_recv(&self) -> Option<Inbound> {
            if !self.done.swap(true, Ordering::Relaxed) {
                Some(Inbound::DeviceInfo(Ok(None)))
            } else {
                None
            }
        }
        fn try_send(&self, _change: &StateChange) {}
        async fn get_device_info(&self) -> BridgeResult<Option<DeviceInfo>> {
            Ok(None)
        }
        async fn update_device_info(
            &self,
            info: Option<DeviceInfo>,
        ) -> BridgeResult<Option<DeviceInfo>> {
            Ok(info)
        }
    }

    async fn build(bridge: Arc<dyn Bridge>) -> Runtime {
        let arora = Arora::start().await.expect("arora starts");
        Runtime::with_io(arora, Arc::new(arora_hal::FakeHal::new()), bridge)
    }

    /// Like [`build`], but runs the runtime against a caller-provided store.
    async fn build_in(bridge: Arc<dyn Bridge>, store: Arc<dyn DataStore>) -> Runtime {
        let arora = Arora::start().await.expect("arora starts");
        Runtime::with_io_in(arora, Arc::new(arora_hal::FakeHal::new()), bridge, store)
    }

    /// Drive `step` until the runtime reports unregistered, with a safety bound.
    /// Fully synchronous now: the bridge hands over the unregister event through
    /// `try_recv` on the very next step, no pump to wait on.
    fn drive_until_unregistered(mut runtime: Runtime) {
        for _ in 0..1000 {
            if runtime.step(16_000_000).expect("step ok") == StepOutcome::Unregistered {
                return;
            }
        }
        panic!("runtime never reported unregistered");
    }

    #[tokio::test]
    async fn stops_when_unregistered() {
        let runtime = build(Arc::new(UnregisterBridge::default())).await;
        drive_until_unregistered(runtime);
    }

    #[tokio::test]
    async fn runs_a_queued_tree() {
        let xml = r#"<root main_tree_to_execute="MainTree">
  <BehaviorTree ID="MainTree">
    <Sequence name="11111111-1111-4111-8111-111111111111">
      <Succeed name="22222222-2222-4222-8222-222222222222" />
    </Sequence>
  </BehaviorTree>
</root>"#;
        let mut runtime = build(Arc::new(UnregisterBridge::default())).await;
        runtime.queue_groot_xml(xml).expect("tree queues");
        drive_until_unregistered(runtime);
    }

    #[tokio::test]
    async fn get_and_update_commands_round_trip() {
        let mut runtime = build(Arc::new(UnregisterBridge::default())).await;
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
        fn try_recv(&self) -> Option<Inbound> {
            None
        }
        fn try_send(&self, _change: &StateChange) {}
        async fn get_device_info(&self) -> BridgeResult<Option<DeviceInfo>> {
            Ok(None)
        }
        async fn update_device_info(
            &self,
            info: Option<DeviceInfo>,
        ) -> BridgeResult<Option<DeviceInfo>> {
            Ok(info)
        }
    }

    /// A non-tree [`BehaviorInterpreter`]: writes one key through the shared
    /// store, done.
    struct WriteOnce;

    impl BehaviorInterpreter for WriteOnce {
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
    #[tokio::test]
    async fn runs_a_queued_non_tree_behavior() {
        let mut runtime = build(Arc::new(SilentBridge)).await;
        runtime.queue_behavior(Box::new(WriteOnce));

        // One step ticks the behavior, which writes through the shared store.
        runtime.step(16_000_000).expect("step");
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

    #[tokio::test]
    async fn list_keys_enumerates_the_store_by_prefix() {
        let mut runtime = build(Arc::new(UnregisterBridge::default())).await;

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
    #[tokio::test]
    async fn runtime_over_namespaced_store_writes_under_namespace() {
        let shared = SimpleDataStore::new();
        let store: Arc<dyn DataStore> =
            Arc::new(NamespacedStore::new(Arc::new(shared.clone()), "robotA"));
        let mut runtime = build_in(Arc::new(UnregisterBridge::default()), store.clone()).await;

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
        assert_eq!(
            runtime.step(16_000_000).expect("step ok"),
            StepOutcome::Live
        );

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

    impl BehaviorInterpreter for WriteKey {
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
    /// behavior changes what gets written. Driven by a real
    /// `BehaviorInterpreter` (not a Groot literal), so it sidesteps the
    /// typed-literal coercion of ARORA-43.
    #[tokio::test]
    async fn behavior_writes_then_switching_changes_the_namespaced_store() {
        let shared = SimpleDataStore::new();
        let store: Arc<dyn DataStore> =
            Arc::new(NamespacedStore::new(Arc::new(shared.clone()), "robotA"));
        // SilentBridge never unregisters, so `step()` stays `Live` and ticks the
        // queued behavior each frame.
        let mut runtime = build_in(Arc::new(SilentBridge), store).await;

        // A behavior writes greeting = "hi"; one step lands it under the namespace.
        runtime.queue_behavior(Box::new(WriteKey {
            key: "greeting",
            value: Value::String("hi".into()),
        }));
        assert_eq!(runtime.step(16_000_000).expect("step"), StepOutcome::Live);
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
        assert_eq!(runtime.step(16_000_000).expect("step"), StepOutcome::Live);
        assert_eq!(
            shared.read(&[Key::from("robotA/greeting")]),
            vec![Some(Value::String("bye".into()))],
            "switching the queued behavior changed what was written"
        );
    }

    /// The runtime publishes the frame clock into the golden keys *before* it
    /// ticks, so a behavior reads `dt`/time from the store. Nanoseconds
    /// accumulate into `time`; `dt` reflects only the latest step.
    #[tokio::test]
    async fn golden_clock_is_published_to_the_store_each_step() {
        let store = Arc::new(SimpleDataStore::new());
        let mut runtime = build_in(Arc::new(SilentBridge), store.clone()).await;

        // Before any step the golden keys are unset.
        assert_eq!(store.read(&[Key::from(golden::DT)]), vec![None]);
        assert_eq!(store.read(&[Key::from(golden::TIME)]), vec![None]);

        // Step at 16 ms: dt and elapsed time both read 16_000_000 ns.
        assert_eq!(runtime.step(16_000_000).expect("step"), StepOutcome::Live);
        assert_eq!(
            store.read(&[Key::from(golden::DT)]),
            vec![Some(Value::U64(16_000_000))]
        );
        assert_eq!(
            store.read(&[Key::from(golden::TIME)]),
            vec![Some(Value::U64(16_000_000))]
        );

        // Step at 4 ms: dt resets to the latest delta, time accumulates to 20 ms.
        assert_eq!(runtime.step(4_000_000).expect("step"), StepOutcome::Live);
        assert_eq!(
            store.read(&[Key::from(golden::DT)]),
            vec![Some(Value::U64(4_000_000))]
        );
        assert_eq!(
            store.read(&[Key::from(golden::TIME)]),
            vec![Some(Value::U64(20_000_000))]
        );
    }

    /// A bridge that records every `try_send` payload, and otherwise stays
    /// silent (never unregisters), so a test can inspect what the runtime
    /// actually forwards outbound.
    struct RecordingBridge {
        sent: Arc<std::sync::Mutex<Vec<StateChange>>>,
    }

    #[async_trait]
    impl Bridge for RecordingBridge {
        fn try_recv(&self) -> Option<Inbound> {
            None
        }
        fn try_send(&self, change: &StateChange) {
            self.sent.lock().unwrap().push(change.clone());
        }
        async fn get_device_info(&self) -> BridgeResult<Option<DeviceInfo>> {
            Ok(None)
        }
        async fn update_device_info(
            &self,
            info: Option<DeviceInfo>,
        ) -> BridgeResult<Option<DeviceInfo>> {
            Ok(info)
        }
    }

    /// The golden clock keys stay local: the runtime never forwards them out to
    /// the bridge, even though an ordinary behavior write on the same step is
    /// forwarded. This is what keeps the wall-clock (which changes every frame)
    /// off the wire.
    #[tokio::test]
    async fn golden_keys_are_not_forwarded_outbound() {
        let sent = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut runtime = build(Arc::new(RecordingBridge { sent: sent.clone() })).await;

        // A behavior writes one ordinary key; that write must reach the bridge.
        runtime.queue_behavior(Box::new(WriteKey {
            key: "greeting",
            value: Value::String("hi".into()),
        }));

        // Step a few times; `try_send` records synchronously, in-line with step.
        for _ in 0..5 {
            runtime.step(16_000_000).expect("step");
        }

        let sent = sent.lock().unwrap();
        let forwarded_keys: Vec<String> = sent
            .iter()
            .flat_map(|c| c.set.keys().map(|k| k.path.clone()))
            .collect();
        assert!(
            forwarded_keys.iter().any(|k| k.as_str() == "greeting"),
            "the ordinary behavior write should be forwarded outbound, got {forwarded_keys:?}"
        );
        assert!(
            !forwarded_keys.iter().any(|k| golden::is_golden(k.as_str())),
            "golden keys must never be forwarded outbound, got {forwarded_keys:?}"
        );
    }
}
