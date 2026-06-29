//! The behavior-tree variable cell — where a `{var}` lives during a tick.
//!
//! A variable is either a **tree-local** scratch cell or a **handle into the
//! host data store**, resolved once at build time via a [`VariableResolver`].
//! The store handle is the `arora_types` [`Slot`] trait, so this crate stays
//! agnostic of any concrete store — the runtime supplies the resolver. Values
//! are `Option<Value>` because a store key may legitimately be absent.

use std::cell::RefCell;
use std::rc::Rc;

use arora_types::data::Slot;
use arora_types::value::Value;

/// A behavior-tree variable cell. Cloning shares the same underlying cell, so
/// two nodes bound to the same `{var}` read and write one place.
#[derive(Clone)]
pub enum VariableCell {
    /// A tree-local scratch cell (no store backing).
    Local(Rc<RefCell<Option<Value>>>),
    /// A handle into the host data store, resolved once at build.
    Stored(Rc<dyn Slot>),
}

impl std::fmt::Debug for VariableCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `dyn Slot` is not `Debug`; show the current value instead, which is all
        // the schema's `#[derive(Debug)]` on `Expression::Variable` ever needs.
        f.debug_tuple("VariableCell").field(&self.get()).finish()
    }
}

impl PartialEq for VariableCell {
    /// Cells compare by current value, mirroring the value comparison the schema
    /// relied on when this was a `Rc<RefCell<Value>>` (`RefCell` compares contents).
    fn eq(&self, other: &Self) -> bool {
        self.get() == other.get()
    }
}

impl VariableCell {
    /// An absent local cell.
    pub fn local() -> Self {
        VariableCell::Local(Rc::new(RefCell::new(None)))
    }

    /// A local cell holding `value`.
    pub fn local_with(value: Value) -> Self {
        VariableCell::Local(Rc::new(RefCell::new(Some(value))))
    }

    /// Wrap a store slot handed back by a [`VariableResolver`].
    pub fn stored(slot: Box<dyn Slot>) -> Self {
        VariableCell::Stored(Rc::from(slot))
    }

    /// The current value, or `None` if absent (unset local cell / missing key).
    pub fn get(&self) -> Option<Value> {
        match self {
            VariableCell::Local(cell) => cell.borrow().clone(),
            VariableCell::Stored(slot) => slot.get(),
        }
    }

    /// The current value, treating "absent" as [`Value::Unit`] — the common case
    /// for behavior-tree reads that expect a value to be present.
    pub fn get_or_unit(&self) -> Value {
        self.get().unwrap_or(Value::Unit)
    }

    /// Set the value.
    pub fn set(&self, value: Value) {
        self.set_opt(Some(value));
    }

    /// Set (or clear, with `None`) the value.
    pub fn set_opt(&self, value: Option<Value>) {
        match self {
            VariableCell::Local(cell) => *cell.borrow_mut() = value,
            // A failed store write surfaces via the change feed (the runtime's
            // STEP 3 is where it is observed), so we don't propagate it here and
            // keep the tick signature unchanged. Revisit if write failures must
            // fail a tick.
            VariableCell::Stored(slot) => {
                let _ = slot.set(value);
            }
        }
    }
}

/// Resolves a `{var}` name to a store-backed cell, or `None` for a tree-local
/// variable. Supplied by the host (the runtime backs it with the data store), so
/// the behavior-tree crate never depends on a concrete `DataStore`.
pub type VariableResolver<'a> = dyn Fn(&str) -> Option<Box<dyn Slot>> + 'a;
