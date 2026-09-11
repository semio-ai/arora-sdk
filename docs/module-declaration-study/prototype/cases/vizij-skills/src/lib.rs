//! The two skills vizij registers as host modules today with hand-built
//! signatures: `say(text, voice, &mut viseme) -> Status` (vizij-arora-tts,
//! tts_piper) and `look_at(policy, target: [f32], frame) -> Status` (gaze).
//! Ids are the ones `vizij-arora-host::skills` and `speech.rs` carry; gaze's
//! were name-hashed and are pinned here.

use arora_module_derive::AroraValue;
use arora_types::AroraType;

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
  id = "31ca2243-5719-4862-aa57-c30d27cab62e",
  name = "tts-piper",
  version = "1.0.0"
)]
pub mod tts {
  use super::Status;
  use arora_module_derive::export;

  /// Speaks `text`; reports the phoneme at the playhead through `viseme`.
  #[export(id = "77bf2798-e7ce-47c6-a45c-3c2e9ba1837d")]
  pub fn say(
    #[param(id = "881dc182-d4ba-4ea0-9e81-f4eddab6f669")] text: String,
    #[param(id = "f56ca142-db46-4c58-bc44-7896c4b54d5c")] voice: String,
    #[param(id = "a1fbf58b-bf66-44a6-a503-9d9078ee5755")] viseme: &mut String,
  ) -> Status {
    let _ = voice;
    *viseme = if text.is_empty() {
      "sil".into()
    } else {
      "aa".into()
    };
    if text.is_empty() {
      Status::Failure
    } else {
      Status::Running
    }
  }
}

#[arora_module_derive::module(
  id = "67617a65-0000-4000-8000-000000000000",
  name = "gaze",
  version = "1.0.0"
)]
pub mod gaze {
  use super::Status;
  use arora_module_derive::export;

  #[export(id = "6c6f6f6b-0000-4000-8000-000000000000")]
  pub fn look_at(
    #[param(id = "706f6c69-0000-4000-8000-000000000000")] policy: String,
    #[param(id = "74617267-0000-4000-8000-000000000000")] target: Vec<f32>,
    #[param(id = "6672616d-0000-4000-8000-000000000000")] frame: String,
  ) -> Status {
    let _ = (policy, frame);
    if target.len() == 3 {
      Status::Running
    } else {
      Status::Failure
    }
  }
}
