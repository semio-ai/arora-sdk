// A contract function's parameters are checked as a module function's are.
#[arora_module::contract]
pub trait Speak {
    #[export(id = "5e1f0000-0000-4000-8000-000000000001")] fn say(&mut self, #[param(id = "5e1f0000-0000-4000-8000-000000000002")] text: String, #[param(id = "5e1f0000-0000-4000-8000-000000000002")] voice: String);
}

fn main() {}
