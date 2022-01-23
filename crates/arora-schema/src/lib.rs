pub mod module;
pub mod ty;

use derive_more::Display;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Display)]
#[display(fmt = "{}.{}.{}", major, minor, patch)]
pub struct SemanticVersion {
  major: u32,
  minor: u32,
  patch: u32,
}
