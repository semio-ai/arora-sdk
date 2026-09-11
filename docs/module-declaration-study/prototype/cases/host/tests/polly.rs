//! polly (both declaration forms) registers host-side and dispatches through a
//! real engine: arguments by parameter id, functions described.

use arora_types::call::{Call, CallBridge};
use arora_types::record::module::frozen::Function;
use arora_types::record::ty::{FrozenTy, PrimitiveKind};
use arora_types::value::{StructureField, Value};
use arora_types::AroraType;
use case_host::{engine_with, polly, polly_outline};
use case_polly::polly as decl;
use case_polly::Status;

#[test]
fn say_dispatches_through_the_engine() {
  let mut engine = engine_with(vec![polly()]);
  let result = engine
    .arora_call(Call {
      module_id: Some(decl::ids::MODULE),
      id: decl::ids::say::FUNCTION,
      args: vec![StructureField {
        id: decl::ids::say::TEXT,
        value: Box::new(Value::String("hello".into())),
      }],
    })
    .expect("say dispatches");
  assert_eq!(
    Status::try_from(result.ret).expect("a Status"),
    Status::Success
  );
  assert!(result.mutated.is_empty());
}

#[test]
fn the_outline_declaration_is_the_same_module() {
  // Same ids, same header, same dispatch — only the declaration form differs.
  use case_polly_outline::polly as outline;
  assert_eq!(outline::ids::MODULE, decl::ids::MODULE);
  assert_eq!(outline::ids::say::TEXT, decl::ids::say::TEXT);
  let mut engine = engine_with(vec![polly_outline()]);
  let result = engine
    .arora_call(Call {
      module_id: Some(outline::ids::MODULE),
      id: outline::ids::hello_world::FUNCTION,
      args: vec![],
    })
    .expect("hello_world dispatches");
  assert_eq!(
    case_polly_outline::Status::try_from(result.ret).unwrap(),
    case_polly_outline::Status::Success
  );
}

#[test]
fn a_missing_parameter_fails_the_call_by_name() {
  let mut engine = engine_with(vec![polly()]);
  let error = engine
    .arora_call(Call {
      module_id: Some(decl::ids::MODULE),
      id: decl::ids::say::FUNCTION,
      args: vec![],
    })
    .expect_err("say without its text");
  assert!(
    error
      .to_string()
      .contains("missing parameter `text` of `say`"),
    "{error}"
  );
}

#[test]
fn an_argument_of_the_wrong_type_fails_the_call() {
  let mut engine = engine_with(vec![polly()]);
  let error = engine
    .arora_call(Call {
      module_id: Some(decl::ids::MODULE),
      id: decl::ids::say::FUNCTION,
      args: vec![StructureField {
        id: decl::ids::say::TEXT,
        value: Box::new(Value::U32(7)),
      }],
    })
    .expect_err("say with a number");
  assert!(
    error.to_string().contains("parameter `text` of `say`"),
    "{error}"
  );
}

#[test]
fn every_export_is_described_with_its_frozen_signature() {
  let module = polly();
  let described: Vec<(&str, &Function)> = module
    .descriptions()
    .iter()
    .map(|d| (d.name.as_str(), &d.function))
    .collect();
  assert_eq!(described.len(), 2);
  let say = described
    .iter()
    .find(|(n, _)| *n == "say")
    .expect("say described")
    .1;
  assert_eq!(say.parameter_ordering, vec![decl::ids::say::TEXT]);
  assert_eq!(
    say.parameters[&decl::ids::say::TEXT].ty,
    FrozenTy::from(PrimitiveKind::String)
  );
  let FrozenTy::FrozenScalar(ret) = &say.return_ty else {
    panic!("say returns a record type")
  };
  assert_eq!(ret.reference.id, Status::arora_type_id());
}
