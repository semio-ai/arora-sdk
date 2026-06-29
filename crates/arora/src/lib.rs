//! Opinionated Arora runtime.
//!
//! Where [`arora_engine`] is the bare, unopinionated runtime, this crate wires
//! a ready-to-use [`Arora`]: an engine with the WebAssembly and native
//! executors, the behavior-tree node module loaded, and the Semio backend
//! ([`semio_record`]) underneath. It can run a behavior tree handed to it at
//! startup (as Groot XML) and otherwise idles, waiting for behavior trees that
//! will soon arrive over the bridge.

pub mod launcher;
pub mod runtime;

#[cfg(feature = "native")]
pub use launcher::launch;

use anyhow::{anyhow, Context, Result};
use arora_behavior_tree::{
    arora_generated::behavior_tree::status::Status, run_behavior_tree, schema_groot,
    tree_node::TreeNode, BehaviorTree, ModuleFunction,
};
use arora_engine::engine::{EngineBuilder, PinnedEngine};
#[cfg(feature = "native")]
use arora_engine::executor::{native::NativeExecutor, wasm::WebAssemblyExecutor};
use arora_module_core::resolve::resolve_low_module;
use arora_registry::{
    local::LocalRegistry,
    local_yaml::{load_records, RecordKind},
    EditableRegistry, ModuleFrozen,
};
use arora_types::module::low::{Header, ModuleDefinition};
use include_dir::{include_dir, Dir};
use semio_record::module::v0::frozen::ExportKind;
use std::collections::HashMap;
use std::rc::Rc;
use uuid::Uuid;

/// The behavior-tree type records (Status, TickId, …) that the behavior-tree
/// node module imports, embedded at build time so they load without a
/// filesystem (e.g. in the browser). They live in the
/// `arora-behavior-tree-types-yaml` crate.
static TYPE_RECORDS: Dir<'static> =
    include_dir!("$CARGO_MANIFEST_DIR/../arora-behavior-tree-types-yaml/records");

/// The behavior-tree node implementations, compiled to wasm and embedded at
/// build time via the `behavior-tree-nodes` artifact dependency. Only with the
/// `native` feature — without it (e.g. the wasm build) the bytes are supplied at
/// runtime (see [`Arora::start_with_nodes`]).
#[cfg(feature = "native")]
const BEHAVIOR_TREE_NODES_WASM: &[u8] = include_bytes!(env!("BT_NODES_WASM"));

/// The behavior-tree-nodes module header, embedded at build time (a committed
/// source file, so this is safe to inline).
const BEHAVIOR_TREE_NODES_HEADER: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../modules/behavior-tree-nodes/src/arora_generated/module.yaml"
));

/// An opinionated Arora runtime: an engine pre-wired with the behavior-tree
/// module, ready to run behavior trees.
pub struct Arora {
    engine: PinnedEngine,
    /// Behavior-tree node functions, keyed by their function UUID.
    function_index: Rc<HashMap<Uuid, ModuleFunction>>,
}

impl Arora {
    /// Start the runtime with the behavior-tree node module embedded at build
    /// time. Requires the `native` feature; without it (e.g. the wasm build) the
    /// artifact isn't embedded, so use [`start_with_nodes`](Arora::start_with_nodes)
    /// with bytes supplied by the host.
    #[cfg(feature = "native")]
    pub async fn start() -> Result<Self> {
        Self::start_with_nodes(BEHAVIOR_TREE_NODES_WASM).await
    }

    /// Start the runtime with the given behavior-tree node module wasm: build the
    /// engine (the browser host on wasm, the wasmtime + native hosts otherwise)
    /// and load the node module from `nodes_wasm`.
    pub async fn start_with_nodes(nodes_wasm: &[u8]) -> Result<Self> {
        let mut engine = build_engine()?;

        let mut registry = LocalRegistry::new();
        let mut function_index = HashMap::new();

        // The behavior-tree node module imports the behavior-tree type records
        // (Status, TickId, …), so seed those into the registry first. They are
        // embedded from the arora-behavior-tree-types-yaml crate (see
        // `TYPE_RECORDS`), so this works without a filesystem.
        load_embedded_records(&mut registry)
            .await
            .map_err(|e| anyhow!("failed to load behavior-tree type records: {e:?}"))?;

        let header: Header = serde_yaml::from_str(BEHAVIOR_TREE_NODES_HEADER)
            .context("invalid behavior-tree-nodes module header")?;
        let module_id = header.id;
        let module_version = header.version.clone();

        // Resolve the module's exports against the registry, now seeded with
        // the behavior-tree type records that the module imports.
        let module = resolve_low_module(header.clone(), &mut registry)
            .await
            .map_err(|e| anyhow!("failed to resolve behavior-tree-nodes module: {e:?}"))?
            .module;
        index_module_functions(&module_id, &module, &mut function_index);
        registry
            .add_module(module_id, module_version.into(), module)
            .await
            .map_err(|e| anyhow!("failed to register behavior-tree-nodes module: {e:?}"))?;

        engine
            .load_module(ModuleDefinition {
                schema_version: 0,
                header,
                executable: nodes_wasm.to_vec().into_boxed_slice(),
            })
            .map_err(|e| anyhow!("failed to load behavior-tree-nodes module: {e:?}"))?;

        Ok(Self {
            engine,
            function_index: Rc::new(function_index),
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
        .add_executor(WebAssemblyExecutor::new().context("failed to create wasm executor")?)
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

/// Load the embedded behavior-tree type records ([`TYPE_RECORDS`]) into the
/// registry, in dependency order, without touching a filesystem. The records are
/// grouped into `folder` / `enumeration` / `structure` / `module` subdirectories
/// (mirroring [`arora_registry::local_yaml::load_records_from_yaml_dir`]).
async fn load_embedded_records(registry: &mut dyn EditableRegistry) -> Result<()> {
    let mut entries: Vec<(RecordKind, String, String)> = Vec::new();
    for subdir in TYPE_RECORDS.dirs() {
        let kind = match subdir.path().file_name().and_then(|n| n.to_str()) {
            Some("folder") => RecordKind::Folder,
            Some("enumeration") => RecordKind::Enumeration,
            Some("structure") => RecordKind::Structure,
            Some("module") => RecordKind::Module,
            _ => continue,
        };
        for file in subdir.files() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
                continue;
            }
            let (Some(stem), Some(content)) = (
                path.file_stem().and_then(|s| s.to_str()),
                file.contents_utf8(),
            ) else {
                continue;
            };
            entries.push((kind, stem.to_string(), content.to_string()));
        }
    }
    let records = entries
        .iter()
        .map(|(kind, stem, content)| (*kind, stem.as_str(), content.clone()));
    load_records(records, registry)
        .await
        .map_err(|e| anyhow!("failed to load embedded records: {e:?}"))
}

/// Index a frozen module's exported functions by their UUID, so the
/// behavior-tree runtime can look them up when ticking nodes.
fn index_module_functions(
    module_id: &Uuid,
    module: &ModuleFrozen,
    index: &mut HashMap<Uuid, ModuleFunction>,
) {
    for (export_id, export) in &module.exports {
        let ExportKind::Function(function) = &export.kind;
        index.insert(
            *export_id,
            ModuleFunction {
                module_id: *module_id,
                function_id: *export_id,
                function_name: export.name.clone(),
                function: function.clone(),
            },
        );
    }
}
