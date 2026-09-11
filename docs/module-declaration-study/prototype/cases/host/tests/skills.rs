//! The skill modules dispatch like any other; the mutable viseme comes back.

use arora_types::call::{Call, CallBridge};
use arora_types::value::{StructureField, Value};
use case_host::{engine_with, host_module};
use case_vizij_skills::{gaze, tts, Status};

fn field(id: uuid::Uuid, value: Value) -> StructureField {
  StructureField {
    id,
    value: Box::new(value),
  }
}

#[test]
fn say_reports_its_viseme_through_the_mutable_parameter() {
  let mut engine = engine_with(vec![host_module::<tts::Module>()]);
  let result = engine
    .arora_call(Call {
      module_id: Some(tts::ids::MODULE),
      id: tts::ids::say::FUNCTION,
      args: vec![
        field(tts::ids::say::TEXT, Value::String("hello".into())),
        field(tts::ids::say::VOICE, Value::String("lessac".into())),
        field(tts::ids::say::VISEME, Value::String(String::new())),
      ],
    })
    .expect("say");
  assert_eq!(Status::try_from(result.ret).unwrap(), Status::Running);
  assert_eq!(
    result.mutated,
    vec![field(tts::ids::say::VISEME, Value::String("aa".into()))]
  );
}

#[test]
fn look_at_takes_an_f32_array_through_the_stub() {
  let mut engine = engine_with(vec![host_module::<gaze::Module>()]);
  let status = gaze::client::look_at(
    &mut engine,
    "track".into(),
    vec![0.0, 1.0, 2.0],
    "face".into(),
  )
  .expect("look_at");
  assert_eq!(status, Status::Running);
}
