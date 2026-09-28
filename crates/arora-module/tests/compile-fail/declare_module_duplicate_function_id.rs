// The aggregate checks ids it cannot see written: two functions of one module
// cannot share an id.
mod m {
    use arora_module::{declare_module, export};

    #[export(id = "5e1f0000-0000-4000-8000-000000000001")]
    pub fn f() {}

    #[export(id = "5e1f0000-0000-4000-8000-000000000001")]
    pub fn g() {}

    declare_module! { id = "5e1f0000-0000-4000-8000-000000000064", name = "m", exports = [f, g] }
}

fn main() {}
