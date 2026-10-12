//! The HALs' target keys, read by [`set_targets`] and folded by
//! [`fold_newest_wins`].

use std::collections::{HashMap, HashSet};

use arora_types::data::{Key, StateChange};
use arora_types::value::Value;

/// The entity name that makes a key's last segment a target (`k/target.c`).
const TARGET_ENTITY: &str = "target";

/// The prefix that makes a component a target (`k.target_c`).
const TARGET_PREFIX: &str = "target_";

/// A key that names a target, read: the measured key it targets, and whether
/// it is the `k/target.c` form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    measured: Key,
    entity_form: bool,
}

impl Target {
    /// Read `key` as a target, in either form; `None` for any other key. The
    /// grammar is [`set_targets`]'s.
    pub(crate) fn parse(key: &Key) -> Option<Self> {
        let (namespace, last) = match key.get_path().rsplit_once('/') {
            Some((namespace, last)) => (Some(namespace), last),
            None => (None, key.get_path()),
        };
        if namespace.is_some_and(|namespace| namespace.contains('.')) {
            return None;
        }
        let (entity, attributes) = last.split_once('.')?;
        let (component, rest) = match attributes.split_once('.') {
            Some((component, rest)) => (component, Some(rest)),
            None => (attributes, None),
        };
        if component.is_empty() {
            return None;
        }
        let measured = |entity: &str, component: &str| {
            Key::new(match rest {
                Some(rest) => format!("{entity}.{component}.{rest}"),
                None => format!("{entity}.{component}"),
            })
        };
        if let Some(namespace) = namespace.filter(|_| entity == TARGET_ENTITY) {
            let targeted = namespace.rsplit('/').next().unwrap_or(namespace);
            if targeted.is_empty() {
                return None;
            }
            return Some(Target {
                measured: measured(namespace, component),
                entity_form: true,
            });
        }
        let component = component.strip_prefix(TARGET_PREFIX)?;
        if component.is_empty() {
            return None;
        }
        let entity = match namespace {
            Some(namespace) => format!("{namespace}/{entity}"),
            None => entity.to_string(),
        };
        Some(Target {
            measured: measured(&entity, component),
            entity_form: false,
        })
    }

    /// The key of the measured value this target is for.
    pub(crate) fn measured(&self) -> &Key {
        &self.measured
    }
}

/// A target set by a change: the key it was written under, the measured key it
/// targets, and the value written. Read from a change by [`set_targets`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SetTarget<'c> {
    /// The key as written, in either form (`hip_yaw/target.position` or
    /// `hip_yaw.target_position`).
    pub key: &'c Key,
    /// The key of the measured value (`hip_yaw.position`).
    pub measured: Key,
    /// The value written; `None` for a key set with no value.
    pub value: &'c Option<Value>,
}

impl SetTarget<'_> {
    /// The targeted component: `position` for `hip_yaw/target.position` and
    /// `hip_yaw.target_position` alike.
    pub fn component(&self) -> &str {
        self.measured
            .get_component()
            .expect("a measured key carries its component")
    }

    /// The targeted entity, its last segment's name: `hip_yaw` for
    /// `robot/hip_yaw/target.position` and `robot/hip_yaw.target_position`.
    pub fn entity(&self) -> &str {
        self.measured.get_entity()
    }
}

