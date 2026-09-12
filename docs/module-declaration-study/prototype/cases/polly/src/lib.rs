//! `modules/polly` declared in Rust, on an inline module. The declaration
//! mirrors `modules/polly/module.yaml` id for id; the tests hold it to the
//! header the SDK generator produced from that YAML.

use arora_module_derive::AroraValue;
use arora_types::AroraType;

/// The behavior `Status`, exactly as `arora_behavior::Status` declares it
/// (pinned enumeration and variant ids). `AroraValue` derives the `Value`
/// conversions `arora_behavior::Status` writes by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, AroraType, AroraValue)]
#[arora(id = "325a5767-e344-4532-860e-0749bcf2e428")]
pub enum Status {
  #[arora(id = "766e9e9a-446d-4e46-83e6-14b7ca101169")]
  Success,
  #[arora(id = "2468f46c-bb60-425c-9a4d-9ad326ccc7e2")]
  Failure,
  #[arora(id = "acd79ec6-0c44-401a-82f8-5da5422d3eec")]
  Running,
}

#[arora_module_derive::module(
  id = "a1a6bb9a-334f-4617-a764-9a55817039d8",
  name = "polly",
  version = "0.1.0",
  author = "Semio",
  license = "Proprietary",
  description = "AWS Polly support module",
  executable_mime = "application/x-binary"
)]
pub mod polly {
  use super::Status;
  use arora_module_derive::export;

  #[export(id = "e5a41333-4848-411f-878c-f1d662ebb4a0")]
  pub fn hello_world() -> Status {
    say("Hello, world!".to_string())
  }

  /// A stand-in for the synthesis: succeeds on a non-empty text.
  #[export(id = "e1b4bda7-1c7b-4322-b9a0-552201b8a011")]
  pub fn say(#[param(id = "fb3787f2-2151-49ce-8b61-6274984558ea")] text: String) -> Status {
    if text.is_empty() {
      Status::Failure
    } else {
      Status::Success
    }
  }
}
