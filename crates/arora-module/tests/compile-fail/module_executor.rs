// A declaration does not name its executor.
#[arora_module::module(id = "5e1f0000-0000-4000-8000-000000000064", executor = "wasm")]
pub mod m {
    #[export(id = "5e1f0000-0000-4000-8000-000000000001")] pub fn f() {}
}

fn main() {}
