//! Dirty propagation and per-path status caches for incremental ticks.
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use arora_behavior::Status;

use crate::schema::{Expression, Node, _ON_CHANGE_PARAM_ID};
use crate::variable::VariableCell;
use crate::{is_native, nodes};

thread_local! {
    static WRITER: Cell<usize> = const { Cell::new(0) };
}

/// Restores the enclosing visitor when a child returns, including on errors.
pub(crate) struct Visit(usize);
impl Visit {
    pub(crate) fn detached() -> Self {
        Self(WRITER.with(|writer| writer.replace(0)))
    }
    pub(crate) fn enter(state: &Rc<State>) -> Self {
        Self(WRITER.with(|writer| writer.replace(Rc::as_ptr(state) as usize)))
    }
}
impl Drop for Visit {
    fn drop(&mut self) {
        WRITER.with(|writer| writer.set(self.0));
    }
}

pub(crate) struct State {
    pub(crate) dirty: Cell<bool>,
    pub(crate) status: Cell<Option<Status>>,
    pub(crate) parent: Option<Weak<State>>,
    pub(crate) cacheable: bool,
    pub(crate) pulls: bool,
}
impl State {
    pub(crate) fn mark(&self) {
        self.dirty.set(true);
        let mut next = self.parent.as_ref().and_then(Weak::upgrade);
        while let Some(parent) = next {
            if parent.dirty.replace(true) {
                break;
            }
            next = parent.parent.as_ref().and_then(Weak::upgrade);
        }
    }
    pub(crate) fn written(self: &Rc<Self>) {
        if !WRITER.with(|writer| writer.get() == Rc::as_ptr(self) as usize) {
            self.mark();
        }
    }
}

#[derive(Default)]
pub(crate) struct Incremental {
    pub(crate) enabled: Cell<bool>,
    pub(crate) states: RefCell<Vec<Weak<State>>>,
    pub(crate) stored: RefCell<Vec<VariableCell>>,
}
impl Incremental {
    pub(crate) fn set_enabled(&self, enabled: bool) {
        if self.enabled.replace(enabled) != enabled {
            for state in self.states.borrow().iter().filter_map(Weak::upgrade) {
                state.status.set(None);
                state.mark();
            }
        }
    }
    /// Observe writes performed through raw store slots (Scatter or a module)
    /// before a later sibling decides whether its subtree can be skipped.
    pub(crate) fn observe_writes(&self) {
        if self.enabled.get() {
            for cell in self.stored.borrow().iter() {
                cell.observe_written();
            }
        }
    }
    pub(crate) fn begin_tick(&self) {
        if !self.enabled.get() {
            return;
        }
        for cell in self.stored.borrow().iter() {
            cell.observe();
        }
        for state in self.states.borrow().iter().filter_map(Weak::upgrade) {
            // An unvisited child's initial dirtiness does not invalidate a
            // parent that already short-circuited before reaching it. It will
            // still have no cache if a later visit reaches it.
            if state.status.get() == Some(Status::Running)
                || (!state.cacheable && state.status.get().is_some())
            {
                state.mark();
            }
            if state.pulls && state.status.get().is_some() {
                if let Some(parent) = state.parent.as_ref().and_then(Weak::upgrade) {
                    parent.mark();
                }
            }
        }
    }
}

pub(crate) fn cacheable(node: &Node) -> bool {
    fn nested(expression: &Expression) -> bool {
        match expression {
            Expression::Call(_) => true,
            Expression::Select { source, .. } => nested(source),
            _ => false,
        }
    }
    if node.arguments.values().any(nested) {
        return false;
    }
    if is_native(node.function) {
        !matches!(
            node.function,
            nodes::RUN_CALL_FUNCTION_ID
                | nodes::SCATTER_FUNCTION_ID
                | nodes::SET_STR_FUNCTION_ID
                | nodes::UNSET_STR_FUNCTION_ID
        )
    } else {
        node.children.is_none()
            && matches!(
                node.arguments.get(&_ON_CHANGE_PARAM_ID),
                Some(Expression::Value(arora_types::value::Value::Boolean(true)))
            )
    }
}
