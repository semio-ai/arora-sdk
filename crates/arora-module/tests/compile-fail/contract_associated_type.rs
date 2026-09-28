// A contract declares functions only.
#[arora_module::contract]
pub trait Speak {
    type Voice;
    #[export(id = "5e1f0000-0000-4000-8000-000000000001")] fn say(&mut self);
}

fn main() {}
