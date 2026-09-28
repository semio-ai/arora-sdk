// `declare_module!` names its exports.
mod m {
    use arora_module::{declare_module, export};

    #[export(id = "5e1f0000-0000-4000-8000-000000000001")]
    pub fn f() {}

    declare_module! { id = "5e1f0000-0000-4000-8000-000000000064", name = "m" }
}

fn main() {}
