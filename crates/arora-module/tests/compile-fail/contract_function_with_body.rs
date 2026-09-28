// Each implementation provides the body.
#[arora_module::contract]
pub trait Speak {
    #[export(id = "5e1f0000-0000-4000-8000-000000000001")] fn say(&mut self) {}
}

fn main() {}
