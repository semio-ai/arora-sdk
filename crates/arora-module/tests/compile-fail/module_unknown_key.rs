// Only the header's fields are module attributes.
#[arora_module::module(id = "5e1f0000-0000-4000-8000-000000000064", homepage = "https://example.com")]
pub mod m {
    #[export(id = "5e1f0000-0000-4000-8000-000000000001")] pub fn f() {}
}

fn main() {}
