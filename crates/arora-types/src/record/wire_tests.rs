//! Golden wire-format tests: the frozen record serde must stay byte-compatible
//! with semio-record's, because these YAML documents are what the Semio store
//! consumes. Fixtures are copied verbatim from committed, store-accepted
//! records under `modules/test-behavior-tree-nodes/records/`.

use super::enumeration::frozen::Enumeration;
use super::folder::public::Public as Folder;
use super::structure::frozen::Structure;
use super::ty::{FrozenTy, PrimitiveKind};

const TICK_ID_STRUCTURE_YAML: &str = "\
parent: 1232d7c4-d5af-4f91-9a34-8c707b0c9693
name: TickId
fields:
  237992d2-17d1-459f-bca1-7185fa6a69d7:
    name: callable_id
    type:
      type: primitive
      value:
        kind: u64
";

const STATUS_ENUMERATION_YAML: &str = "\
parent: 1232d7c4-d5af-4f91-9a34-8c707b0c9693
name: Status
variants:
  766e9e9a-446d-4e46-83e6-14b7ca101169:
    name: Success
    type:
      type: primitive
      value:
        kind: unit
  2468f46c-bb60-425c-9a4d-9ad326ccc7e2:
    name: Failure
    type:
      type: primitive
      value:
        kind: unit
  acd79ec6-0c44-401a-82f8-5da5422d3eec:
    name: Running
    type:
      type: primitive
      value:
        kind: unit
";

const FOLDER_YAML: &str = "\
name: behavior_tree
parent: 00000000-0000-0000-0000-000001000000
";

#[test]
fn frozen_structure_wire_round_trips() {
  let structure: Structure = serde_yaml::from_str(TICK_ID_STRUCTURE_YAML).unwrap();
  assert_eq!(structure.name, "TickId");
  let field = structure.field_named("callable_id").unwrap();
  assert_eq!(field.ty, FrozenTy::from(PrimitiveKind::U64));

  let reserialized = serde_yaml::to_string(&structure).unwrap();
  let reparsed: Structure = serde_yaml::from_str(&reserialized).unwrap();
  assert_eq!(reparsed, structure);
}

#[test]
fn frozen_enumeration_wire_round_trips() {
  let enumeration: Enumeration = serde_yaml::from_str(STATUS_ENUMERATION_YAML).unwrap();
  assert_eq!(enumeration.name, "Status");
  assert_eq!(enumeration.variants.len(), 3);
  // IndexMap preserves the wire order of variants.
  let names: Vec<&str> = enumeration
    .variants
    .values()
    .map(|v| v.name.as_str())
    .collect();
  assert_eq!(names, ["Success", "Failure", "Running"]);
  for variant in enumeration.variants.values() {
    assert_eq!(variant.ty, FrozenTy::from(PrimitiveKind::Unit));
  }

  let reserialized = serde_yaml::to_string(&enumeration).unwrap();
  let reparsed: Enumeration = serde_yaml::from_str(&reserialized).unwrap();
  assert_eq!(reparsed, enumeration);
}

#[test]
fn folder_wire_round_trips() {
  let folder: Folder = serde_yaml::from_str(FOLDER_YAML).unwrap();
  assert_eq!(folder.name, "behavior_tree");

  let reserialized = serde_yaml::to_string(&folder).unwrap();
  let reparsed: Folder = serde_yaml::from_str(&reserialized).unwrap();
  assert_eq!(reparsed, folder);
}

#[test]
fn frozen_ty_serializes_with_adjacent_type_value_tags() {
  let yaml = serde_yaml::to_string(&FrozenTy::from(PrimitiveKind::U64)).unwrap();
  // The exact adjacent tagging semio-record uses: `type` + `value`.
  assert!(yaml.contains("type: primitive"), "got: {yaml}");
  assert!(yaml.contains("kind: u64"), "got: {yaml}");
}
