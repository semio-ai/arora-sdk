//! `#[derive(AroraValue)]` produces the `Value` shape the ARORA-55 generated
//! conversions produce (`arora_generated/vizij/track_output.rs`): a
//! `Structure` under the type id, fields in declared order under their ids;
//! a `Vec` of structures as `ArrayStructure`.

use arora_types::value::{Structure, StructureField, StructureWithoutId, Value};
use arora_types::AroraType;
use case_animation::{AnimTrack, AnimationClip, TrackOutput};
use uuid::Uuid;

fn u(s: &str) -> Uuid {
  Uuid::parse_str(s).unwrap()
}

#[test]
fn a_struct_converts_to_the_generated_wire_shape() {
  let output = TrackOutput {
    track_id: "t1".into(),
    default_key: "k".into(),
    value: Value::F32(0.5),
  };
  let value = Value::from(output.clone());
  assert_eq!(
    value,
    Value::Structure(Structure {
      id: u("76697a69-6a00-0000-0000-000000000110"),
      fields: vec![
        StructureField {
          id: u("76697a69-6a00-0000-0110-000000000001"),
          value: Box::new(Value::String("t1".into()))
        },
        StructureField {
          id: u("76697a69-6a00-0000-0110-000000000002"),
          value: Box::new(Value::String("k".into()))
        },
        StructureField {
          id: u("76697a69-6a00-0000-0110-000000000003"),
          value: Box::new(Value::F32(0.5))
        },
      ],
    })
  );
  assert_eq!(TrackOutput::try_from(value).unwrap(), output);
}

#[test]
fn nested_arrays_of_structures_round_trip() {
  let clip = AnimationClip {
    name: "wave".into(),
    duration: 1200,
    tracks: vec![AnimTrack {
      id: "a".into(),
      name: "A".into(),
      animatable_id: "face/a".into(),
      points: vec![],
    }],
  };
  let value = Value::from(clip.clone());
  let Value::Structure(s) = &value else {
    panic!("a structure")
  };
  assert_eq!(s.id, AnimationClip::arora_type_id());
  assert!(matches!(
    &*s.fields[2].value,
    Value::ArrayStructure { id, elements } if *id == AnimTrack::arora_type_id() && elements.len() == 1
  ));
  assert_eq!(AnimationClip::try_from(value).unwrap(), clip);
  let _ = StructureWithoutId { fields: vec![] };
}

#[test]
fn a_structure_of_the_wrong_id_is_refused() {
  let wrong = Value::Structure(Structure {
    id: AnimTrack::arora_type_id(),
    fields: vec![],
  });
  let err = TrackOutput::try_from(wrong).unwrap_err();
  assert!(
    err.message.contains("expected a `TrackOutput` structure"),
    "{}",
    err.message
  );
}
