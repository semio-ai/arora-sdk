//! What waits to be published: each key once, with its newest value.
//!
//! A step's change arrives shared (`Arc<StateChange>`), and a key's value is
//! held in it rather than copied, so a frame waits without a copy. A change
//! is held while one of its keys has its newest value there. Once what it
//! still holds weighs less than what later changes replaced, those values
//! are copied out and the change is let go: a small key changed beside each
//! frame never keeps the frames alive, and what waits weighs at most about
//! twice the newest values. Accepting a change costs its own keys, and once,
//! when it is let go, the older change's: a change is let go once, and what
//! it copies weighs less than what replaced it, so the work stays linear in
//! what was pushed.
//!
//! A key a change both sets and unsets is unset, as the runtime applies it.

use std::collections::HashMap;
use std::sync::Arc;

use arora_types::data::{Key, StateChange};
use arora_types::value::Value;

/// Where a waiting key's newest value is.
pub(crate) enum Held {
    /// In a shared change: its value there, or unset when the change unsets
    /// it (unset winning over set).
    Shared(Arc<StateChange>),
    /// Copied out of a change let go: `Some` sets the key, `None` unsets it.
    Own(Option<Option<Value>>),
}

impl Held {
    /// The value the key takes: `Some` to set it, `None` to unset it.
    pub(crate) fn value<'a>(&'a self, key: &Key) -> Option<&'a Option<Value>> {
        match self {
            Held::Shared(change) if change.unset.contains(key) => None,
            Held::Shared(change) => change.set.get(key),
            Held::Own(value) => value.as_ref(),
        }
    }
}

#[derive(Default)]
pub(crate) struct Pending {
    next: u64,
    /// Each waiting key: when its newest change came, and where it is.
    keys: HashMap<Key, (u64, Held)>,
    /// The shared changes `keys` holds, by address: the weight of their keys
    /// still newest, and of those later changes replaced.
    shared: HashMap<usize, Weights>,
}

#[derive(Default)]
struct Weights {
    newest: usize,
    replaced: usize,
}

fn address(change: &Arc<StateChange>) -> usize {
    Arc::as_ptr(change) as usize
}

impl Pending {
    pub(crate) fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Wait to publish `change`'s keys, replacing what waits for them.
    pub(crate) fn push(&mut self, change: &Arc<StateChange>) {
        if change.set.is_empty() && change.unset.is_empty() {
            return;
        }
        let at = self.next;
        self.next += 1;
        let mut weights = Weights::default();
        let mut let_go = Vec::new();
        for key in keys(change) {
            weights.newest += key_weight(change, key);
            let held = (at, Held::Shared(change.clone()));
            if let Some((_, Held::Shared(older))) = self.keys.insert(key.clone(), held) {
                if Arc::ptr_eq(&older, change) {
                    continue;
                }
                let replaced = key_weight(&older, key);
                if let Some(weights) = self.shared.get_mut(&address(&older)) {
                    weights.newest -= replaced;
                    weights.replaced += replaced;
                    if weights.newest < weights.replaced {
                        let_go.push(older);
                    }
                }
            }
        }
        *self.shared.entry(address(change)).or_default() = weights;
        for older in let_go {
            self.let_go(&older);
        }
    }

    /// Copy out of `change` the values still newest there, and drop it.
    fn let_go(&mut self, change: &Arc<StateChange>) {
        if self.shared.remove(&address(change)).is_none() {
            return;
        }
        for key in keys(change) {
            if let Some((_, held)) = self.keys.get_mut(key) {
                if matches!(held, Held::Shared(newest) if Arc::ptr_eq(newest, change)) {
                    *held = Held::Own(held.value(key).cloned());
                }
            }
        }
    }

    /// Everything waiting, in the order the keys' newest changes came.
    pub(crate) fn take(&mut self) -> Vec<(Key, Held)> {
        self.shared.clear();
        let mut taken: Vec<_> = self.keys.drain().collect();
        taken.sort_by_key(|(_, (at, _))| *at);
        taken
            .into_iter()
            .map(|(key, (_, held))| (key, held))
            .collect()
    }
}

/// The keys `change` names, each once.
fn keys(change: &StateChange) -> impl Iterator<Item = &Key> {
    change
        .set
        .keys()
        .filter(|key| !change.unset.contains(*key))
        .chain(&change.unset)
}

/// What `key` takes in `change`: its path, and its value if set.
fn key_weight(change: &StateChange, key: &Key) -> usize {
    key.path.len()
        + match change.set.get(key) {
            Some(Some(value)) if !change.unset.contains(key) => weight(value),
            _ => 0,
        }
}

/// About the bytes `value` holds.
fn weight(value: &Value) -> usize {
    const HEADER: usize = 16;
    HEADER
        + match value {
            Value::String(text) => text.len(),
            Value::Option(Some(inner)) => weight(inner),
            Value::Structure(structure) => fields(&structure.fields),
            Value::Enumeration(enumeration) => weight(&enumeration.value),
            Value::ArrayBoolean(items) => items.len(),
            Value::ArrayU8(items) => items.len(),
            Value::ArrayI8(items) => items.len(),
            Value::ArrayU16(items) => 2 * items.len(),
            Value::ArrayI16(items) => 2 * items.len(),
            Value::ArrayU32(items) => 4 * items.len(),
            Value::ArrayI32(items) => 4 * items.len(),
            Value::ArrayF32(items) => 4 * items.len(),
            Value::ArrayU64(items) => 8 * items.len(),
            Value::ArrayI64(items) => 8 * items.len(),
            Value::ArrayF64(items) => 8 * items.len(),
            Value::ArrayString(items) => items.iter().map(|text| HEADER + text.len()).sum(),
            Value::ArrayValue(items) => items.iter().map(weight).sum(),
            Value::ArrayStructure { elements, .. } => elements
                .iter()
                .map(|element| HEADER + fields(&element.fields))
                .sum(),
            Value::ArrayEnumeration { elements, .. } => elements
                .iter()
                .map(|element| HEADER + weight(&element.value))
                .sum(),
            Value::KeyValue(map) => map
                .fields
                .iter()
                .map(|(name, field)| HEADER + name.len() + field.value.as_deref().map_or(0, weight))
                .sum(),
            _ => 0,
        }
}

