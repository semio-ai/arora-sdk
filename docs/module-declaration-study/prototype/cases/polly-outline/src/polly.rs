//! The module's file: exported functions, and the declaration at the bottom
//! naming them — the attribute form cannot see this file from `mod polly;`.

use super::Status;
use arora_module_derive::{declare_module, export};

#[export(id = "e5a41333-4848-411f-878c-f1d662ebb4a0")]
pub fn hello_world() -> Status {
  say("Hello, world!".to_string())
}

#[export(id = "e1b4bda7-1c7b-4322-b9a0-552201b8a011")]
pub fn say(#[param(id = "fb3787f2-2151-49ce-8b61-6274984558ea")] text: String) -> Status {
  if text.is_empty() {
    Status::Failure
  } else {
    Status::Success
  }
}

declare_module! {
  id = "a1a6bb9a-334f-4617-a764-9a55817039d8",
  name = "polly",
  version = "0.1.0",
  author = "Semio",
  license = "Proprietary",
  description = "AWS Polly support module",
  executable_mime = "application/x-binary",
  exports = [hello_world, say]
}
