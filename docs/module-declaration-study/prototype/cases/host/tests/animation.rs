//! The animation module: struct arguments, array returns, and the property
//! VIZ-129 found broken — argument order does not matter.

use arora_types::call::{Call, CallBridge};
use arora_types::value::{StructureField, Value};
use arora_types::AroraType;
use case_animation::animation::ids;
use case_animation::{AnimTrack, AnimationClip, TrackOutput};
use case_host::{animation, engine_with};

fn field(id: uuid::Uuid, value: Value) -> StructureField {
  StructureField {
    id,
    value: Box::new(value),
  }
}

#[test]
fn arguments_are_matched_by_id_whatever_their_order() {
  let mut engine = engine_with(vec![animation()]);
  let call = |args: Vec<StructureField>| Call {
    module_id: Some(ids::MODULE),
    id: ids::add_instance::FUNCTION,
    args,
  };
  let declared_order = engine
    .arora_call(call(vec![
      field(ids::add_instance::PLAYER, Value::U32(2)),
      field(ids::add_instance::ANIM, Value::U32(7)),
    ]))
    .expect("add_instance");
  let reversed_order = engine
    .arora_call(call(vec![
      field(ids::add_instance::ANIM, Value::U32(7)),
      field(ids::add_instance::PLAYER, Value::U32(2)),
    ]))
    .expect("add_instance, arguments reversed");
  // player * 1000 + anim: a positional read would have produced 7002.
  assert_eq!(declared_order.ret, Value::U32(2007));
  assert_eq!(reversed_order.ret, Value::U32(2007));
}

#[test]
fn a_structure_argument_crosses_the_boundary_and_an_array_comes_back() {
  let mut engine = engine_with(vec![animation()]);
  let clip = AnimationClip {
    name: "wave".into(),
    duration: 1200,
    tracks: vec![
      AnimTrack {
        id: "a".into(),
        name: "A".into(),
        animatable_id: "face/a".into(),
        points: vec![],
      },
      AnimTrack {
        id: "b".into(),
        name: "B".into(),
        animatable_id: "face/b".into(),
        points: vec![],
      },
    ],
  };
  let loaded = engine
    .arora_call(Call {
      module_id: Some(ids::MODULE),
      id: ids::load_animation::FUNCTION,
      args: vec![field(ids::load_animation::CLIP, Value::from(clip))],
    })
    .expect("load_animation");
  assert!(matches!(loaded.ret, Value::U32(_)));

  let stepped = engine
    .arora_call(Call {
      module_id: Some(ids::MODULE),
      id: ids::step::FUNCTION,
      args: vec![field(ids::step::DT_NS, Value::U64(16_000_000))],
    })
    .expect("step");
  let Value::ArrayStructure { id, elements } = stepped.ret else {
    panic!("step returns an array of structures")
  };
  assert_eq!(id, TrackOutput::arora_type_id());
  assert_eq!(elements.len(), 2);
  // Each element decodes back to a TrackOutput through the derived conversion.
  let outputs: Vec<TrackOutput> = elements
    .into_iter()
    .map(|e| {
      TrackOutput::try_from(Value::Structure(arora_types::value::Structure {
        id,
        fields: e.fields,
      }))
      .unwrap()
    })
    .collect();
  assert_eq!(outputs[0].track_id, "a");
  assert_eq!(outputs[1].default_key, "face/b");
  assert_eq!(outputs[0].value, Value::U64(16_000_000));
}

#[test]
fn every_declared_function_is_discoverable() {
  let module = animation();
  let names: Vec<&str> = module
    .descriptions()
    .iter()
    .map(|d| d.name.as_str())
    .collect();
  for expected in [
    "load_animation",
    "create_player",
    "add_instance",
    "step",
    "seek",
    "player_states",
  ] {
    assert!(names.contains(&expected), "{expected} described");
  }
}

#[test]
fn a_user_type_is_pinned_at_its_declared_version() {
  use arora_types::record::ty::FrozenTy;
  let module = animation();
  let load = module
    .descriptions()
    .iter()
    .find(|d| d.name == "load_animation")
    .expect("described")
    .function
    .clone();
  let FrozenTy::FrozenScalar(clip) = &load.parameters[&ids::load_animation::CLIP].ty else {
    panic!("a record type")
  };
  assert_eq!(clip.reference.id, AnimationClip::arora_type_id());
  assert_eq!(clip.reference.version.to_string(), "1.1.0");
}
