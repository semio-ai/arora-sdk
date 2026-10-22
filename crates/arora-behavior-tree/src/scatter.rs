//! The `Scatter` data node
//! ([`SCATTER_FUNCTION_ID`](crate::nodes::SCATTER_FUNCTION_ID)): an array
//! written to the store keys another array names, under a key prefix fixed at
//! lowering or anywhere in the tree's namespace.
//!
//! The keys are data, so the node resolves them while the tree ticks, through
//! the resolver the tree was given ([`BehaviorTree::with_key_resolver`]), and
//! keeps the slots they resolved to until other keys arrive.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use arora_behavior::Status;
use arora_types::data::{prefix_covers, Slot};
use arora_types::value::Value;
use uuid::Uuid;

use crate::error::BehaviorTreeError;
use crate::nodes::SCATTER_WITHIN_PARAM_ID;
use crate::schema::{Expression, Node};
use crate::variable::{is_tree_local, VariableResolver};
use crate::{BehaviorTree, Outputs};

/// A `Scatter` node's binding: the prefix its keys are under, and the store
/// slots its last keys resolved to.
pub(crate) struct Scatter {
    node: Uuid,
    /// The key prefix; empty for the tree's whole namespace.
    within: String,
    /// Resolves a key to its store slot while the tree ticks; `None` until the
    /// tree is given a resolver.
    resolver: Option<Rc<VariableResolver<'static>>>,
    bound: RefCell<Bound>,
}

/// The keys a tick last resolved, and their slots, in order.
#[derive(Default)]
struct Bound {
    keys: Option<Value>,
    slots: Vec<Box<dyn Slot>>,
}

impl Scatter {
    /// Bind the `Scatter` node `node`. Its `within`, if linked, is a literal
    /// string. Empty or left out, it is the tree's whole namespace; otherwise
    /// it has no trailing `/` and is not tree-local: a prefix that covers store
    /// keys only.
    pub(crate) fn bind(node: &Node) -> Result<Self, BehaviorTreeError> {
        let refuse = |what: String| BehaviorTreeError::InconsistentTreeError {
            message: format!("Scatter node {}: {what}", node.id),
        };
        let within = match node.arguments.get(&SCATTER_WITHIN_PARAM_ID) {
            None => "",
            Some(Expression::Value(Value::String(within))) => within.as_str(),
            Some(other) => {
                return Err(refuse(format!(
                    "`within` is a key prefix fixed at lowering, a literal string, not {other:?}"
                )))
            }
        };
        if !within.is_empty() && (within.ends_with('/') || is_tree_local(within)) {
            return Err(refuse(format!(
                "`within` '{within}' is not a key prefix: it ends with '/' or is tree-local"
            )));
        }
        Ok(Scatter {
            node: node.id,
            within: within.to_string(),
            resolver: None,
            bound: RefCell::default(),
        })
    }

    /// Write element `i` of `values` to the key named by element `i` of `keys`
    /// (see [`SCATTER_FUNCTION_ID`](crate::nodes::SCATTER_FUNCTION_ID)):
    /// every check passes before the first write.
    pub(crate) fn write(&self, keys: Value, values: Value) -> Result<Status, BehaviorTreeError> {
        let Ok(values) = values.into_elements() else {
            return Ok(Status::Failure);
        };
        let mut bound = self.bound.borrow_mut();
        if bound.keys.as_ref() != Some(&keys) {
            let Some(names) = key_names(&keys) else {
                return Ok(Status::Failure);
            };
            // A table that does not match yet fails before its keys resolve,
            // so a guard can wait it out whatever those keys name.
            if names.len() != values.len() {
                return Ok(Status::Failure);
            }
            *bound = Bound {
                slots: self.resolve(&names)?,
                keys: Some(keys),
            };
        }
        if bound.slots.len() != values.len() {
            return Ok(Status::Failure);
        }
        // A slot notifies a change only when the value moves (the `Slot`
        // contract), so a held value costs none.
        for (slot, value) in bound.slots.iter().zip(values) {
            if value != Value::Unit {
                let _ = slot.set(Some(value));
            }
        }
        Ok(Status::Success)
    }

    /// The slots of `names`, in order, once every name is checked: not
    /// tree-local, under `within`, and named once.
    fn resolve(&self, names: &[String]) -> Result<Vec<Box<dyn Slot>>, BehaviorTreeError> {
        let refuse = |what: String| BehaviorTreeError::InconsistentTreeError {
            message: format!("Scatter node {}: {what}", self.node),
        };
        let mut named = HashSet::with_capacity(names.len());
        for name in names {
            if is_tree_local(name) {
                return Err(refuse(format!(
                    "the key '{name}' is tree-local, not a store key"
                )));
            }
            if !prefix_covers(&self.within, name) {
                return Err(refuse(format!(
                    "the key '{name}' is not within '{}'",
                    self.within
                )));
            }
            if !named.insert(name) {
                return Err(refuse(format!("the key '{name}' is named twice")));
            }
        }
        let resolver = self
            .resolver
            .as_ref()
            .ok_or_else(|| refuse("the tree has no store to resolve keys in".to_string()))?;
        names
            .iter()
            .map(|name| {
                resolver(name).ok_or_else(|| refuse(format!("the key '{name}' has no store slot")))
            })
            .collect()
    }
}

/// The key names `keys` holds: `None` unless it is an array of strings.
fn key_names(keys: &Value) -> Option<Vec<String>> {
    match keys {
        Value::ArrayString(names) => Some(names.clone()),
        Value::ArrayValue(elements) => elements
            .iter()
            .map(|element| match element {
                Value::String(name) => Some(name.clone()),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

impl BehaviorTree {
    /// The tree whose `Scatter` nodes
    /// ([`SCATTER_FUNCTION_ID`](crate::nodes::SCATTER_FUNCTION_ID)) resolve
    /// the keys they are handed through `resolver` while the tree ticks: under
    /// the Direct convention, the store slot named by the key. A tree built
    /// without it runs, but its `Scatter` nodes error when they write.
    pub fn with_key_resolver(mut self, resolver: Rc<VariableResolver<'static>>) -> Self {
        for outputs in self.outputs.values_mut() {
            if let Outputs::Keys(scatter) = outputs {
                *scatter = Rc::new(Scatter {
                    node: scatter.node,
                    within: scatter.within.clone(),
                    resolver: Some(resolver.clone()),
                    bound: RefCell::default(),
                });
            }
        }
        self
    }
}

#[cfg(test)]
mod tests;
