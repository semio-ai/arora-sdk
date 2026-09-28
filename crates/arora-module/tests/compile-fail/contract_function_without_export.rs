// Every function of a contract pins its id.
#[arora_module::contract]
pub trait Speak {
    fn say(&mut self);
}

fn main() {}
