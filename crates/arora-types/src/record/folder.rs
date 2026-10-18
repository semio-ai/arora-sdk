//! The `folder` record: the tree node type records are organized under.
//! Folders carry no version — they serialize as-is (public form).

use uuid::Uuid;

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

/// The root's id: the parent of a top-level folder or record. A registry
/// holds the root without a record.
pub const ROOT_ID: Uuid = Uuid::from_bytes([
  0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
]);
