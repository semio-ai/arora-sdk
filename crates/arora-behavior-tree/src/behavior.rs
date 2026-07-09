//! [`BehaviorTreeInterpreter`]: the [`BehaviorInterpreter`] that runs an Arora
//! behavior tree.

use std::collections::HashMap;
use std::rc::Rc;

use arora_behavior::{BehaviorContext, BehaviorError, BehaviorInterpreter, BehaviorStatus};
use arora_types::data::{DataStore, Key};
use uuid::Uuid;

use crate::error::BehaviorTreeError;
use crate::{run_behavior_tree, schema_groot, tree_node::TreeNode, BehaviorTree, ModuleFunction};

/// The [`BehaviorInterpreter`] that runs a [`BehaviorTree`].
///
/// It is an executor, not a behavior: construct it **empty and ready** with
/// [`new`](Self::new) — it holds only the module-function index it needs to
/// resolve call nodes, no tree — then [`load`](Self::load) a behavior into it as
/// a separate step. It is never swapped; a behavior is loaded *into* it.
///
/// With a tree loaded, each tick runs the tree to a terminal status
/// (success/failure), so it reports [`BehaviorStatus::Done`] — the run-once
/// semantics the engine's queued trees already had. With **no** tree loaded it
/// idles: every tick is a no-op reporting [`BehaviorStatus::Running`], so the
/// interpreter stays installed (it is never dropped) waiting for a behavior.
pub struct BehaviorTreeInterpreter {
    tree: Option<BehaviorTree>,
    function_index: Rc<HashMap<Uuid, ModuleFunction>>,
}

impl BehaviorTreeInterpreter {
    /// Construct an empty, ready interpreter over the module-function index its
    /// call nodes resolve against. It holds no behavior until one is loaded with
    /// [`load`](Self::load) or [`load_groot`](Self::load_groot); until then its
    /// tick idles.
    pub fn new(function_index: Rc<HashMap<Uuid, ModuleFunction>>) -> Self {
        Self {
            tree: None,
            function_index,
        }
    }

    /// Load an already-built [`BehaviorTree`] into the interpreter, replacing any
    /// tree currently loaded.
    pub fn load(&mut self, behavior: BehaviorTree) {
        self.tree = Some(behavior);
    }

    /// Load a behavior tree from Groot XML, replacing any tree currently loaded.
    ///
    /// Parses the Groot XML, lowers it to a [`TreeNode`] (resolving call nodes
    /// through this interpreter's function index), and builds the runnable
    /// [`BehaviorTree`] with the **Direct convention**: each Groot `{var}` is
    /// bound to `store`'s slot under its own name (variable name == store key),
    /// so a behavior reading or writing `{var}` reads/writes the store directly
    /// during the tick. `store` must be the same store the device ticks against;
    /// it is only borrowed to resolve the slots — the tree keeps the slots, not
    /// the store.
    pub fn load_groot(
        &mut self,
        xml: &str,
        store: &dyn DataStore,
    ) -> Result<(), BehaviorTreeError> {
        let groot = schema_groot::BehaviorTree::try_from_groot_xml(xml)?;
        // `try_into_tree_node` fills `variables` as name → variable id.
        let mut variables = HashMap::new();
        let tree_node: TreeNode = groot
            .root
            .try_into_tree_node(self.function_index.as_ref(), &mut variables)?;
        // Invert to variable id → name for the BT builder.
        let id_to_name: HashMap<Uuid, String> =
            variables.into_iter().map(|(name, id)| (id, name)).collect();
        // Direct convention: a variable resolves to the store slot under its name.
        let resolver = move |name: &str| Some(store.slot(&Key::from(name)));
        let behavior: BehaviorTree = tree_node.into_behavior_tree(&resolver, &id_to_name)?;
        self.tree = Some(behavior);
        Ok(())
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
}
