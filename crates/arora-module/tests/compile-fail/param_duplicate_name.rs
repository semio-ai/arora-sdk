// Two parameters of one function cannot share a name.
#[arora_module::export(id = "5e1f0000-0000-4000-8000-000000000001")]
pub fn f(#[param(id = "5e1f0000-0000-4000-8000-000000000002")] x: u32, #[param(id = "5e1f0000-0000-4000-8000-000000000003", name = "x")] y: u32) { unimplemented!() }

fn main() {}
