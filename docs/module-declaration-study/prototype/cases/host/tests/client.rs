//! The consumer side: typed call stubs — from the declaring crate
//! (`polly::client::say`), and from the header alone (`case_polly_client`).

use arora_types::value::Value;
use case_host::{engine_with, polly};
use case_polly::polly as decl;
use case_polly::Status;

#[test]
fn a_declared_module_is_its_own_client_interface() {
  let mut engine = engine_with(vec![polly()]);
  let status = decl::client::say(&mut engine, "hello".to_string()).expect("say");
  assert_eq!(status, Status::Success);
  let status = decl::client::hello_world(&mut engine).expect("hello_world");
  assert_eq!(status, Status::Success);
}

#[test]
fn a_client_built_from_the_header_alone_calls_the_same_module() {
  use case_polly_client as client;
  assert_eq!(client::ids::MODULE, decl::ids::MODULE);
  assert_eq!(client::ids::say::TEXT, decl::ids::say::TEXT);
  assert_eq!(client::NAME, "polly");
  let mut engine = engine_with(vec![polly()]);
  assert_eq!(
    client::say(&mut engine, "hello".to_string()).expect("say"),
    Status::Success
  );
  assert_eq!(
    client::say(&mut engine, String::new()).expect("say"),
    Status::Failure
  );
  let _ = Value::Unit;
}

#[test]
fn a_mutable_parameter_is_read_back_by_the_stub() {
  use case_bt_nodes::nodes;
  let mut engine = engine_with(vec![case_host::bt_nodes()]);
  let mut variable = "old".to_string();
  let status =
    nodes::client::set_str(&mut engine, &mut variable, "new".to_string()).expect("set_str");
  assert_eq!(status, case_bt_nodes::Status::Success);
  assert_eq!(variable, "new");
}
