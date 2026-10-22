//! Dirty propagation and per-path status caches for incremental ticks.
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use arora_behavior::Status;

use crate::schema::{Expression, Node, _ON_CHANGE_PARAM_ID};
use crate::variable::VariableCell;
use crate::{is_native, nodes};

thread_local! {
    static WRITER: Cell<usize> = const { Cell::new(0) };
    static STORE_MOVED: Cell<bool> = const { Cell::new(false) };
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

/// Tracks stored-cell writes, including detached publication, for this tick.
pub(crate) struct StoreVisit(Option<bool>);
impl Drop for StoreVisit {
    fn drop(&mut self) {
        if let Some(previous) = self.0 {
            // A nested tree's store writes also affect its enclosing visitor.
            STORE_MOVED.with(|moved| moved.set(previous || moved.get()));
        }
    }
}
pub(crate) fn store_written() {
    STORE_MOVED.with(|moved| moved.set(true));
}
pub(crate) fn store_moved() -> bool {
    STORE_MOVED.with(Cell::get)
}

pub(crate) struct State {
    pub(crate) dirty: Cell<bool>,
    pub(crate) status: Cell<Option<Status>>,
    pub(crate) parent: Option<Weak<State>>,
    pub(crate) cacheable: bool,
    pub(crate) pulls: bool,
    pub(crate) reads: Vec<VariableCell>,
    pub(crate) arguments: Vec<VariableCell>,
    pub(crate) subtree_stored: RefCell<Vec<VariableCell>>,
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
    polled: RefCell<Vec<Weak<State>>>,
    /// Visits that ran a node rather than answering from its cache: what the
    /// tests count to see which nodes a tick reached.
    #[cfg(test)]
    pub(crate) executed: Cell<u64>,
}
impl Incremental {
    pub(crate) fn set_enabled(&self, enabled: bool) {
        if self.enabled.replace(enabled) != enabled {
            self.polled.borrow_mut().clear();
            for state in self.states.borrow().iter().filter_map(Weak::upgrade) {
                for cell in &state.reads {
                    if enabled {
                        cell.subscribe(&state);
                    } else {
                        cell.unsubscribe(&state);
                    }
                }
                state.status.set(None);
                state.mark();
            }
        }
    }
    pub(crate) fn visited(&self, state: &Rc<State>) {
        if state.status.get() == Some(Status::Running) || !state.cacheable || state.pulls {
            self.polled.borrow_mut().push(Rc::downgrade(state));
        }
    }
    pub(crate) fn begin_tick(&self) -> StoreVisit {
        if !self.enabled.get() {
            return StoreVisit(None);
        }
        let previous = STORE_MOVED.with(|moved| moved.replace(false));
        for cell in self.stored.borrow().iter() {
            cell.observe();
        }
        for state in self
            .polled
            .take()
            .into_iter()
            .filter_map(|state| state.upgrade())
        {
            // A Running child bypassed by its parent drops out after that tick.
            if state.status.get() == Some(Status::Running) || !state.cacheable {
                state.mark();
            }
            if state.pulls {
                if let Some(parent) = state.parent.as_ref().and_then(Weak::upgrade) {
                    parent.mark();
                }
            }
        }
        StoreVisit(Some(previous))
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
