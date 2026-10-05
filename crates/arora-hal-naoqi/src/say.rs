//! `Say` in the robot's own voice: the speech contract of Arora's behavior trees, provided
//! by `ALTextToSpeech`.
//!
//! The contract is `say(text, voice, viseme: &mut String) -> Status` under fixed function
//! and parameter ids, one signature for every provider (`say-silent` and `say-piper` in
//! `examples/policy-modules` carry the same ids), so a tree names `Say` without knowing who
//! speaks. Here the robot speaks:
//!
//! - `Running` while `ALTextToSpeech.say` has not returned, `Success` once it has, and for
//!   as long as the leaf keeps being ticked afterwards (an episodic leaf's convention: it
//!   pairs with an action in a `Parallel`), `Failure` when NAOqi refuses the sentence or no
//!   robot voice is [installed](install);
//! - sentences are keyed by their text (a module call carries no run id): a new text stops
//!   the one under way and forgets the finished ones;
//! - a halt is silence: a sentence whose leaf goes [`IDLE_STOP`] unticked is stopped
//!   (`ALTextToSpeech.stopAll`) and forgotten, so a branch entered anew speaks again;
//! - `viseme` stays at rest ([`SILENCE_VISEME`]): NAO and Pepper have no mouth to shape.
//!
//! `voice` is accepted for the contract's sake and ignored: the robot speaks with the voice
//! and language its `ALTextToSpeech` is set to.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use arora_behavior::Status;

use crate::naoqi_hal::{Utterance, Voice};

/// The rest shape `viseme` holds.
pub const SILENCE_VISEME: &str = "sil";

/// How long a sentence's leaf goes unticked before the sentence is stopped and forgotten.
pub const IDLE_STOP: Duration = Duration::from_millis(250);

#[arora_module::module(
    id = "9d67f9c8-78c6-41bc-bcfb-c72423c9f393",
    name = "naoqi-say",
    version = "0.1.0",
    author = "Semio",
    license = "MIT",
    description = "The say contract in a NAOqi robot's voice (ALTextToSpeech)"
)]
pub mod naoqi_say {
    use super::*;

    /// Say `text` with the robot's voice: `Running` while it speaks, then `Success` while
    /// ticked; `Failure` when the robot cannot say it. `viseme` stays at rest; an empty text
    /// succeeds at once.
    #[export(id = "77bf2798-e7ce-47c6-a45c-3c2e9ba1837d")]
    pub fn say(
        #[param(id = "881dc182-d4ba-4ea0-9e81-f4eddab6f669")] text: String,
        #[param(id = "f56ca142-db46-4c58-bc44-7896c4b54d5c")] voice: String,
        #[param(id = "a1fbf58b-bf66-44a6-a503-9d9078ee5755")] viseme: &mut String,
    ) -> Status {
        let _ = voice;
        *viseme = SILENCE_VISEME.to_string();
        lock(&SPEAKING).tick(&text, Instant::now())
    }
}

/// Give the `say` leaves the robot's voice ([`NaoqiHal::voice`](crate::NaoqiHal::voice)).
/// Until a voice is installed, every non-empty sentence fails.
pub fn install(voice: Voice) {
    let mut speaking = lock(&SPEAKING);
    speaking.stop_all();
    speaking.voice = Some(voice);
}

/// The sentences under way or said, and who says them. The provider's state is the
/// process's, as the module ABI hands its functions no instance.
#[derive(Default)]
struct Speaking {
    voice: Option<Voice>,
    sentences: HashMap<String, Sentence>,
    warned_mute: bool,
}

struct Sentence {
    utterance: Utterance,
    last_tick: Instant,
}

static SPEAKING: LazyLock<Mutex<Speaking>> = LazyLock::new(Mutex::default);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Speaking {
    fn tick(&mut self, text: &str, now: Instant) -> Status {
        // Leaves nobody ticks any more were halted: their sentences stop.
        let halted: Vec<String> = self
            .sentences
            .iter()
            .filter(|(_, sentence)| now.duration_since(sentence.last_tick) > IDLE_STOP)
            .map(|(text, _)| text.clone())
            .collect();
        for text in halted {
            if let Some(sentence) = self.sentences.remove(&text) {
                if sentence.utterance.status() == Status::Running {
                    self.stop_voice();
                }
            }
        }
        if text.is_empty() {
            return Status::Success;
        }
        let Some(voice) = self.voice.clone() else {
            if !self.warned_mute {
                log::warn!("say: no robot voice installed, \"{text}\" is not said");
                self.warned_mute = true;
            }
            return Status::Failure;
        };
        if !self.sentences.contains_key(text) {
            // A new sentence takes the voice over: the one under way stops, the finished
            // ones are forgotten.
            self.stop_all();
            self.sentences.insert(
                text.to_string(),
                Sentence {
                    utterance: voice.say(text.to_string()),
                    last_tick: now,
                },
            );
        }
        let sentence = self.sentences.get_mut(text).expect("present");
        sentence.last_tick = now;
        sentence.utterance.status()
    }

    /// Stop the sentence under way, if any, and forget them all.
    fn stop_all(&mut self) {
        if self
            .sentences
            .values()
            .any(|sentence| sentence.utterance.status() == Status::Running)
        {
            self.stop_voice();
        }
        self.sentences.clear();
    }

    fn stop_voice(&self) {
        if let Some(voice) = &self.voice {
            voice.stop();
        }
    }
}
