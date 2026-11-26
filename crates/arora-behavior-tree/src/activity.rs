//! Which nodes are active, and halting the ones a tick left.
//!
//! The root is ticked at every tick: it is always active, and the
//! interpreter's root, the runner, keeps each run going whatever the others
//! return. Any other node is *active* after a tick when it returned
//! `Running` and its parent is active. A node active after the last tick
//! that is not active after this one, or that returned `Running` this tick
//! and is not active, is *halted*: its parent stopped ticking it while it ran
//! (a fallback whose earlier child now succeeds, a sequence whose earlier
//! child now fails), or finished with it still running (a parallel that
//! failed, a retry whose child failed). A halted node's subtree is halted
//! with it, none of it being active. Halting resets the state a native holds for an activation: a
//! `sequence_star` starts again at its first child, and a `retry` has made no
//! attempt.
//!
//! A node's first tick of an activation is its first tick while it was not
//! active after the last tick: a module function declaring the parameter
//! `activated: bool`, which the node does not bind, is told so through it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use arora_behavior::Status;
use uuid::Uuid;

use crate::id_hash::{IdMap, IdSet};
use crate::schema::Node;

/// The nodes active after the last tick, and what this tick did.
#[derive(Default)]
pub(crate) struct Activity {
    /// The nodes active after the last tick.
    active: RefCell<IdSet<Uuid>>,
    /// Each node this tick reached, with the status it last returned: a node
    /// whose tick failed returned `Failure`.
    ticked: RefCell<IdMap<Uuid, Status>>,
}

impl Activity {
    /// Whether `node` was active after the last tick.
    pub(crate) fn was_active(&self, node: Uuid) -> bool {
        self.active.borrow().contains(&node)
    }

    /// `node` returned `status` this tick.
    pub(crate) fn ticked(&self, node: Uuid, status: Status) {
        self.ticked.borrow_mut().insert(node, status);
    }

    /// End the tick of the tree rooted at `root`: the nodes active from now
    /// on, and the ones halted, which the caller resets.
    pub(crate) fn end_tick(&self, root: &Node, nodes: &HashMap<Uuid, Rc<Node>>) -> Vec<Uuid> {
        let ticked = std::mem::take(&mut *self.ticked.borrow_mut());
        let running = |id: &Uuid| ticked.get(id) == Some(&Status::Running);
        let mut active = IdSet::default();
        let mut stack = Vec::new();
        if ticked.contains_key(&root.id) {
            stack.push(root);
        }
        while let Some(node) = stack.pop() {
            active.insert(node.id);
            for child in node.children.iter().flatten() {
                if let Some(child) = nodes.get(child).filter(|child| running(&child.id)) {
                    stack.push(child);
                }
            }
        }
        let previous = std::mem::replace(&mut *self.active.borrow_mut(), active);
        let active = self.active.borrow();
        let mut halted: Vec<Uuid> = previous
            .into_iter()
            .chain(ticked.keys().copied().filter(|id| running(id)))
            .filter(|id| !active.contains(id))
            .collect();
        halted.sort_unstable();
        halted.dedup();
        halted
    }

    /// Take over which of `kept` were active in `previous`: a tree built
    /// again from a changed graph goes on with the nodes the change left in
    /// place.
    pub(crate) fn keep(&self, previous: &Activity, kept: impl Fn(&Uuid) -> bool) {
        *self.active.borrow_mut() = previous
            .active
            .borrow()
            .iter()
            .copied()
            .filter(|id| kept(id))
            .collect();
    }
}
