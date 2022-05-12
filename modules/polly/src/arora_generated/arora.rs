#[link(wasm_import_module = "env")]
extern "C" {
  pub fn arora_dispatch(module_id: i32, method_id: i32, arg: i32) -> i32;
  pub fn arora_dispatch_indirect(callable_id: u64) -> i32;
}
