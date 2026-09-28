// An id is a uuid.
#[arora_module::export(id = "not-a-uuid")]
pub fn f() {}

fn main() {}
