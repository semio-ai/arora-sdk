//! The four declared exports equal the generator's from
//! `modules/test-behavior-tree-nodes/module.yaml` — mutability and the
//! `children` array included.

use arora_types::module::low::{ExportSymbol, Header};
use case_bt_nodes::nodes;
use std::path::PathBuf;

fn generated_header() -> Header {
  let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("../../../../../modules/test-behavior-tree-nodes/src/arora_generated/module.yaml");
  let yaml = std::fs::read_to_string(&path).expect("read the generated header");
  serde_yaml::from_str(&yaml).expect("parse the generated header")
}

fn exports_by_id(header: &Header) -> Vec<(uuid::Uuid, serde_yaml::Value)> {
  let mut exports: Vec<_> = header
    .exports
    .iter()
    .map(|e| {
      let ExportSymbol::Function(f) = e;
      (f.id, serde_yaml::to_value(e).expect("serialize an export"))
    })
    .collect();
  exports.sort_by_key(|(id, _)| *id);
  exports
}

#[test]
fn the_declared_exports_reproduce_the_generated_ones() {
  let declared = nodes::header(arora_types::module::low::Executor {
    name: "wasm".into(),
    min_version: None,
    max_version: None,
  });
  let generated = generated_header();
  assert_eq!(declared.id, generated.id);
  assert_eq!(declared.name, generated.name);
  let declared = exports_by_id(&declared);
  let declared_ids: Vec<_> = declared.iter().map(|(id, _)| *id).collect();
  let generated: Vec<_> = exports_by_id(&generated)
    .into_iter()
    .filter(|(id, _)| declared_ids.contains(id))
    .collect();
  assert_eq!(declared.len(), 4);
  assert_eq!(declared, generated);
}

#[test]
fn a_mutable_parameter_declares_mutable() {
  let header = nodes::header(arora_types::module::low::Executor {
    name: "wasm".into(),
    min_version: None,
    max_version: None,
  });
  let set_str = header
    .exports
    .iter()
    .find_map(|e| {
      let ExportSymbol::Function(f) = e;
      (f.id == nodes::ids::set_str::FUNCTION).then_some(f)
    })
    .expect("set_str");
  assert!(set_str.parameters[0].mutable);
  assert!(!set_str.parameters[1].mutable);
}