/// The targets `change` sets, one per measured key, in no particular order.
///
/// A target is the value a client asks one component of an entity to reach,
/// beside the measured value the hardware reports for it. For the component
/// `c` of the entity at path `k`, its measured value is `k.c`
/// (`hip_yaw.position`) and its key takes either of two forms:
///
/// - `k/target.c`, the target as an entity of its own under `k`
///   (`hip_yaw/target.position`, `robot/hip_yaw/target.velocity`);
/// - `k.target_c`, the component prefixed with `target_`
///   (`hip_yaw.target_position`).
///
/// Attributes are a convention of the last segment, so a key with a `.`
/// before its last `/` is no target. The component and the entity are never
/// empty, and attributes after the component carry over to the measured key
/// (`hip_yaw.target_position.x` targets `hip_yaw.position.x`). A last segment
/// `target.c` under a namespace is always the first form:
/// `x/target.target_position` targets `x.target_position`. At the root,
/// `target.c` is no target, and `target.target_position` targets
/// `target.position`.
///
/// The keys of a change carry no order, so where `change` sets both forms of
/// one target, the form decides: the `k/target.c` key and its value are the
/// ones returned, whatever that value is. Across changes the later write wins,
/// as for any key; [`fold_newest_wins`] keeps that when changes are merged.
/// The two keys are distinct keys of the store, each holding what was last
/// written to it.
pub fn set_targets(change: &StateChange) -> Vec<SetTarget<'_>> {
    let mut targets: HashMap<Key, (bool, SetTarget<'_>)> = HashMap::new();
    for (key, value) in &change.set {
        let Some(target) = Target::parse(key) else {
            continue;
        };
        let entity_form = target.entity_form;
        let set = SetTarget {
            key,
            measured: target.measured,
            value,
        };
        match targets.get(&set.measured) {
            Some((held_entity_form, _)) if *held_entity_form || !entity_form => {}
            _ => {
                targets.insert(set.measured.clone(), (entity_form, set));
            }
        }
    }
    targets.into_values().map(|(_, set)| set).collect()
}

