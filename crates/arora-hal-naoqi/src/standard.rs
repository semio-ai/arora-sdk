//! The Arora standard this HAL maps NAOqi onto.
//!
//! Like a Vizij profile, the standard is an interface, as data: the keys a robot exposes
//! to Arora, each with its kind, type, unit and range, independent of the middleware
//! behind them (`standard/arora-robot.json`). The HAL is the mapping: [`keys`](crate::keys)
//! says which NAOqi source or command feeds each standard key. No Arora standard exists
//! yet, so this one is local to the HAL; a second HAL mapping the same keys is what would
//! move it to a crate of its own.

use serde::Deserialize;

/// The standard, as JSON.
pub const PROFILE_JSON: &str = include_str!("../standard/arora-robot.json");

/// An interface: the keys one party exposes to another.
#[derive(Clone, Debug, Deserialize)]
pub struct Profile {
    pub id: String,
    pub version: String,
    pub title: String,
    pub description: String,
    /// Where the paths live: `device`, absolute on the device's store.
    pub scope: String,
    pub keys: Vec<StandardKey>,
}

/// One key of a [`Profile`]. A path segment in braces (`{joint}`) stands for one name.
#[derive(Clone, Debug, Deserialize)]
pub struct StandardKey {
    pub path: String,
    pub kind: KeyKind,
    /// An Arora type name: `f64`, `u32`, `bool`, …
    pub value_type: String,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    pub description: String,
}

/// Which way a key flows, from the device's point of view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyKind {
    /// Sensed and published by the device.
    Output,
    /// Commanded to the device.
    Input,
}

impl StandardKey {
    /// Whether `key` is an instance of this key's path.
    pub fn matches(&self, key: &str) -> bool {
        let pattern: Vec<&str> = self.path.split('.').collect();
        let key: Vec<&str> = key.split('.').collect();
        pattern.len() == key.len()
            && pattern.iter().zip(&key).all(|(pattern, segment)| {
                (pattern.starts_with('{') && pattern.ends_with('}') && !segment.is_empty())
                    || pattern == segment
            })
    }
}

impl Profile {
    /// The standard key `key` is an instance of.
    pub fn find(&self, key: &str) -> Option<&StandardKey> {
        self.keys.iter().find(|standard| standard.matches(key))
    }
}

/// The standard this HAL maps NAOqi onto.
pub fn profile() -> Profile {
    serde_json::from_str(PROFILE_JSON).expect("the standard profile is valid JSON")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys;

    /// Every key the HAL publishes, for a robot with one joint.
    fn published_keys() -> Vec<String> {
        let mut published: Vec<String> = [
            keys::POSITION,
            keys::VELOCITY,
            keys::STIFFNESS,
            keys::TEMPERATURE,
            keys::CURRENT,
        ]
        .iter()
        .map(|component| format!("head_yaw.{component}"))
        .collect();
        for table in [
            keys::BATTERY_MEMORY_KEYS,
            keys::INERTIAL_MEMORY_KEYS,
            keys::SONAR_MEMORY_KEYS,
            keys::TOUCH_EVENTS,
        ] {
            published.extend(table.iter().map(|(_, key)| key.to_string()));
        }
        published.push(keys::BATTERY_CHARGING.to_string());
        published
    }

    /// Every key the HAL turns into a NAOqi command, for a robot with one joint.
    fn commanded_keys() -> Vec<String> {
        vec![
            format!("head_yaw.{}", keys::TARGET_POSITION),
            format!("head_yaw.{}", keys::TARGET_STIFFNESS),
            format!("{}.FaceLeds.color", keys::LED_ENTITY),
            format!("{}.FaceLeds.intensity", keys::LED_ENTITY),
            keys::VELOCITY_X.to_string(),
            keys::VELOCITY_Y.to_string(),
            keys::ROTATION_Z.to_string(),
        ]
    }

    #[test]
    fn every_key_of_the_hal_is_standard_with_its_kind() {
        let profile = profile();
        for (keys, kind) in [
            (published_keys(), KeyKind::Output),
            (commanded_keys(), KeyKind::Input),
        ] {
            for key in keys {
                let standard = profile
                    .find(&key)
                    .unwrap_or_else(|| panic!("{key} is not in the standard"));
                assert_eq!(standard.kind, kind, "{key}");
            }
        }
    }

    #[test]
    fn every_standard_key_is_mapped() {
        let profile = profile();
        let mapped: Vec<String> = published_keys()
            .into_iter()
            .chain(commanded_keys())
            .collect();
        for standard in &profile.keys {
            assert!(
                mapped.iter().any(|key| standard.matches(key)),
                "{} is mapped by no NAOqi source or command",
                standard.path
            );
        }
    }

    #[test]
    fn placeholders_match_one_segment() {
        let profile = profile();
        assert!(profile.find("l_hand.target_position").is_some());
        assert!(profile.find("target_position").is_none());
        assert!(profile.find("led.FaceLeds.color").is_some());
        assert!(profile.find("led.color").is_none());
    }
}
