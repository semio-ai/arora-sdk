//! The ids Semio Studio knows the robot by.
//!
//! Studio builds a robot from an uploaded URDF, gives every joint and the
//! robot's root placement an id of its own, and saves the result as a GLB
//! whose nodes carry them (the `RobotData` glTF extension). A connected
//! device drives the robot Studio draws by publishing `<id>.position`: one
//! per joint, in radians, and six for the root — its translation in metres
//! and its roll, pitch, yaw in radians. [`StudioModel::from_glb`] reads those
//! ids back from the GLB Studio lets you download ("Download Asset" on the
//! model), so the device can publish under them.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use arora_hal_mujoco::BasePoseKeys;
use serde_json::Value as Json;

/// The ids of a robot model saved by Studio.
#[derive(Debug, Clone)]
pub struct StudioModel {
    /// Joint name (the URDF's) → the id Studio drives it by.
    pub joints: HashMap<String, String>,
    /// The root's translation and rotation ids, when the model is a robot
    /// Studio can place.
    pub base: Option<BasePoseKeys>,
}

impl StudioModel {
    /// Read the ids from a GLB saved by Studio.
    pub fn from_glb(path: &Path) -> Result<Self> {
        let bytes =
            std::fs::read(path).with_context(|| format!("could not read {}", path.display()))?;
        Self::from_glb_bytes(&bytes)
            .with_context(|| format!("{} is not a Studio robot model", path.display()))
    }

    fn from_glb_bytes(bytes: &[u8]) -> Result<Self> {
        let word = |at: usize| -> Result<u32> {
            let slice = bytes
                .get(at..at + 4)
                .ok_or_else(|| anyhow!("truncated GLB"))?;
            Ok(u32::from_le_bytes(slice.try_into().expect("four bytes")))
        };
        if bytes.get(0..4) != Some(b"glTF") {
            bail!("not a GLB file");
        }
        if word(4)? != 2 {
            bail!("only GLB version 2 is read");
        }
        let json_length = word(12)? as usize;
        if word(16)? != 0x4E4F_534A {
            bail!("the first GLB chunk is not JSON");
        }
        let json = bytes
            .get(20..20 + json_length)
            .ok_or_else(|| anyhow!("truncated GLB"))?;
        let document: Json = serde_json::from_slice(json).context("the GLB's JSON")?;
        let nodes = document
            .get("nodes")
            .and_then(Json::as_array)
            .ok_or_else(|| anyhow!("the GLB has no nodes"))?;

        let mut joints = HashMap::new();
        let mut base = None;
        for data in nodes
            .iter()
            .filter_map(|node| node.pointer("/extensions/RobotData"))
        {
            let features = data.get("features");
            // A joint is named on its node; its value may repeat the name.
            if let Some(id) = animated_id(features.and_then(|f| f.get("jointValue"))) {
                let name = data
                    .get("name")
                    .and_then(Json::as_str)
                    .ok_or_else(|| anyhow!("joint {id} has no name"))?;
                if joints.insert(name.to_string(), id).is_some() {
                    bail!("two joints are named {name}");
                }
            }
            let is_root = data.get("type").and_then(Json::as_str) == Some("body")
                && data.get("root").and_then(Json::as_bool) == Some(true);
            if is_root {
                if base.is_some() {
                    bail!("more than one robot root");
                }
                let id = |feature: &str| -> Result<String> {
                    animated_id(features.and_then(|f| f.get(feature)))
                        .ok_or_else(|| anyhow!("the robot's root has no animated {feature}"))
                };
                base = Some(BasePoseKeys {
                    x: id("translation.x")?,
                    y: id("translation.y")?,
                    z: id("translation.z")?,
                    roll: id("rotation.r")?,
                    pitch: id("rotation.p")?,
                    yaw: id("rotation.y")?,
                });
            }
        }
        if joints.is_empty() {
            bail!("no animated joint");
        }
        Ok(Self { joints, base })
    }

    /// The id of each of `names`, in order; fails naming every joint the
    /// model lacks.
    pub fn joint_ids(&self, names: &[&str]) -> Result<Vec<String>> {
        let missing: Vec<&str> = names
            .iter()
            .copied()
            .filter(|name| !self.joints.contains_key(*name))
            .collect();
        if !missing.is_empty() {
            bail!(
                "the Studio model has no joint {missing:?}; upload the URDF tools/export-urdf.py \
                 writes, then download the model Studio saved from it"
            );
        }
        Ok(names
            .iter()
            .map(|name| self.joints[*name].clone())
            .collect())
    }
}

/// An animated feature's id: stored as the animatable itself (`{"id": …}`)
/// or as its bare id.
fn animated_id(feature: Option<&Json>) -> Option<String> {
    let feature = feature?;
    if feature.get("animated").and_then(Json::as_bool) != Some(true) {
        return None;
    }
    match feature.get("value")? {
        Json::String(id) => Some(id.clone()),
        value => value.get("id").and_then(Json::as_str).map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GLB holding only its JSON chunk, the ids laid out as Studio lays
    /// them.
    fn glb(json: &Json) -> Vec<u8> {
        let mut chunk = serde_json::to_vec(json).unwrap();
        while chunk.len() % 4 != 0 {
            chunk.push(b' ');
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&((20 + chunk.len()) as u32).to_le_bytes());
        out.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
        out.extend_from_slice(&chunk);
        out
    }

    fn animated(id: &str) -> Json {
        serde_json::json!({"animated": true, "value": {"id": id, "type": "number"}})
    }

    #[test]
    fn reads_the_joint_and_root_ids() {
        let document = serde_json::json!({"nodes": [
            {"extensions": {"RobotData": {"type": "body", "root": true, "features": {
                "translation.x": animated("tx"), "translation.y": animated("ty"),
                "translation.z": animated("tz"), "rotation.r": animated("rr"),
                "rotation.p": animated("rp"), "rotation.y": {"animated": true, "value": "ry"},
                "scale.x": {"animated": false, "value": 1}}}}},
            {"extensions": {"RobotData": {"type": "revolute", "name": "left_knee", "features": {
                "jointValue": {"animated": true, "value": {"id": "k-1", "name": "left_knee"}}}}}},
            {"extensions": {"RobotData": {"type": "fixed", "name": "mount", "features": {}}}},
            {"mesh": 0}
        ]});
        let model = StudioModel::from_glb_bytes(&glb(&document)).unwrap();
        assert_eq!(model.joint_ids(&["left_knee"]).unwrap(), vec!["k-1"]);
        assert!(model.joint_ids(&["left_knee", "right_knee"]).is_err());
        let base = model.base.unwrap();
        assert_eq!(
            [base.x, base.y, base.z, base.roll, base.pitch, base.yaw],
            ["tx", "ty", "tz", "rr", "rp", "ry"].map(String::from)
        );
    }

    #[test]
    fn refuses_what_is_not_a_studio_robot() {
        assert!(StudioModel::from_glb_bytes(b"not a glb").is_err());
        let no_joints = serde_json::json!({"nodes": [{"mesh": 0}]});
        assert!(StudioModel::from_glb_bytes(&glb(&no_joints)).is_err());
        let knee = |id: &str| {
            serde_json::json!({"extensions": {"RobotData": {"type": "revolute", "name": "knee",
                "features": {"jointValue": {"animated": true, "value": id}}}}})
        };
        let twice = serde_json::json!({"nodes": [knee("a"), knee("b")]});
        assert!(StudioModel::from_glb_bytes(&glb(&twice)).is_err());
    }
}
