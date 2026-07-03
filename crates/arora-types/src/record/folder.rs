//! The `folder` record: the tree node type records are organized under.
//! Folders carry no version — they serialize as-is (public form).

/// The public (wire) form.
pub mod public {
  use serde::{Deserialize, Serialize};
  use uuid::Uuid;

  #[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
  #[serde(rename = "folder_V0_Public")]
  pub struct Public {
    pub name: String,
    pub parent: Uuid,
  }
}
