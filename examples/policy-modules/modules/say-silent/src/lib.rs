//! The say contract (see `say-piper`) without a speaker: what a test, an
//! offline run or a device with no speaker registers in place of a speaking
//! provider; the tree does not change.
//!
//! It keeps a speaking provider's timing and semantics, counted in ticks: a
//! sentence is `Running` for [`TICKS_PER_CHARACTER`] ticks per character,
//! then logged ([`spoken`]) and `Success` — for as long as the leaf keeps
//! being ticked. A new sentence forgets the finished ones, so a phase
//! entered again speaks again, and abandons one still under way, unlogged.

use std::sync::Mutex;

use arora_behavior::Status;

/// The rest shape the viseme holds.
pub const SILENCE_VISEME: &str = "sil";

/// How many ticks a character takes: at a 50 Hz tick, ten characters a
/// second, a speaker's pace.
pub const TICKS_PER_CHARACTER: usize = 5;

#[arora_module::module(
    id = "e903dd63-884c-43d5-823d-d292c49aaf9d",
    name = "silent-say",
    version = "0.1.0",
    author = "Semio",
    license = "MIT",
    description = "The say contract without a speaker: a sentence takes a speaker's time, then is logged"
)]
pub mod silent {
    use super::*;

    /// "Say" `text`: `Running` for [`TICKS_PER_CHARACTER`] ticks per
    /// character, then logged (see [`spoken`]) and `Success` while it keeps
    /// being ticked. `viseme` stays at rest; an empty text succeeds at once.
    #[export(id = "77bf2798-e7ce-47c6-a45c-3c2e9ba1837d")]
    pub fn say(
        #[param(id = "881dc182-d4ba-4ea0-9e81-f4eddab6f669")] text: String,
        #[param(id = "f56ca142-db46-4c58-bc44-7896c4b54d5c")] voice: String,
        #[param(id = "a1fbf58b-bf66-44a6-a503-9d9078ee5755")] viseme: &mut String,
    ) -> Status {
        let _ = voice;
        *viseme = SILENCE_VISEME.to_string();
        if text.is_empty() {
            return Status::Success;
        }
        let mut runs = RUNS.lock().unwrap();
        if !runs.iter().any(|run| run.text == text) {
            // A new sentence: the finished ones are forgotten, and one still
            // under way is abandoned.
            runs.clear();
            runs.push(Run {
                text: text.clone(),
                ticks: 0,
            });
        }
        let run = runs
            .iter_mut()
            .find(|run| run.text == text)
            .expect("present");
        let length = text.chars().count() * TICKS_PER_CHARACTER;
        if run.ticks >= length {
            return Status::Success;
        }
        run.ticks += 1;
        if run.ticks < length {
            return Status::Running;
        }
        log::info!("say (silent): {text}");
        SPOKEN.lock().unwrap().push(text);
        Status::Success
    }
}

struct Run {
    text: String,
    ticks: usize,
}

static RUNS: Mutex<Vec<Run>> = Mutex::new(Vec::new());
static SPOKEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Forget every sentence, finished or under way, and empty the log: what a
/// test does before a fresh device, the provider's state being the
/// process's.
pub fn reset() {
    RUNS.lock().unwrap().clear();
    SPOKEN.lock().unwrap().clear();
}

/// Every sentence said to its end since the last call, oldest first; taking
/// them empties the log.
pub fn spoken() -> Vec<String> {
    std::mem::take(&mut SPOKEN.lock().unwrap())
}

#[cfg(test)]
mod tests {
    use super::silent::say;
    use super::*;

    #[test]
    fn takes_a_speakers_time_then_latches_success() {
        let _ = spoken();
        let mut viseme = String::new();
        let mut tick = |text: &str| say(text.into(), String::new(), &mut viseme);
        // Another sentence abandons "Hi" unlogged.
        assert_eq!(tick("Hi"), Status::Running);
        for _ in 1.."Hello".len() * TICKS_PER_CHARACTER {
            assert_eq!(tick("Hello"), Status::Running);
        }
        assert_eq!(tick("Hello"), Status::Success);
        assert_eq!(tick("Hello"), Status::Success, "latched while ticked");
        assert_eq!(spoken(), vec!["Hello".to_string()], "said once");
        // A new sentence forgets it: "Hello" speaks again after "Bye".
        assert_eq!(tick("Bye"), Status::Running);
        assert_eq!(tick("Hello"), Status::Running);
        assert_eq!(tick(""), Status::Success);
        assert_eq!(viseme, SILENCE_VISEME);
    }
}
