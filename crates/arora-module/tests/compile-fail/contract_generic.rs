// A contract's types are fixed.
#[arora_module::contract]
pub trait Speak<T> {
    #[export(id = "5e1f0000-0000-4000-8000-000000000001")]
    fn say(&mut self, #[param(id = "5e1f0000-0000-4000-8000-000000000002")] text: String);
}

fn main() {}
