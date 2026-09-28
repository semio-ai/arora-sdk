// Two functions of one contract cannot share a name.
#[arora_module::contract]
pub trait Speak {
    #[export(id = "5e1f0000-0000-4000-8000-000000000001")] fn say(&mut self);
    #[export(id = "5e1f0000-0000-4000-8000-000000000002", name = "say")] fn whisper(&mut self);
}

fn main() {}
