use std::collections::HashMap;

use crate::config::JointIdMapping;
use crate::conversions::keys;

/// The joints of the robot: NAOqi names, Arora ids and the `ALMemory` keys sampled for them.
#[derive(Debug, Clone, Default)]
pub(crate) struct JointTable {
    /// NAOqi names, in the order of `ALMotion.getBodyNames("Body")`.
    names: Vec<String>,
    /// Arora ids, aligned with `names`.
    ids: Vec<String>,
    by_id: HashMap<String, usize>,
}

/// The `ALMemory` sub-paths sampled for every joint, with the key component they feed.
const JOINT_SENSORS: &[(&str, &str)] = &[
    ("Position/Sensor", keys::POSITION),
    ("Hardness/Actuator", keys::STIFFNESS),
    ("Temperature/Sensor", keys::TEMPERATURE),
    ("ElectricCurrent/Sensor", keys::CURRENT),
];

impl JointTable {
    pub(crate) fn new(names: Vec<String>, mapping: &JointIdMapping) -> Self {
        let (names, ids): (Vec<String>, Vec<String>) = names
            .into_iter()
            .filter_map(|name| {
                let id = match mapping {
                    JointIdMapping::SnakeCase => keys::joint_id_from_naoqi_name(&name),
                    JointIdMapping::Override { ids } => ids.get(&name)?.clone(),
                    JointIdMapping::Extend { ids } => ids
                        .get(&name)
                        .cloned()
                        .unwrap_or_else(|| keys::joint_id_from_naoqi_name(&name)),
                };
                Some((name, id))
            })
            .unzip();
        let by_id = ids
            .iter()
            .enumerate()
            .map(|(index, id)| (id.clone(), index))
            .collect();
        Self { names, ids, by_id }
    }

    pub(crate) fn len(&self) -> usize {
        self.names.len()
    }

    /// The NAOqi name of a joint from its Arora id.
    pub(crate) fn naoqi_name(&self, id: &str) -> Option<&str> {
        self.by_id.get(id).map(|&index| self.names[index].as_str())
    }

    /// The Arora ids, aligned with the NAOqi names.
    #[cfg(test)]
    pub(crate) fn ids(&self) -> &[String] {
        &self.ids
    }

    /// The `ALMemory` keys sampled for the joints, with the Arora key each one feeds; the
    /// order is stable and matches the values `ALMemory.getListData` returns.
    pub(crate) fn memory_keys(&self) -> Vec<(String, String)> {
        self.names
            .iter()
            .zip(&self.ids)
            .flat_map(|(name, id)| {
                JOINT_SENSORS.iter().map(move |(path, component)| {
                    (
                        keys::joint_memory_key(name, path),
                        format!("{id}.{component}"),
                    )
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_maps_names_and_ids() {
        let table = JointTable::new(
            vec!["HeadYaw".into(), "LShoulderPitch".into()],
            &JointIdMapping::Extend {
                ids: HashMap::from([("HeadYaw".to_string(), "neck".to_string())]),
            },
        );
        assert_eq!(table.len(), 2);
        assert_eq!(table.ids(), ["neck", "l_shoulder_pitch"]);
        assert_eq!(table.naoqi_name("neck"), Some("HeadYaw"));
        assert_eq!(table.naoqi_name("l_shoulder_pitch"), Some("LShoulderPitch"));
        assert_eq!(table.naoqi_name("nope"), None);
        let keys = table.memory_keys();
        assert_eq!(keys.len(), 8);
        assert_eq!(
            keys[0],
            (
                "Device/SubDeviceList/HeadYaw/Position/Sensor/Value".to_string(),
                "neck.position".to_string()
            )
        );
    }

    #[test]
    fn override_keeps_listed_joints_only() {
        let table = JointTable::new(
            vec!["HeadYaw".into(), "HeadPitch".into()],
            &JointIdMapping::Override {
                ids: HashMap::from([("HeadPitch".to_string(), "nod".to_string())]),
            },
        );
        assert_eq!(table.ids(), ["nod"]);
    }
}
