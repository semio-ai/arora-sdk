//! Record selection + classification vocabulary.
//!
//! `Selector` and `RecordType` are neutral, store-agnostic types previously
//! sourced from the private `semio-client` crate (`common::{Selector,
//! RecordType}`). Hosting them here lets arora's local and codegen paths
//! (registry, module tooling, behavior-tree) name and classify records without
//! depending on semio-client — only the actual network/store client needs it.

use serde::{Deserialize, Serialize};
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;
use uuid::Uuid;

/// Describes a record by its UUID or its path, with no version information.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Selector {
  Id(Uuid),
  Path(String),
}

impl FromStr for Selector {
  // String (rather than Infallible) to stay drop-in compatible with
  // semio-client's `Selector::from_str` for the eventual consumer swap.
  type Err = String;
  fn from_str(s: &str) -> Result<Self, Self::Err> {
    Ok(match Uuid::parse_str(s) {
      Ok(id) => Self::Id(id),
      Err(_) => Self::Path(s.to_string()),
    })
  }
}

impl Display for Selector {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    match self {
      Self::Id(id) => write!(f, "{}", id),
      Self::Path(path) => write!(f, "{}", path),
    }
  }
}

/// The kind of a record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecordType {
  User,
  Folder,
  Organization,
  Module,
  Structure,
  Enumeration,
  Unknown,
}

impl Display for RecordType {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    let s = match self {
      Self::User => "user",
      Self::Folder => "folder",
      Self::Organization => "organization",
      Self::Module => "module",
      Self::Structure => "structure",
      Self::Enumeration => "enumeration",
      Self::Unknown => "unknown",
    };
    write!(f, "{}", s)
  }
}

#[cfg(test)]
mod tests {
  use super::{RecordType, Selector};
  use std::str::FromStr;
  use uuid::Uuid;

  #[test]
  fn selector_from_str_id_vs_path() {
    let id = Uuid::new_v4();
    assert_eq!(
      Selector::from_str(&id.to_string()).unwrap(),
      Selector::Id(id)
    );
    assert_eq!(
      Selector::from_str("foo/bar").unwrap(),
      Selector::Path("foo/bar".into())
    );
  }

  #[test]
  fn selector_display() {
    let id = Uuid::new_v4();
    assert_eq!(Selector::Id(id).to_string(), id.to_string());
    assert_eq!(Selector::Path("a/b".into()).to_string(), "a/b");
  }

  #[test]
  fn record_type_display() {
    assert_eq!(RecordType::Module.to_string(), "module");
    assert_eq!(RecordType::Unknown.to_string(), "unknown");
  }
}