/// Fold `later` into `earlier`, per key newest wins, so that the targets
/// ([`set_targets`]) of the folded change are those of applying both in order:
/// a target `later` sets or unsets, in either form, drops `earlier`'s set of
/// that target in either form. For a HAL that merges a backlog of writes into
/// one.
pub fn fold_newest_wins(earlier: &mut StateChange, later: StateChange) {
    let retargeted: HashSet<Key> = later
        .set
        .keys()
        .chain(&later.unset)
        .filter_map(|key| Target::parse(key).map(|target| target.measured))
        .collect();
    if !retargeted.is_empty() {
        earlier.set.retain(|key, _| {
            Target::parse(key).is_none_or(|target| !retargeted.contains(&target.measured))
        });
    }
    for key in later.unset {
        earlier.set.remove(&key);
        earlier.unset.insert(key);
    }
    for (key, value) in later.set {
        earlier.unset.remove(&key);
        earlier.set.insert(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured(key: &str) -> Option<String> {
        Target::parse(&Key::from(key)).map(|t| t.measured().get_path().to_string())
    }

    #[test]
    fn both_forms_target_the_same_measured_key() {
        assert_eq!(
            measured("hip_yaw/target.position").as_deref(),
            Some("hip_yaw.position")
        );
        assert_eq!(
            measured("hip_yaw.target_position").as_deref(),
            Some("hip_yaw.position")
        );
        assert_eq!(
            measured("hip_yaw/target.torque").as_deref(),
            Some("hip_yaw.torque")
        );
        assert_eq!(
            measured("hip_yaw.target_torque").as_deref(),
            Some("hip_yaw.torque")
        );
        assert_eq!(
            measured("robot/hip_yaw/target.velocity").as_deref(),
            Some("robot/hip_yaw.velocity")
        );
        assert_eq!(
            measured("robot/hip_yaw.target_velocity").as_deref(),
            Some("robot/hip_yaw.velocity")
        );
        assert_eq!(
            measured("hip_yaw/target.position.x").as_deref(),
            Some("hip_yaw.position.x")
        );
        assert_eq!(
            measured("hip_yaw.target_position.x").as_deref(),
            Some("hip_yaw.position.x")
        );
    }

    /// A namespaced `target.c` is the entity form, whatever `c` is; a joint
    /// named `target` keeps the attribute form at the root.
    #[test]
    fn a_namespaced_target_segment_is_the_entity_form() {
        assert_eq!(
            measured("x/target.target_position").as_deref(),
            Some("x.target_position")
        );
        assert_eq!(
            measured("target.target_position").as_deref(),
            Some("target.position")
        );
    }

    #[test]
    fn other_keys_are_not_targets() {
        for key in [
            "hip_yaw.position",
            "hip_yaw.torque",
            "hip_yaw",
            "hip_yaw/target",
            "hip_yaw/target.",
            "hip_yaw.target_",
            "target.position",
            "arora/time",
            "a.b/target.position",
            "a.b/joint.target_position",
            "hip_yaw.targetposition",
            "/target.position",
            "a//target.position",
        ] {
            assert_eq!(measured(key), None, "{key}");
        }
    }

    /// Whatever order a change holds its keys in — each fresh `HashMap` has
    /// its own — the entity form's value is the one returned.
    #[test]
    fn a_change_with_both_forms_applies_the_entity_form() {
        for _ in 0..32 {
            let mut change = StateChange::new();
            change
                .set
                .insert(Key::from("hip_yaw.target_position"), Some(Value::from(1.0)));
            change
                .set
                .insert(Key::from("hip_yaw/target.position"), Some(Value::from(2.0)));
            change
                .set
                .insert(Key::from("knee.target_position"), Some(Value::from(3.0)));
            change
                .set
                .insert(Key::from("knee.position"), Some(Value::from(4.0)));
            let mut targets = set_targets(&change);
            targets.sort_by(|a, b| a.measured.get_path().cmp(b.measured.get_path()));
            assert_eq!(targets.len(), 2);
            assert_eq!(targets[0].key, &Key::from("hip_yaw/target.position"));
            assert_eq!(targets[0].measured, Key::from("hip_yaw.position"));
            assert_eq!(targets[0].value, &Some(Value::from(2.0)));
            assert_eq!(targets[0].component(), "position");
            assert_eq!(targets[0].entity(), "hip_yaw");
            assert_eq!(targets[1].key, &Key::from("knee.target_position"));
            assert_eq!(targets[1].value, &Some(Value::from(3.0)));
        }
    }

    fn folded(earlier: StateChange, later: StateChange) -> Vec<(String, Option<Value>)> {
        let mut folded = earlier;
        fold_newest_wins(&mut folded, later);
        let mut targets: Vec<(String, Option<Value>)> = set_targets(&folded)
            .into_iter()
            .map(|t| (t.measured.get_path().to_string(), t.value.clone()))
            .collect();
        targets.sort_by(|a, b| a.0.cmp(&b.0));
        targets
    }

    /// A later write in one form replaces an earlier write in the other, in
    /// both directions.
    #[test]
    fn folding_keeps_the_later_target_across_forms() {
        for (first, second) in [
            ("j/target.position", "j.target_position"),
            ("j.target_position", "j/target.position"),
        ] {
            let mut earlier = StateChange::set(first, Value::from(1.0));
            earlier
                .set
                .insert(Key::from("k.target_position"), Some(Value::from(5.0)));
            earlier
                .set
                .insert(Key::from("other"), Some(Value::from(6.0)));
            let later = StateChange::set(second, Value::from(2.0));
            let mut folded_change = earlier.clone();
            fold_newest_wins(&mut folded_change, later.clone());
            assert!(folded_change.set.contains_key(&Key::from("other")));
            assert_eq!(
                folded(earlier, later),
                vec![
                    ("j.position".to_string(), Some(Value::from(2.0))),
                    ("k.position".to_string(), Some(Value::from(5.0))),
                ]
            );
        }
    }

    /// A later unset of one form drops an earlier set of the other.
    #[test]
    fn folding_an_unset_drops_the_other_forms_set() {
        let earlier = StateChange::set("j/target.position", Value::from(1.0));
        let mut later = StateChange::new();
        later.unset.insert(Key::from("j.target_position"));
        let mut folded_change = earlier;
        fold_newest_wins(&mut folded_change, later);
        assert!(folded_change.set.is_empty());
        assert_eq!(
            folded_change.unset,
            HashSet::from([Key::from("j.target_position")])
        );
    }
}
