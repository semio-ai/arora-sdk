//! Scopes: the top folders records live in.

use uuid::Uuid;

use super::folder::{public::Public as FolderPublic, ROOT_ID};

/// A scope: the top folder of the records one account — a person or an
/// organization — publishes, named by the account's handle (`vizij` in
/// `@vizij/animation`). A record in a scope is named uniquely in it, so its
/// path is `<handle>.<name>`.
///
/// A scope's id is fixed once by its owner, in code, and never derived from
/// the handle: a handle can be renamed, and records name their scope by id.
/// The handle is the folder's name.
///
/// A record published to the store states its scope. A local registry may
/// also hold records with none, at the root: an operator's hand-installed
/// module, a test's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Scope {
  pub id: Uuid,
  pub handle: &'static str,
}

impl Scope {
  /// The scope's folder record: named by its handle, at the root.
  pub fn record(&self) -> FolderPublic {
    FolderPublic {
      name: self.handle.to_string(),
      parent: ROOT_ID,
    }
  }
}

/// The `arora` scope: the records arora declares, such as the behavior
/// `Status` and the behavior tree's `TickId`. Its id leads with the ASCII of
/// "arora", as arora's fixed module ids do, and `f01d` marks a folder.
pub const ARORA: Scope = Scope {
  id: Uuid::from_u128(0x61726f72_6100_0000_f01d_000000000000),
  handle: "arora",
};

#[cfg(test)]
mod tests {
  use super::*;

  /// A scope's record is a folder at the root, named by its handle.
  #[test]
  fn a_scope_s_record_is_a_root_folder_named_by_its_handle() {
    assert_eq!(
      ARORA.record(),
      FolderPublic {
        name: "arora".to_string(),
        parent: ROOT_ID,
      }
    );
    assert_eq!(ARORA.id.to_string(), "61726f72-6100-0000-f01d-000000000000");
  }
}
