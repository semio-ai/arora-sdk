//! Mutable parameters come back as mutated arguments; an array of structures
//! goes in as `children`.

use arora_types::call::{Call, CallBridge};
use arora_types::value::{StructureField, Value};
use case_bt_nodes::nodes::ids;
use case_bt_nodes::{Status, TickId};
use case_host::{bt_nodes, engine_with};

fn field(id: uuid::Uuid, value: Value) -> StructureField {
  StructureField {
    id,
    value: Box::new(value),
  }
}

#[test]
fn a_mutable_parameter_is_written_back_as_a_mutated_argument() {
  let mut engine = engine_with(vec![bt_nodes()]);
  let result = engine
    .arora_call(Call {
      module_id: Some(ids::MODULE),
      id: ids::set_str::FUNCTION,
      args: vec![
        field(ids::set_str::VALUE, Value::String("new".into())),
        field(ids::set_str::VARIABLE, Value::String("old".into())),
      ],
    })
    .expect("set_str");
  assert_eq!(Status::try_from(result.ret).unwrap(), Status::Success);
  // Exactly the mutable parameter, under its id, with its new value.
  assert_eq!(
    result.mutated,
    vec![field(ids::set_str::VARIABLE, Value::String("new".into()))]
  );
}

#[test]
fn an_unset_variable_comes_back_empty() {
  let mut engine = engine_with(vec![bt_nodes()]);
  let result = engine
    .arora_call(Call {
      module_id: Some(ids::MODULE),
      id: ids::unset_str::FUNCTION,
      args: vec![field(
        ids::unset_str::VARIABLE,
        Value::String("something".into()),
      )],
    })
    .expect("unset_str");
  assert_eq!(
    result.mutated,
    vec![field(
      ids::unset_str::VARIABLE,
      Value::String(String::new())
    )]
  );
}

#[test]
fn children_arrive_as_an_array_of_tick_ids() {
  let mut engine = engine_with(vec![bt_nodes()]);
  // An array of structures on the wire: the element structures without their id.
  let children = Value::ArrayStructure {
    id: <TickId as arora_types::AroraType>::arora_type_id(),
    elements: [7, 9]
      .into_iter()
      .map(|callable_id| {
        let Value::Structure(s) = Value::from(TickId { callable_id }) else {
          unreachable!()
        };
        arora_types::value::StructureWithoutId { fields: s.fields }
      })
      .collect(),
  };
  let result = engine
    .arora_call(Call {
      module_id: Some(ids::MODULE),
      id: ids::seq::FUNCTION,
      args: vec![field(ids::seq::CHILDREN, children)],
    })
    .expect("seq");
  assert_eq!(Status::try_from(result.ret).unwrap(), Status::Success);
}
