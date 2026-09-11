//! Four of `modules/test-behavior-tree-nodes`'s exports: the mutable-parameter
//! data nodes and one control node taking `children`. Ids are the module's.

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

/// `behavior_tree.TickId`: the callable a control node ticks for a child.
#[derive(Debug, Clone, PartialEq, Eq, AroraType, AroraValue)]
#[arora(id = "6f49e650-84ca-4899-a9bd-1f3bf17fab51")]
pub struct TickId {
  #[arora(id = "237992d2-17d1-459f-bca1-7185fa6a69d7")]
  pub callable_id: u64,
}

#[arora_module_derive::module(
  id = "3f0aa0c0-6dd5-4cc5-a05e-97a21ec44455",
  name = "behavior-tree-nodes",
  version = "0.1.0"
)]
pub mod nodes {
  use super::*;
  use arora_module_derive::export;

  #[export(id = "ef48e6d3-c735-4b5c-8f63-fc54d94dd4ee")]
  pub fn status_identity(
    #[param(id = "e1f174e6-ca9e-4344-84cb-7f3f22115239")] value: Status,
  ) -> Status {
    value
  }

  /// Writes `value` into the caller's `variable` — a mutable parameter, sent
  /// back as a mutated argument.
  #[export(id = "c803889f-4757-4b56-908f-4b2b47041eff")]
  pub fn set_str(
    #[param(id = "8fa2f965-1eb5-40d9-baca-8facef0d31a8")] variable: &mut String,
    #[param(id = "88438955-7872-44ad-8464-d636dc5fe26f")] value: String,
  ) -> Status {
    *variable = value;
    Status::Success
  }

  #[export(id = "7dce01ed-9818-4b7d-b45a-2e7fdece3633")]
  pub fn unset_str(
    #[param(id = "2c84bf0f-4ec2-41a4-83ee-3f92a53be79d")] variable: &mut String,
  ) -> Status {
    variable.clear();
    Status::Success
  }

  /// A sequence over its children: here, succeeds when given any children
  /// (ticking them needs the indirect-dispatch seam a host provides).
  #[export(id = "32246df6-ab5d-4f18-9221-23e28731de93")]
  pub fn seq(
    #[param(id = "5b6e9515-dbcc-411d-bee9-3d8cba5fedda")] children: Vec<TickId>,
  ) -> Status {
    if children.is_empty() {
      Status::Failure
    } else {
      Status::Success
    }
  }
}
