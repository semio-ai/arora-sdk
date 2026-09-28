// Two functions of one module cannot share an id.
#[arora_module::module(id = "5e1f0000-0000-4000-8000-000000000064")]
pub mod m {
    #[export(id = "5e1f0000-0000-4000-8000-000000000001")] pub fn f() {}
    #[export(id = "5e1f0000-0000-4000-8000-000000000001")] pub fn g() {}
}

fn main() {}
