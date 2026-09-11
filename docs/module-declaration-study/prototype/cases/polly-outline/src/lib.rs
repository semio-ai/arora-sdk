//! polly declared in its own file: the form for a module too large to inline.
//! `Status` is shared with the inline case's shape.

use arora_module_derive::AroraValue;
use arora_types::AroraType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, AroraType, AroraValue)]
#[arora(id = "325a5767-e344-4532-860e-0749bcf2e428")]
pub enum Status {
  #[arora(id = "766e9e9a-446d-4e46-83e6-14b7ca101169")]
  Success,
  #[arora(id = "2468f46c-bb60-425c-9a4d-9ad326ccc7e2")]
  Failure,
  #[arora(id = "acd79ec6-0c44-401a-82f8-5da5422d3eec")]
  Running,
}

pub mod polly;
