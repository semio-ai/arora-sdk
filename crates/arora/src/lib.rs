//! Opinionated Arora runtime.
//!
//! Where [`arora_engine`] is the bare, unopinionated runtime, this crate wires
//! a ready-to-use [`Arora`]: an engine with the WebAssembly and native
//! executors. The basic behavior-tree control nodes are wired natively into
//! [`arora_behavior_tree`], so no module needs to be loaded to run a tree of
//! them. It can run a behavior tree handed to it at startup (as Groot XML) and
//! otherwise idles, waiting for behavior trees that will soon arrive over the
//! bridge.

#[cfg(feature = "native")]
pub mod headless;
pub mod launcher;
pub mod runtime;

#[cfg(feature = "native")]
pub use launcher::{launch, launch_with};

use anyhow::{anyhow, Result};
use arora_behavior_tree::{
    arora_generated::behavior_tree::status::Status, run_behavior_tree, schema_groot,
    tree_node::TreeNode, BehaviorTree, ModuleFunction,
};
use arora_engine::engine::{EngineBuilder, PinnedEngine};
#[cfg(feature = "native")]
use arora_engine::executor::{native::NativeExecutor, wasm::WebAssemblyExecutor};
use std::collections::HashMap;
use std::rc::Rc;
use uuid::Uuid;

/// An opinionated Arora runtime: an engine with the basic behavior-tree control
/// nodes wired natively, ready to run behavior trees.
pub struct Arora {
    engine: PinnedEngine,
    /// Module functions referenced by behavior-tree nodes, keyed by function
    /// UUID. The basic control nodes are dispatched natively and are not in this
    /// index; it stays empty until a runtime loads a real module.
    function_index: Rc<HashMap<Uuid, ModuleFunction>>,
}

impl Arora {
    /// Start the runtime: build the engine (the browser host on wasm, the
    /// wasmtime + native hosts otherwise). The basic behavior-tree control nodes
    /// are wired natively, so nothing needs to be loaded to run a tree of them.
    pub async fn start() -> Result<Self> {
        let engine = build_engine()?;
        Ok(Self {
            engine,
            function_index: Rc::new(HashMap::new()),
        })
    }

    /// Run a behavior tree supplied as Groot XML, ticking it until it reaches a
    /// terminal status.
    pub fn run_groot_xml(&mut self, xml: &str) -> Result<Status> {
        let groot = schema_groot::BehaviorTree::try_from_groot_xml(xml)
            .map_err(|e| anyhow!("failed to parse Groot XML: {e:?}"))?;
        let mut variables = HashMap::new();
        let tree_node: TreeNode = groot
            .root
            .try_into_tree_node(self.function_index.as_ref(), &mut variables)
            .map_err(|e| anyhow!("failed to build behavior tree from Groot: {e:?}"))?;
        let behavior: BehaviorTree = tree_node
            .try_into()
            .map_err(|e| anyhow!("failed to instantiate behavior tree: {e:?}"))?;
        run_behavior_tree(
            &behavior,
            self.function_index.clone(),
            &mut self.engine,
            false,
        )
        .map_err(|e| anyhow!("behavior tree run failed: {e:?}"))
    }

    /// Idle forever, doing nothing.
    ///
    /// This is the placeholder for the steady state: a future bridge channel
    /// will deliver behavior trees to run here. It is intentionally synchronous
    /// (a plain sleep): ticking a tree drives the wasm executor, which manages
    /// its own blocking runtime and must not run inside a Tokio runtime.
    pub fn run_forever(&mut self) -> Result<()> {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(100));
            // TODO: receive behavior trees over the bridge and run them.
        }
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
                .map_err(|e| anyhow!("failed to create wasm executor: {e}"))?,
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
