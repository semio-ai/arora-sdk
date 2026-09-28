// A module id is a uuid.
#[arora_module::module(id = "0000")]
pub mod m {
    #[export(id = "5e1f0000-0000-4000-8000-000000000001")] pub fn f() {}
}

fn main() {}
