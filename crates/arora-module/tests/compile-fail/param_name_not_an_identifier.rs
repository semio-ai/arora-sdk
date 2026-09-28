// A parameter's name spells its id constant.
#[arora_module::export(id = "5e1f0000-0000-4000-8000-000000000001")]
pub fn f(#[param(id = "5e1f0000-0000-4000-8000-000000000002", name = "frame rate")] x: u32) { unimplemented!() }

fn main() {}
