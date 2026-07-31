// Generated code: lint hygiene is the generator's responsibility, not this
// repo's. Allow clippy/dead_code over the whole generated subtree.
#[allow(clippy::all, dead_code)]
mod arora_generated;

use arora_generated::behavior_tree::status::Status;
use serde::{Deserialize, Serialize};
use soloud::*;
use std::collections::HashMap;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

// Default provider: the same Vizij TTS cloud function the web demo
// (`@vizij/speech-react`'s `fetchVisemeData`) calls — AWS Polly behind an HTTP
// endpoint, so the module needs no AWS credentials. Overridable via `API_URL`
// (the same env var the standalone app reads) to point at another deployment.
const DEFAULT_API_BASE: &str = "https://us-central1-semio-vizij.cloudfunctions.net/api";
const DEFAULT_VOICE: &str = "Ruth";
// The rest / silence viseme (AWS Polly viseme set); written whenever nothing is
// speaking.
const SILENCE_VISEME: &str = "sil";

lazy_static::lazy_static! {
  // Reuse the caller's runtime if one is active (e.g. arora-cli), otherwise
  // create a dedicated one. Either way we only need a Handle for spawning.
  static ref TOKIO_HANDLE: tokio::runtime::Handle = tokio::runtime::Handle::try_current()
    .unwrap_or_else(|_| Box::leak(Box::new(tokio::runtime::Runtime::new().unwrap())).handle().clone());
  // Live utterances, keyed so concurrent `say`s do not share a slot (see the
  // poll-on-tick contract in docs/async-functions.md). Each run owns the
  // synthesis+playback `JoinHandle` and a shared viseme cell the playback task
  // advances at the audio playhead; `say` polls the handle and samples the cell
  // each tick.
  static ref RUNS: Mutex<HashMap<u64, Run>> = Mutex::new(HashMap::new());
}

/// One live utterance.
struct Run {
    /// The synthesis+playback task. A `JoinHandle` is a `Future`, so `say` drives
    /// the poll-on-tick contract by polling it directly; its output *is* the
    /// terminal `Status`.
    handle: JoinHandle<Status>,
    /// The viseme code at the audio playhead, published by the playback task and
    /// read by `say` each tick to fill the `viseme` out-parameter. `sil` = silence.
    viseme: Arc<Mutex<String>>,
}

/// The request body both TTS endpoints take.
#[derive(Serialize)]
struct TtsRequest<'a> {
    voice: &'a str,
    text: &'a str,
}

/// One AWS Polly speech mark. The endpoint returns them already filtered to
/// `type == "viseme"`; the other fields (`type`, `start`, `end`) are ignored.
#[derive(Deserialize)]
struct SpeechMark {
    /// Milliseconds into the audio.
    time: u64,
    /// The viseme code (AWS Polly viseme set).
    value: String,
}

#[derive(Deserialize)]
struct VisemeResponse {
    visemes: Vec<SpeechMark>,
}

/// The TTS provider base URL — `API_URL` or the demo cloud function by default.
fn api_base() -> String {
    std::env::var("API_URL").unwrap_or_else(|_| DEFAULT_API_BASE.to_string())
}

fn hello_world() -> Status {
    let mut viseme = None;
    say(Some("Hello, world!".to_string()), None, &mut viseme)
}

/// The run key for an utterance. The module ABI hands `say` only its arguments —
/// no per-invocation identity — so runs are keyed by content. Distinct utterances
/// no longer collide; same (text, voice) callers still share one run. True
/// per-caller keying needs a run id in the ABI — see the open question in
/// docs/async-functions.md.
fn utterance_key(text: &str, voice: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    voice.hash(&mut h);
    h.finish()
}

