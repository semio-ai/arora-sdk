//! The declared exports reproduce the generator's, export for export, for the
//! functions this case declares (`fixtures/module.yaml` is the generated
//! header of the real module; see `fixtures/ORIGIN`).

use arora_types::module::low::{ExportSymbol, Header};
use case_animation::animation;
use std::path::PathBuf;

fn generated_header() -> Header {
  let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/module.yaml");
  let yaml = std::fs::read_to_string(&path).expect("read the fixture header");
  serde_yaml::from_str(&yaml).expect("parse the fixture header")
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
  let declared = animation::header(arora_types::module::low::Executor {
    name: "wasm".into(),
    min_version: None,
    max_version: None,
  });
  let generated = generated_header();
  assert_eq!(declared.id, generated.id);
  let declared = exports_by_id(&declared);
  let declared_ids: Vec<_> = declared.iter().map(|(id, _)| *id).collect();
  // The generated header has thirteen exports; this case declares six of them.
  let generated: Vec<_> = exports_by_id(&generated)
    .into_iter()
    .filter(|(id, _)| declared_ids.contains(id))
    .collect();
  assert_eq!(declared.len(), 6);
  assert_eq!(declared, generated);
}

#[test]
fn a_struct_parameter_and_an_array_return_declare_by_their_type_ids() {
  use arora_types::module::low::TypeRef;
  use arora_types::AroraType;
  use case_animation::{AnimationClip, TrackOutput};
  let header = animation::header(arora_types::module::low::Executor {
    name: "wasm".into(),
    min_version: None,
    max_version: None,
  });
  let find = |id| {
    header.exports.iter().find_map(|e| {
      let ExportSymbol::Function(f) = e;
      (f.id == id).then_some(f)
    })
  };
  let load = find(animation::ids::load_animation::FUNCTION).expect("load_animation");
  assert!(
    matches!(load.parameters[0].ty, TypeRef::Scalar { id } if id == AnimationClip::arora_type_id())
  );
  let step = find(animation::ids::step::FUNCTION).expect("step");
  assert!(matches!(step.ret, TypeRef::Array { id } if id == TrackOutput::arora_type_id()));
}
