// Two parameters of one function cannot share an id, however it is spelled.
#[arora_module::export(id = "5e1f0000-0000-4000-8000-000000000001")]
pub fn f(#[param(id = "5e1f0000-0000-4000-8000-000000000002")] x: u32, #[param(id = "5E1F0000-0000-4000-8000-000000000002")] y: u32) { unimplemented!() }

fn main() {}
