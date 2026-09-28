// A contract function takes `&mut self` first.
#[arora_module::contract]
pub trait Speak {
    #[export(id = "5e1f0000-0000-4000-8000-000000000001")] fn say(#[param(id = "5e1f0000-0000-4000-8000-000000000002")] text: String);
}

fn main() {}
