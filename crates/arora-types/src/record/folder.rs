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

/// A folder declared in Rust, with what a registry holds for it: the home a
/// type names with `#[arora(parent = …)]`.
///
/// A type's record lives in the folder the type declares, whatever names the
/// type, so every module sharing the type ships the same record, and the
/// folder's record with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeclaredFolder {
  pub id: Uuid,
  pub name: &'static str,
  /// The folder holding this one; `None` for the root.
  pub parent: Option<&'static DeclaredFolder>,
}

impl DeclaredFolder {
  /// The folder's record.
  pub fn record(&self) -> public::Public {
    public::Public {
      name: self.name.to_string(),
      parent: self.parent.map_or(ROOT_ID, |parent| parent.id),
    }
  }

  /// This folder's record and those of the folders holding it, each with its
  /// id: what a registry holds for a record in this folder to load.
  pub fn records(&self) -> impl Iterator<Item = (Uuid, public::Public)> {
    std::iter::successors(Some(*self), |folder| folder.parent.copied())
      .map(|folder| (folder.id, folder.record()))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const OUTER: DeclaredFolder = DeclaredFolder {
    id: Uuid::from_u128(0x0f01),
    name: "outer",
    parent: None,
  };
  const INNER: DeclaredFolder = DeclaredFolder {
    id: Uuid::from_u128(0x0f02),
    name: "inner",
    parent: Some(&OUTER),
  };

  /// A declared folder's records are its own and those of the folders
  /// holding it, up to the root.
  #[test]
  fn a_declared_folder_s_records_reach_the_root() {
    let records: Vec<(Uuid, public::Public)> = INNER.records().collect();
    assert_eq!(
      records,
      vec![
        (
          INNER.id,
          public::Public {
            name: "inner".to_string(),
            parent: OUTER.id
          }
        ),
        (
          OUTER.id,
          public::Public {
            name: "outer".to_string(),
            parent: ROOT_ID
          }
        ),
      ]
    );
  }
}