fn fields(fields: &[arora_types::value::StructureField]) -> usize {
    fields.iter().map(|field| weight(&field.value)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn published(pending: &mut Pending) -> Vec<(String, Option<Option<Value>>)> {
        pending
            .take()
            .iter()
            .map(|(key, held)| (key.path.clone(), held.value(key).cloned()))
            .collect()
    }

    /// Each key goes out once, with its newest value or as unset, in the
    /// order its newest change came; a key no later change names keeps its
    /// own.
    #[test]
    fn what_waits_goes_out_newest_value_only() {
        let mut pending = Pending::default();
        let mut first = StateChange::set("camera/frame", Value::ArrayU8(vec![1; 4]));
        first
            .set
            .insert(Key::from("dcm/stamp"), Some(Value::U32(1)));
        first
            .set
            .insert(Key::from("arm/target"), Some(Value::F64(0.5)));
        pending.push(&Arc::new(first));
        let mut second = StateChange::set("camera/frame", Value::ArrayU8(vec![2; 4]));
        second
            .set
            .insert(Key::from("dcm/stamp"), Some(Value::U32(2)));
        pending.push(&Arc::new(second));
        let mut third = StateChange::new();
        third.unset.insert(Key::from("dcm/stamp"));
        third.set.insert(Key::from("arm/hint"), None);
        pending.push(&Arc::new(third));

        let mut out = published(&mut pending);
        assert_eq!(out[0].0, "arm/target", "the oldest newest change first");
        out.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            out,
            [
                ("arm/hint".to_string(), Some(None)),
                ("arm/target".to_string(), Some(Some(Value::F64(0.5)))),
                (
                    "camera/frame".to_string(),
                    Some(Some(Value::ArrayU8(vec![2; 4])))
                ),
                ("dcm/stamp".to_string(), None),
            ]
        );
        assert!(pending.is_empty());
    }

    /// While nothing is published, frames that each come beside a small key
    /// of their own leave one frame held: the small keys are copied out, and
    /// the replaced frames let go.
    #[test]
    fn small_keys_do_not_keep_replaced_frames() {
        let mut pending = Pending::default();
        let mut frames = Vec::new();
        for i in 0..64u32 {
            let mut change =
                StateChange::set("camera/frame", Value::ArrayU8(vec![i as u8; 1 << 16]));
            change
                .set
                .insert(Key::from(format!("status/{i}")), Some(Value::U32(i)));
            let change = Arc::new(change);
            frames.push(Arc::downgrade(&change));
            pending.push(&change);
        }
        let alive = frames
            .iter()
            .filter(|frame| frame.strong_count() > 0)
            .count();
        assert_eq!(alive, 1, "only the newest frame is held");
        let out = published(&mut pending);
        assert_eq!(out.len(), 65);
        assert!(out.contains(&(
            "camera/frame".to_string(),
            Some(Some(Value::ArrayU8(vec![63; 1 << 16])))
        )));
        assert!(out.contains(&("status/0".to_string(), Some(Some(Value::U32(0))))));
    }

    /// A frame beside a small key replaced later stays shared: its value is
    /// never copied while it outweighs what was replaced.
    #[test]
    fn a_newest_frame_is_not_copied() {
        let mut pending = Pending::default();
        let mut change = StateChange::set("camera/frame", Value::ArrayU8(vec![7; 1 << 16]));
        change
            .set
            .insert(Key::from("dcm/stamp"), Some(Value::U32(1)));
        let change = Arc::new(change);
        pending.push(&change);
        pending.push(&Arc::new(StateChange::set("dcm/stamp", Value::U32(2))));
        let held = pending.take();
        let frame = held
            .iter()
            .find(|(key, _)| key.path == "camera/frame")
            .expect("the frame waits");
        assert!(matches!(&frame.1, Held::Shared(shared) if Arc::ptr_eq(shared, &change)));
    }

    /// A key a change both sets and unsets is unset, whether its change is
    /// held shared or let go.
    #[test]
    fn unset_wins_over_set_in_one_change() {
        let mut both = StateChange::set("arm/target", Value::F64(1.0));
        both.unset.insert(Key::from("arm/target"));
        both.set.insert(
            Key::from("camera/frame"),
            Some(Value::ArrayU8(vec![1; 1 << 12])),
        );
        let both = Arc::new(both);

        let mut pending = Pending::default();
        pending.push(&both);
        assert!(published(&mut pending).contains(&("arm/target".to_string(), None)));

        // Replacing the frame lets the change go: the key is copied out unset.
        pending.push(&both);
        pending.push(&Arc::new(StateChange::set(
            "camera/frame",
            Value::ArrayU8(vec![2; 1 << 12]),
        )));
        let out = published(&mut pending);
        assert!(out.contains(&("arm/target".to_string(), None)), "{out:?}");
    }
}