/// Speak `text` in `voice`, streaming the current viseme into `viseme`.
///
/// Long-running per the poll-on-tick contract (docs/async-functions.md): returns
/// `Running` while speaking and is re-ticked until `Success`/`Failure`. Each tick
/// it writes the viseme code at the audio playhead into `viseme` (the mutable
/// out-parameter). The module only *produces* the current viseme; mapping that
/// code to face poses is the caller's job — this function never touches the face.
fn say(text: Option<String>, voice: Option<String>, viseme: &mut Option<String>) -> Status {
    let text = match text {
        Some(text) => text,
        None => return Status::Failure,
    };
    let voice = voice.unwrap_or_else(|| DEFAULT_VOICE.to_string());
    let key = utterance_key(&text, &voice);
    let mut runs = match RUNS.lock() {
        Ok(runs) => runs,
        Err(_) => return Status::Failure,
    };

    // First tick for this utterance: spawn synthesis + playback off the tick
    // thread and record the run. Later ticks fall straight through to the poll.
    if !runs.contains_key(&key) {
        runs.insert(key, spawn_say(text, voice));
    }
    let run = runs.get_mut(&key).expect("just inserted");

    // Publish the viseme at the playhead (advanced by the playback task).
    if let Ok(current) = run.viseme.lock() {
        *viseme = Some(current.clone());
    }

    // Poll the run's `JoinHandle` — the `Future` — once. This call is one `poll`:
    // the tick loop is the executor, and a no-op waker suffices because the runtime
    // re-ticks us every step regardless of any wake-up. A completed `JoinHandle`
    // must never be polled again, so a terminal result drops the run.
    let mut cx = Context::from_waker(Waker::noop());
    match Future::poll(Pin::new(&mut run.handle), &mut cx) {
        // Still speaking — the runtime re-ticks me next step.
        Poll::Pending => Status::Running,
        // Terminal — silence the viseme and drop the run so a re-trigger respawns.
        Poll::Ready(Ok(status)) => {
            *viseme = Some(SILENCE_VISEME.to_string());
            runs.remove(&key);
            status
        }
        // The task panicked or was aborted: terminal failure.
        Poll::Ready(Err(_join_error)) => {
            *viseme = Some(SILENCE_VISEME.to_string());
            runs.remove(&key);
            Status::Failure
        }
    }
}

/// Spawn synthesis (the Vizij TTS provider) + playback on the Tokio runtime and
/// return the run. The task fetches audio + a viseme timeline, plays the audio,
/// and advances the shared viseme cell at the playhead. The heavy work never runs
/// on the tick thread; `say` only samples the cell and polls the handle.
fn spawn_say(text: String, voice: String) -> Run {
    let viseme = Arc::new(Mutex::new(SILENCE_VISEME.to_string()));
    let viseme_task = viseme.clone();
    let handle = TOKIO_HANDLE.spawn(async move {
        let base = api_base();
        let (audio, marks) = match synthesize(&base, &voice, &text).await {
            Some(pair) => pair,
            None => return Status::Failure,
        };

        // Play the whole utterance; advance the current viseme by the playhead.
        let sl = match Soloud::default() {
            Ok(sl) => sl,
            Err(_) => return Status::Failure,
        };
        let mut wav = audio::WavStream::default();
        if wav.load_mem(&audio).is_err() {
            return Status::Failure;
        }
        let start = Instant::now();
        sl.play(&wav);
        let mut next = 0usize;
        while sl.voice_count() > 0 {
            let elapsed = start.elapsed().as_millis() as u64;
            while next < marks.len() && marks[next].time <= elapsed {
                if let Ok(mut cur) = viseme_task.lock() {
                    *cur = marks[next].value.clone();
                }
                next += 1;
            }
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
        if let Ok(mut cur) = viseme_task.lock() {
            *cur = SILENCE_VISEME.to_string();
        }
        Status::Success
    });
    Run { handle, viseme }
}

/// Fetch audio (mp3) + the viseme timeline from the TTS provider — the same two
/// endpoints the web demo's `fetchVisemeData` uses. `None` on any transport or
/// decode error.
async fn synthesize(base: &str, voice: &str, text: &str) -> Option<(Vec<u8>, Vec<SpeechMark>)> {
    let client = reqwest::Client::new();
    let body = TtsRequest { voice, text };
    let audio = client
        .post(format!("{base}/tts/get-audio"))
        .json(&body)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .bytes()
        .await
        .ok()?
        .to_vec();
    let marks = client
        .post(format!("{base}/tts/get-visemes"))
        .json(&body)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json::<VisemeResponse>()
        .await
        .ok()?
        .visemes;
    Some((audio, marks))
}
