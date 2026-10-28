//! The behavior-tree variable cell — where a `{var}` lives during a tick.
//!
//! A variable is either a **tree-local** scratch cell or a **handle into the
//! host data store**, resolved once at build time via a [`VariableResolver`].
//! The store handle is the `arora_types` [`Slot`] trait, so this crate stays
//! agnostic of any concrete store — the runtime supplies the resolver. Values
//! are `Option<Value>` because a store key may legitimately be absent.

use crate::incremental::State;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use arora_types::data::Slot;
use arora_types::value::Value;

/// A behavior-tree variable cell. Cloning shares the same underlying cell, so
/// two nodes bound to the same `{var}` read and write one place. A version
/// advances when an observed value changes, notifying every attached reader.
/// Store handles are read live; incremental ticks also observe outside writes.
#[derive(Clone)]
pub enum VariableCell {
    /// A tree-local scratch cell (no store backing).
    Local(Rc<LocalCell>),
    /// A handle into the host data store, resolved once at build.
    Stored(Rc<StoredCell>),
}

/// Shared local storage and its change metadata. Writes go through `VariableCell`.
pub struct LocalCell {
    value: RefCell<Option<Value>>,
    changes: Changes,
}
/// Shared store handle and the last value observed by this tree.
pub struct StoredCell {
    slot: Box<dyn Slot>,
    changes: Changes,
}
#[derive(Default)]
struct Changes {
    version: Cell<u64>,
    seen: RefCell<Option<Value>>,
    readers: RefCell<Vec<Weak<State>>>,
}
impl Changes {
    fn observe(&self, value: Option<Value>) {
        if self.readers.borrow().is_empty() {
            return;
        }
        if *self.seen.borrow() == value {
            return;
        }
        *self.seen.borrow_mut() = value;
        self.version.set(self.version.get().wrapping_add(1));
        self.readers.borrow_mut().retain(|reader| {
            if let Some(reader) = reader.upgrade() {
                reader.written();
                true
            } else {
                false
            }
        });
    }
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
        Self::Local(Rc::new(LocalCell {
            value: RefCell::new(None),
            changes: Changes::default(),
        }))
    }

    /// A local cell holding `value`.
    pub fn local_with(value: Value) -> Self {
        Self::Local(Rc::new(LocalCell {
            value: RefCell::new(Some(value)),
            changes: Changes::default(),
        }))
    }

    /// Wrap a store slot handed back by a [`VariableResolver`].
    pub fn stored(slot: Box<dyn Slot>) -> Self {
        Self::Stored(Rc::new(StoredCell {
            changes: Changes::default(),
            slot,
        }))
    }

    /// The current value, or `None` if absent (unset local cell / missing key).
    pub fn get(&self) -> Option<Value> {
        match self {
            VariableCell::Local(cell) => cell.value.borrow().clone(),
            VariableCell::Stored(slot) => slot.slot.get(),
        }
    }

    /// Run `f` on the current value, `None` if absent: borrowed in place for
    /// a local cell, copied out of a store slot.
    pub fn with<R>(&self, f: impl FnOnce(Option<&Value>) -> R) -> R {
        match self {
            VariableCell::Local(cell) => f(cell.value.borrow().as_ref()),
            VariableCell::Stored(slot) => f(slot.slot.get().as_ref()),
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
        // A store can move without this handle seeing it. Observe that move
        // before the write, so overwriting it cannot hide a value change.
        let observed = !self.changes().readers.borrow().is_empty();
        if observed && matches!(self, Self::Stored(_)) {
            self.observe();
        }
        match self {
            VariableCell::Local(cell) => {
                *cell.value.borrow_mut() = value;
            }
            // A failed store write surfaces via the change feed (the runtime's
            // STEP 3 is where it is observed), so we don't propagate it here and
            // keep the tick signature unchanged. Revisit if write failures must
            // fail a tick.
            VariableCell::Stored(slot) => {
                crate::incremental::store_written();
                let _ = slot.slot.set(value);
            }
        }
        if observed {
            self.changes().observe(self.get());
        }
    }

    /// Number of value changes observed while subscribed. Equal writes keep it unchanged.
    pub fn version(&self) -> u64 {
        self.changes().version.get()
    }

    fn changes(&self) -> &Changes {
        match self {
            Self::Local(cell) => &cell.changes,
            Self::Stored(cell) => &cell.changes,
        }
    }
    pub(crate) fn observe(&self) {
        let _external = crate::incremental::Visit::detached();
        self.changes().observe(self.get());
    }
    pub(crate) fn subscribe(&self, reader: &Rc<State>) {
        let changes = self.changes();
        let mut readers = changes.readers.borrow_mut();
        readers.retain(|reader| reader.strong_count() > 0);
        if readers.is_empty() {
            *changes.seen.borrow_mut() = self.get();
        }
        if !readers
            .iter()
            .any(|held| held.ptr_eq(&Rc::downgrade(reader)))
        {
            readers.push(Rc::downgrade(reader));
        }
    }
    pub(crate) fn unsubscribe(&self, reader: &Rc<State>) {
        let mut readers = self.changes().readers.borrow_mut();
        readers.retain(|held| held.strong_count() > 0 && !held.ptr_eq(&Rc::downgrade(reader)));
        if readers.is_empty() {
            *self.changes().seen.borrow_mut() = None;
        }
    }
    pub(crate) fn identity(&self) -> usize {
        match self {
            Self::Local(cell) => Rc::as_ptr(cell) as usize,
            Self::Stored(cell) => Rc::as_ptr(cell) as usize,
        }
    }
}

/// Resolves a `{var}` name to a store-backed cell, or `None` for a tree-local
/// variable. Supplied by the host (the runtime backs it with the data store), so
/// the behavior-tree crate never depends on a concrete `DataStore`. It is never
/// asked for a tree-local name (see [`is_tree_local`]).
pub type VariableResolver<'a> = dyn Fn(&str) -> Option<Box<dyn Slot>> + 'a;

/// The prefix of a tree-local variable's name.
pub(crate) const TREE_LOCAL_PREFIX: char = '.';

/// Whether a variable named `name` is tree-local: a name that starts with `.`.
/// Such a variable is one cell of the lowered tree that holds it, never a
/// store key: the resolver is not asked for it, so it has no store slot,
/// reaches no observer, and is in no write set. A store key whose path starts
/// with `.` is therefore out of a tree's reach.
pub fn is_tree_local(name: &str) -> bool {
    name.starts_with(TREE_LOCAL_PREFIX)
}
