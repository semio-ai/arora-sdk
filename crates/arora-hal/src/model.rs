//! The models of a HAL's components: what [`HalAssets::models`](crate::HalAssets::models)
//! states and the HAL module serves (`docs/proposal-hal-components.md`).
//!
//! A HAL is made of named components, fixed for its life; a HAL that is not
//! composed is the one component [`DEVICE`]. A component with a 3D model states
//! it as a [`ComponentModel`]: the published release it uses, the hash of the
//! GLB the device holds, whether the device serves those bytes, and where the
//! model is mounted on another component's model. A component without a model
//! states nothing.

use arora_types::{AroraType, Uuid};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::HalDescription;

/// The name of the one component of a HAL that is not composed.
pub const DEVICE: &str = "device";

/// One component's model.
#[derive(AroraType, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[arora(id = "43adc11a-d394-4074-b2a3-4878a868cdf7")]
pub struct ComponentModel {
    /// The component's name, unique within its HAL.
    #[arora(id = "d4c326ec-adee-4d69-81a1-719a4cb69f0a")]
    pub component: String,
    /// The component's own description, when it has one.
    #[arora(id = "f1f261cf-e3ac-4b61-ac89-488a9e96fd5f")]
    pub description: Option<HalDescription>,
    /// The published release the model is, as resolved. Absent for a model
    /// with no release, such as a face loaded from a file.
    #[arora(id = "a480cf10-70f1-47f9-8849-164c49985a3b")]
    pub reference: Option<ModelReference>,
    /// The [`content_hash`] of the GLB the device holds, when it holds the
    /// bytes. A client caches the model by it.
    #[arora(id = "c2f4a83c-53ec-4e69-a702-31e64e5dd572")]
    pub content_hash: Option<String>,
    /// Whether the device serves the GLB to a remote. A public release and a
    /// Vizij face's GLB are servable; a model held under a private grant never
    /// is, because anything that can call the device receives what it serves.
    #[arora(id = "042ecf38-8656-4fdb-a702-046e72c14a1b")]
    pub servable: bool,
    /// Where the model is drawn on another component's model. Absent for a
    /// model that is not mounted.
    #[arora(id = "5420bc62-f80f-4485-b34b-d76b5260f978")]
    pub mount: Option<Mount>,
}

/// A published model release, as resolved from Semio Studio: what a client
/// fetches from Studio with its own rights.
#[derive(AroraType, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[arora(id = "0ce6f4ed-c7de-47ce-9093-614b1b42fc63")]
pub struct ModelReference {
    /// The release's asset, such as `@quori/quori`.
    #[arora(id = "dcdafe83-0355-4b23-a459-15b5bb121cac")]
    pub scoped_name: String,
    /// The release's version.
    #[arora(id = "4134da05-8262-4a13-9184-cc069830ad7c")]
    pub version: String,
    /// The release's content hash, as Studio states it.
    #[arora(id = "472c6c67-55ef-4d07-8954-29040837965b")]
    pub content_hash: String,
}

/// A model drawn on a `screen` element of another component's model.
#[derive(AroraType, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[arora(id = "1fa28b36-11f5-4497-afc6-48abf0ec4863")]
pub struct Mount {
    /// The component whose model holds the screen.
    #[arora(id = "84cd0dac-4b62-4a53-b99f-5489ab077620")]
    pub component: String,
    /// The screen element of that model.
    #[arora(id = "e07afcfd-1305-4143-9b0a-5d15efd7c4a3")]
    pub element: Uuid,
}

/// A servable model's bytes, under its component.
#[derive(AroraType, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[arora(id = "eeafbaa4-e8d1-43b6-82c7-0fb450fd6a9f")]
pub struct ComponentGlb {
    /// The component's name.
    #[arora(id = "e7f7a84e-9e06-4ee3-b19b-9d44f482e295")]
    pub component: String,
    /// The model, as GLB bytes.
    #[arora(id = "155eb709-d377-4bf4-89d2-7c1f2db9af01")]
    pub glb: Vec<u8>,
}

/// The content hash of a GLB: its SHA-256, in lowercase hex — the form Semio
/// Studio addresses content by.
pub fn content_hash(glb: &[u8]) -> String {
    Sha256::digest(glb)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_types::value::Value;

    #[test]
    fn the_content_hash_is_lowercase_hex_sha256() {
        assert_eq!(
            content_hash(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn a_model_crosses_the_value_plane() {
        let model = ComponentModel {
            component: DEVICE.to_string(),
            description: Some(HalDescription {
                model_family: Some("quori".into()),
                ..Default::default()
            }),
            reference: Some(ModelReference {
                scoped_name: "@quori/quori".into(),
                version: "1.0.0".into(),
                content_hash: content_hash(b"glTF"),
            }),
            content_hash: Some(content_hash(b"glTF")),
            servable: true,
            mount: Some(Mount {
                component: "robot".into(),
                element: Uuid::from_u128(1),
            }),
        };
        let value = Value::from(model.clone());
        assert_eq!(ComponentModel::try_from(value).unwrap(), model);

        let glb = ComponentGlb {
            component: DEVICE.to_string(),
            glb: b"glTF".to_vec(),
        };
        assert_eq!(
            ComponentGlb::try_from(Value::from(glb.clone())).unwrap(),
            glb
        );
    }
}
