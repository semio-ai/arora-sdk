//! The say contract (see the `say` crate) without a speaker: each sentence
//! is logged ([`spoken`]) and succeeds at once. What a test, an offline run
//! or a device with no speaker registers in place of a speaking provider;
//! the tree does not change.

use std::sync::Mutex;

use arora_behavior::Status;

/// The rest shape the viseme holds.
pub const SILENCE_VISEME: &str = "sil";

#[arora_module::module(
    id = "e903dd63-884c-43d5-823d-d292c49aaf9d",
    name = "silent-say",
    version = "0.1.0",
    author = "Semio",
    license = "MIT",
    description = "The say contract without a speaker: each sentence is logged and succeeds at once"
)]
pub mod silent {
    use super::*;

    /// Log `text` (see [`spoken`]) and succeed; `viseme` stays at rest. An
    /// empty text logs nothing.
    #[export(id = "77bf2798-e7ce-47c6-a45c-3c2e9ba1837d")]
    pub fn say(
        #[param(id = "881dc182-d4ba-4ea0-9e81-f4eddab6f669")] text: String,
        #[param(id = "f56ca142-db46-4c58-bc44-7896c4b54d5c")] voice: String,
        #[param(id = "a1fbf58b-bf66-44a6-a503-9d9078ee5755")] viseme: &mut String,
    ) -> Status {
        let _ = voice;
        if !text.is_empty() {
            log::info!("say (silent): {text}");
            SPOKEN.lock().unwrap().push(text);
        }
        *viseme = SILENCE_VISEME.to_string();
        Status::Success
    }
}

static SPOKEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Every sentence said since the last call, oldest first; taking them
/// empties the log.
pub fn spoken() -> Vec<String> {
    std::mem::take(&mut SPOKEN.lock().unwrap())
}

#[cfg(test)]
mod tests {
    use super::silent::say;
    use super::*;

    #[test]
    fn logs_and_succeeds() {
        let _ = spoken();
        let mut viseme = String::new();
        assert_eq!(
            say("Hello".into(), String::new(), &mut viseme),
            Status::Success
        );
        assert_eq!(
            say(String::new(), String::new(), &mut viseme),
            Status::Success
        );
        assert_eq!(spoken(), vec!["Hello".to_string()]);
        assert_eq!(viseme, SILENCE_VISEME);
    }
}
