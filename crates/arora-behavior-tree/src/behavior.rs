//! [`BehaviorTreeInterpreter`]: the [`BehaviorInterpreter`] that runs an Arora
//! behavior tree — an interpreter over the shared [`Graph`] model.

use std::collections::HashMap;
use std::rc::Rc;

use arora_behavior::graph::{Graph, GraphDiff};
use arora_behavior::{BehaviorContext, BehaviorError, BehaviorInterpreter, BehaviorStatus};
use arora_types::data::{DataStore, Key, Slot};
use uuid::Uuid;

use crate::error::BehaviorTreeError;
use crate::graph::build_behavior_tree;
use crate::{run_behavior_tree, schema_groot, BehaviorTree, ModuleFunction};

/// The [`BehaviorInterpreter`] that runs a [`BehaviorTree`].
///
/// It is an executor, not a behavior: construct it **empty and ready** with
/// [`new`](Self::new) — it holds only the module-function index it needs to
/// resolve call nodes, no tree — then load a behavior *into* it as a separate
/// step. It is never swapped.
///
/// With a tree loaded, each tick runs the tree to a terminal status
/// (success/failure), so it reports [`BehaviorStatus::Done`] — the run-once
/// semantics the engine's queued trees already had. With **no** tree loaded it
/// idles: every tick is a no-op reporting [`BehaviorStatus::Running`], so the
/// interpreter stays installed (it is never dropped) waiting for a behavior.
///
/// A behavior loaded from the shared [`Graph`] — [`load_graph`](Self::load_graph),
/// or Groot XML via [`load_groot`](Self::load_groot), which lowers onto the
/// graph — stays **editable**: [`apply`](BehaviorInterpreter::apply) mutates
/// the graph and re-lowers the tree against the context's store. A raw
/// [`BehaviorTree`] loaded with [`load`](Self::load) ticks fine but has no
/// authored graph, so `apply` is rejected.
pub struct BehaviorTreeInterpreter {
    tree: Option<BehaviorTree>,
    function_index: Rc<HashMap<Uuid, ModuleFunction>>,
    /// The authored graph behind the loaded tree, when there is one — what
    /// [`apply`](BehaviorInterpreter::apply) edits and re-lowers. The store the
    /// slots resolve against is never retained: lowering borrows it (at load
    /// from the caller, at apply from the tick context).
    graph: Option<Graph>,
}

/// Bind a `{var}` name to the store slot under that name — the Direct
/// convention (variable name == store key).
fn direct_resolver(store: &dyn DataStore) -> impl Fn(&str) -> Option<Box<dyn Slot>> + '_ {
    move |name: &str| Some(store.slot(&Key::from(name)))
}

impl BehaviorTreeInterpreter {
    /// Construct an empty, ready interpreter over the module-function index its
    /// call nodes resolve against. It holds no behavior until one is loaded with
    /// [`load`](Self::load), [`load_graph`](Self::load_graph), or
    /// [`load_groot`](Self::load_groot); until then its tick idles.
    pub fn new(function_index: Rc<HashMap<Uuid, ModuleFunction>>) -> Self {
        Self {
            tree: None,
            function_index,
            graph: None,
        }
    }

    /// Load an already-built [`BehaviorTree`] into the interpreter, replacing
    /// any behavior currently loaded. A raw tree carries no authored graph, so
    /// [`apply`](BehaviorInterpreter::apply) is rejected until a graph is
    /// loaded instead.
    pub fn load(&mut self, behavior: BehaviorTree) {
        self.tree = Some(behavior);
        self.graph = None;
    }

    /// Load a behavior from the shared [`Graph`], replacing any behavior
    /// currently loaded: lowers the graph to a runnable tree, binding its
    /// variables to `store`'s slots under their own names (the Direct
    /// convention). `store` must be the same store the device ticks against;
    /// it is only borrowed to resolve the slots — the tree keeps the slots,
    /// the interpreter keeps the graph (for edition), and nobody keeps the
    /// store.
    pub fn load_graph(
        &mut self,
        graph: Graph,
        store: &dyn DataStore,
    ) -> Result<(), BehaviorTreeError> {
        let tree = build_behavior_tree(&graph, &direct_resolver(store))?;
        self.tree = Some(tree);
        self.graph = Some(graph);
        Ok(())
    }

    /// Load a behavior tree from Groot XML, replacing any behavior currently
    /// loaded. The XML lowers onto the shared [`Graph`] (names → arora ids,
    /// `{var}` → named variables) and loads through
    /// [`load_graph`](Self::load_graph), so a Groot-loaded behavior is editable
    /// like any other graph.
    pub fn load_groot(
        &mut self,
        xml: &str,
        store: &dyn DataStore,
    ) -> Result<(), BehaviorTreeError> {
        let groot = schema_groot::BehaviorTree::try_from_groot_xml(xml)?;
        let graph = groot.into_graph(self.function_index.as_ref())?;
        self.load_graph(graph, store)
    }

    /// The authored graph, if the loaded behavior came from one.
    pub fn graph(&self) -> Option<&Graph> {
        self.graph.as_ref()
    }
}

impl BehaviorInterpreter for BehaviorTreeInterpreter {
    fn tick(&mut self, ctx: &mut BehaviorContext) -> Result<BehaviorStatus, BehaviorError> {
        // No behavior loaded: idle. The interpreter stays installed — it is an
        // executor waiting for a tree, not something to drop.
        let Some(tree) = self.tree.as_ref() else {
            return Ok(BehaviorStatus::Running);
        };
        run_behavior_tree(tree, self.function_index.clone(), ctx.caller, false).map_err(|e| {
            BehaviorError {
                message: format!("behavior tree: {e:?}"),
            }
        })?;
        Ok(BehaviorStatus::Done)
    }

    fn apply(&mut self, diff: GraphDiff, ctx: &mut BehaviorContext) -> Result<(), BehaviorError> {
        let graph = self.graph.as_mut().ok_or_else(|| BehaviorError {
            message: "the loaded behavior has no editable graph; load one with load_graph or \
                      load_groot to edit it"
                .to_string(),
        })?;
        graph.apply(diff).map_err(|e| BehaviorError {
            message: format!("graph diff: {e}"),
        })?;
        self.tree = Some(
            build_behavior_tree(graph, &direct_resolver(ctx.store)).map_err(|e| BehaviorError {
                message: format!("rebuild after apply: {e:?}"),
            })?,
        );
        Ok(())
    }
}
