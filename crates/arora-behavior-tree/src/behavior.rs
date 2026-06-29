//! [`TreeBehavior`]: an Arora behavior tree as a runtime
//! [`Behavior`](arora_types::behavior::Behavior).

use std::collections::HashMap;
use std::rc::Rc;

use arora_behavior::{Behavior, BehaviorContext, BehaviorError, BehaviorStatus};
use uuid::Uuid;

use crate::{run_behavior_tree, BehaviorTree, ModuleFunction};

/// Runs a [`BehaviorTree`] as a runtime [`Behavior`].
///
/// Each tick runs the tree to a terminal status (success/failure), so it always
/// reports [`BehaviorStatus::Done`] — the run-once semantics the engine's queued
/// trees already had. A continuously-ticked interpreter (e.g. a node graph)
/// instead returns [`BehaviorStatus::Running`].
pub struct TreeBehavior {
    tree: BehaviorTree,
    function_index: Rc<HashMap<Uuid, ModuleFunction>>,
}

impl TreeBehavior {
    /// Wrap a built tree with the module-function index it calls into.
    pub fn new(tree: BehaviorTree, function_index: Rc<HashMap<Uuid, ModuleFunction>>) -> Self {
        Self {
            tree,
            function_index,
        }
    }
}

impl Behavior for TreeBehavior {
    fn tick(&mut self, ctx: &mut BehaviorContext) -> Result<BehaviorStatus, BehaviorError> {
        run_behavior_tree(&self.tree, self.function_index.clone(), ctx.caller, false).map_err(
            |e| BehaviorError {
                message: format!("behavior tree: {e:?}"),
            },
        )?;
        Ok(BehaviorStatus::Done)
    }
}
