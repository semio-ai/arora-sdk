// A version is `major.minor.patch`.
#[arora_module::module(id = "5e1f0000-0000-4000-8000-000000000064", version = "1.0")]
pub mod m {
    #[export(id = "5e1f0000-0000-4000-8000-000000000001")] pub fn f() {}
}

fn main() {}
