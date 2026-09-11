//! The declared signatures are the ones vizij builds by hand
//! (`speech::say_signature()`, `gaze::look_at_signature()`), and a host-only
//! module now has a header it can be exported from.

use arora_types::module::low::{ExportSymbol, TypeRef};
use arora_types::record::ty::{FrozenTy, PrimitiveKind};
use arora_types::AroraType;
use case_vizij_skills::{gaze, tts, Status};

#[test]
fn say_declares_text_voice_and_a_mutable_viseme_returning_status() {
  let (_, name, say, _) = tts::host_functions().remove(0);
  assert_eq!(name, "say");
  assert_eq!(
    say.parameter_ordering,
    vec![
      tts::ids::say::TEXT,
      tts::ids::say::VOICE,
      tts::ids::say::VISEME
    ]
  );
  for id in &say.parameter_ordering {
    assert_eq!(say.parameters[id].ty, FrozenTy::from(PrimitiveKind::String));
  }
  assert!(!say.parameters[&tts::ids::say::TEXT].mutable);
  assert!(say.parameters[&tts::ids::say::VISEME].mutable);
  let FrozenTy::FrozenScalar(ret) = &say.return_ty else {
    panic!("a record return")
  };
  assert_eq!(ret.reference.id, Status::arora_type_id());
  assert_eq!(ret.reference.version.to_string(), "1.0.0");
}

#[test]
fn look_at_declares_an_f32_array_target() {
  let (_, _, look_at, _) = gaze::host_functions().remove(0);
  assert_eq!(
    look_at.parameters[&gaze::ids::look_at::TARGET].ty,
    FrozenTy::from(PrimitiveKind::ArrayF32)
  );
  let header = gaze::header(arora_types::module::low::Executor {
    name: "wasm".into(),
    min_version: None,
    max_version: None,
  });
  let ExportSymbol::Function(f) = &header.exports[0];
  assert!(matches!(f.parameters[1].ty, TypeRef::Array { id } if id == *arora_types::ty::F32_ID));
}

#[test]
fn a_host_only_module_exports_its_interface_as_a_record() {
  // Today these modules have no exportable interface at all; the declaration
  // yields the store's record — no executor in it, since none applies to a
  // module linked into the host — and reads back identical.
  use arora_types::record::module::frozen::Module;
  let folder = uuid::Uuid::parse_str("1232d7c4-d5af-4f91-9a34-8c707b0c9693").unwrap();
  let record = tts::record(folder);
  let yaml = serde_yaml::to_string(&record).expect("serialize");
  assert!(yaml.contains("name: tts-piper"));
  assert!(yaml.contains("name: say"));
  let back: Module = serde_yaml::from_str(&yaml).expect("parse");
  assert_eq!(back, record);
  // A header, by contrast, is only written when an artifact is exported —
  // and the exporter names the executor then.
}
