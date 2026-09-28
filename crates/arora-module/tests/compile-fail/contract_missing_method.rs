// rustc checks that an implementation provides every function of its contract.
#[arora_module::contract]
pub trait Speak {
    #[export(id = "5e1f0000-0000-4000-8000-000000000001")]
    fn say(&mut self, #[param(id = "5e1f0000-0000-4000-8000-000000000002")] text: String);

    #[export(id = "5e1f0000-0000-4000-8000-000000000003")]
    fn whisper(&mut self, #[param(id = "5e1f0000-0000-4000-8000-000000000004")] text: String);
}

struct Loud;

impl Speak for Loud {
    fn say(&mut self, _text: String) {}
}

fn main() {}
