//! Speech as a behavior leaf: `say(text, voice) -> Status`, streaming the
//! mouth shape at the audio playhead through its `viseme` out-parameter.
//!
//! The **contract** is Vizij's speech skill's `say` (vizij-rs,
//! `vizij-arora-host::skills`): the same function and parameter ids and the
//! same signature, so a tree written against it runs whichever provider a
//! device registers, here or in Vizij. A provider is a module implementing
//! the contract under its own module id; a device registers one.
//!
//! - [`cloud`] is Vizij's TTS cloud provider — AWS Polly behind an HTTP
//!   endpoint, no credentials in the app — with its module id. Its native
//!   producer is a copy of vizij-rs `vizij-arora-tts` 4.0.0: until the say
//!   contract and its providers live outside Vizij, that crate cannot be
//!   depended on without Vizij's face crates, and a fix there is a fix here.
//! - `say-silent` (the sibling crate) speaks instantly into a log: tests,
//!   offline runs, a device with no speaker.
//!
//! `say` is a poll-on-tick action (the arora-sdk `docs/async-functions.md`
//! contract): re-invoked each tick while `Running`. Synthesis and playback
//! run off the tick; the tick only polls. A halt is silence: a run the tree
//! stops ticking goes quiet within [`IDLE_STOP`] (or [`HALT_TICKS`] of a
//! slower tick), which is also how a newer sentence cuts off a stale one.
//! The module ABI hands a call no run id, so runs are keyed by their text
//! and voice: the same sentence asked twice at once is one utterance.
//!
//! It is a host module, never a wasm guest: it opens a network connection
//! and an audio device, which a guest has no access to.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use arora_behavior::Status;
use serde::{Deserialize, Serialize};

/// The rest shape: the viseme written while nothing is spoken.
pub const SILENCE_VISEME: &str = "sil";

/// The Vizij TTS cloud function.
pub const DEFAULT_API_BASE: &str = "https://us-central1-semio-vizij.cloudfunctions.net/api";

/// The voice when a tree names none.
pub const DEFAULT_VOICE: &str = "Ruth";

/// How long a producer keeps playing without a `say` tick before it treats
/// the run as halted and stops the audio, when the ticks come at least this
/// often; slower ticks widen the bound to [`HALT_TICKS`] of their interval.
pub const IDLE_STOP: Duration = Duration::from_millis(250);

/// How many of the ticker's own intervals without a tick mean a halt, where
/// that is longer than [`IDLE_STOP`].
pub const HALT_TICKS: u32 = 4;

/// Vizij's TTS cloud provider. The function and parameter ids are the say
/// contract's.
#[arora_module::module(
    id = "4f6f0b0a-62cb-4a1f-ab0d-08f283485091",
    name = "vizij-tts",
    version = "0.1.0",
    author = "Semio",
    license = "MIT",
    description = "Speech through the Vizij TTS cloud function (AWS Polly), played on the host's audio output"
)]
pub mod cloud {
    use super::*;

    /// Speak `text` in `voice` (an AWS Polly voice; [`DEFAULT_VOICE`] when
    /// empty): `Running` while it is fetched and played, `Success` when the
    /// audio ends, `Failure` when synthesis or playback fails or the run was
    /// halted. `viseme` holds the mouth shape at the playhead, one of the face
    /// standard's (`PP`, `aa`, … and [`SILENCE_VISEME`]). An empty text says
    /// nothing and succeeds.
    #[export(id = "77bf2798-e7ce-47c6-a45c-3c2e9ba1837d")]
    pub fn say(
        #[param(id = "881dc182-d4ba-4ea0-9e81-f4eddab6f669")] text: String,
        #[param(id = "f56ca142-db46-4c58-bc44-7896c4b54d5c")] voice: String,
        #[param(id = "a1fbf58b-bf66-44a6-a503-9d9078ee5755")] viseme: &mut String,
    ) -> Status {
        super::cloud_say(text, voice, viseme)
    }
}

// ─── The cloud provider ───────────────────────────────────────────────────────

/// The live runs, keyed by content, and the ticker's pulse.
struct Provider {
    runs: HashMap<u64, Run>,
    /// Beaten on every tick, whichever run: where the tick interval is
    /// learned, so a new run's halt bound is right from its first tick.
    ticks: Pulse,
}

static PROVIDER: LazyLock<Mutex<Provider>> = LazyLock::new(|| {
    Mutex::new(Provider {
        runs: HashMap::new(),
        ticks: Pulse::new(),
    })
});

fn cloud_say(text: String, voice: String, viseme: &mut String) -> Status {
    let mut provider = PROVIDER.lock().unwrap();
    let Provider { runs, ticks } = &mut *provider;
    ticks.beat();
    // A run nobody ticks any more was halted; its producer has stopped or is
    // stopping on its own, and its slot goes.
    runs.retain(|_, run| !is_halted(&run.pulse));
    if text.is_empty() {
        *viseme = SILENCE_VISEME.to_string();
        return Status::Success;
    }
    let voice = if voice.is_empty() {
        DEFAULT_VOICE.to_string()
    } else {
        voice
    };
    let key = utterance_key(&text, &voice);
    // First tick: spawn synthesis and playback off the tick. Later ticks find
    // the run, refresh its pulse and poll it.
    let run = runs
        .entry(key)
        .or_insert_with(|| spawn(DEFAULT_API_BASE, text, voice, ticks.sharing_interval()));
    run.pulse.beat();
    match poll(run) {
        Observed::Running(shape) => {
            *viseme = shape.to_string();
            Status::Running
        }
        Observed::Ended(status) => {
            runs.remove(&key);
            *viseme = SILENCE_VISEME.to_string();
            status
        }
    }
}

/// The run key for an utterance: its content, the ABI carrying no run id.
fn utterance_key(text: &str, voice: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    voice.hash(&mut hasher);
    hasher.finish()
}

/// What one tick observes of a run.
enum Observed {
    Running(&'static str),
    Ended(Status),
}

/// The last instant a run was ticked, shared between the tick and the
/// producer — the producer's only sign that the run is still wanted — and
/// the interval the ticks come at, so the halt bound follows a slow ticker.
#[derive(Clone)]
struct Pulse {
    last: Arc<Mutex<f64>>,
    /// The tick interval in milliseconds: the widest recent gap, forgetting
    /// a one-off hiccup over the ticks that follow; shared by the pulses of
    /// one ticker.
    interval: Arc<Mutex<f64>>,
}

impl Pulse {
    fn new() -> Self {
        Self {
            last: Arc::new(Mutex::new(now_ms())),
            interval: Arc::new(Mutex::new(0.0)),
        }
    }

    /// A pulse beating now whose interval is this one's.
    fn sharing_interval(&self) -> Self {
        Self {
            last: Arc::new(Mutex::new(now_ms())),
            interval: self.interval.clone(),
        }
    }

    fn beat(&self) {
        let now = now_ms();
        let mut last = self.last.lock().unwrap();
        let gap = (now - *last).max(0.0);
        *last = now;
        let mut interval = self.interval.lock().unwrap();
        *interval = if gap > *interval {
            gap
        } else {
            0.9 * *interval + 0.1 * gap
        };
    }

    fn since(&self) -> Duration {
        let last = *self.last.lock().unwrap();
        Duration::from_secs_f64(((now_ms() - last) / 1000.0).max(0.0))
    }

    fn halt_bound(&self) -> Duration {
        let interval = *self.interval.lock().unwrap();
        IDLE_STOP.max(Duration::from_secs_f64(
            f64::from(HALT_TICKS) * interval / 1000.0,
        ))
    }
}

fn is_halted(pulse: &Pulse) -> bool {
    pulse.since() > pulse.halt_bound()
}

fn now_ms() -> f64 {
    static EPOCH: LazyLock<std::time::Instant> = LazyLock::new(std::time::Instant::now);
    EPOCH.elapsed().as_secs_f64() * 1000.0
}

/// The request body both TTS endpoints take.
#[derive(Serialize)]
struct TtsRequest<'a> {
    voice: &'a str,
    text: &'a str,
}

/// One AWS Polly speech mark, as the endpoint returns them (visemes only).
#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
struct SpeechMark {
    /// Milliseconds into the audio.
    time: u64,
    /// The Polly viseme code.
    value: String,
}

#[derive(Deserialize)]
struct VisemeResponse {
    visemes: Vec<SpeechMark>,
}

/// Fetch the audio (mp3) and the viseme timeline from the TTS endpoints.
async fn synthesize(
    base: &str,
    voice: &str,
    text: &str,
) -> Result<(Vec<u8>, Vec<SpeechMark>), String> {
    let client = reqwest::Client::new();
    let body = TtsRequest { voice, text };
    let audio = client
        .post(format!("{base}/tts/get-audio"))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("get-audio: {e}"))?
        .error_for_status()
        .map_err(|e| format!("get-audio: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("get-audio body: {e}"))?
        .to_vec();
    let marks = client
        .post(format!("{base}/tts/get-visemes"))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("get-visemes: {e}"))?
        .error_for_status()
        .map_err(|e| format!("get-visemes: {e}"))?
        .json::<VisemeResponse>()
        .await
        .map_err(|e| format!("get-visemes decode: {e}"))?
        .visemes;
    Ok((audio, marks))
}

/// The face-standard shape for a Polly viseme code; silence for a code the
/// table does not know.
fn polly_shape(code: &str) -> &'static str {
    match code {
        "sil" => "sil",
        "p" => "PP",
        "f" => "FF",
        "T" => "TH",
        "t" => "DD",
        "k" => "kk",
        "S" => "CH",
        "s" => "SS",
        "l" => "nn",
        "r" => "RR",
        "a" => "aa",
        "e" | "E" => "E",
        "i" | "@" => "ih",
        "o" | "O" => "oh",
        "u" => "ou",
        other => {
            log::warn!("say: unknown Polly viseme code {other:?}, at rest");
            SILENCE_VISEME
        }
    }
}

/// A shape change on the audio timeline, in milliseconds from its start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cue {
    time_ms: u64,
    shape: &'static str,
}

fn cues(marks: &[SpeechMark]) -> Vec<Cue> {
    marks
        .iter()
        .map(|mark| Cue {
            time_ms: mark.time,
            shape: polly_shape(&mark.value),
        })
        .collect()
}

/// The shape at `playhead_ms`: every cue at or before the playhead is
/// consumed, and the last one consumed is the current shape.
fn shape_at(
    cues: &[Cue],
    next: &mut usize,
    playhead_ms: u64,
    current: &'static str,
) -> &'static str {
    let mut shape = current;
    while *next < cues.len() && cues[*next].time_ms <= playhead_ms {
        shape = cues[*next].shape;
        *next += 1;
    }
    shape
}

/// A spawning handle: the ambient tokio runtime if the device runs in one,
/// otherwise a dedicated one.
static TOKIO_HANDLE: LazyLock<tokio::runtime::Handle> = LazyLock::new(|| {
    tokio::runtime::Handle::try_current().unwrap_or_else(|_| {
        Box::leak(Box::new(
            tokio::runtime::Runtime::new().expect("a tokio runtime"),
        ))
        .handle()
        .clone()
    })
});

/// One live utterance.
struct Run {
    /// Synthesis then playback; its output is the terminal status.
    handle: tokio::task::JoinHandle<Status>,
    /// The shape at the audio playhead, advanced by the task and sampled by
    /// the tick.
    viseme: Arc<Mutex<&'static str>>,
    pulse: Pulse,
}

fn spawn(base: &str, text: String, voice: String, pulse: Pulse) -> Run {
    let viseme = Arc::new(Mutex::new(SILENCE_VISEME));
    let cell = viseme.clone();
    let pulse_task = pulse.clone();
    let base = base.to_string();
    let handle = TOKIO_HANDLE.spawn(async move {
        let (audio, marks) = match synthesize(&base, &voice, &text).await {
            Ok(pair) => pair,
            Err(e) => {
                log::error!("say: synthesis failed: {e}");
                return Status::Failure;
            }
        };
        // Halted while fetching: nothing to stop, nothing to play.
        if is_halted(&pulse_task) {
            return Status::Failure;
        }
        // Playback blocks and rodio's stream is thread-bound: the blocking
        // pool plays, this task awaits the outcome.
        tokio::task::spawn_blocking(move || play(audio, marks, cell, pulse_task))
            .await
            .unwrap_or(Status::Failure)
    });
    Run {
        handle,
        viseme,
        pulse,
    }
}

/// Poll the run's task once: the tick loop is the executor, a no-op waker
/// suffices.
fn poll(run: &mut Run) -> Observed {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};
    let current = *run.viseme.lock().unwrap();
    let mut cx = Context::from_waker(Waker::noop());
    match Pin::new(&mut run.handle).poll(&mut cx) {
        Poll::Pending => Observed::Running(current),
        Poll::Ready(Ok(status)) => Observed::Ended(status),
        Poll::Ready(Err(_)) => Observed::Ended(Status::Failure),
    }
}

/// Play the mp3 whole, advancing the viseme cell at the sink's playhead; a
/// quiet pulse stops the sink.
fn play(
    audio: Vec<u8>,
    marks: Vec<SpeechMark>,
    viseme: Arc<Mutex<&'static str>>,
    pulse: Pulse,
) -> Status {
    let (_stream, output) = match rodio::OutputStream::try_default() {
        Ok(pair) => pair,
        Err(e) => {
            log::error!("say: audio output init failed: {e}");
            return Status::Failure;
        }
    };
    let sink = match rodio::Sink::try_new(&output) {
        Ok(sink) => sink,
        Err(e) => {
            log::error!("say: audio sink failed: {e}");
            return Status::Failure;
        }
    };
    let source = match rodio::Decoder::new(std::io::Cursor::new(audio)) {
        Ok(source) => source,
        Err(e) => {
            log::error!("say: audio decode failed: {e}");
            return Status::Failure;
        }
    };
    sink.append(source);
    let cues = cues(&marks);
    let mut next = 0;
    let mut current = SILENCE_VISEME;
    let status = loop {
        if is_halted(&pulse) {
            sink.stop();
            break Status::Failure;
        }
        if sink.empty() {
            break Status::Success;
        }
        current = shape_at(&cues, &mut next, sink.get_pos().as_millis() as u64, current);
        *viseme.lock().unwrap() = current;
        std::thread::sleep(Duration::from_millis(15));
    };
    *viseme.lock().unwrap() = SILENCE_VISEME;
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shape_follows_the_playhead_through_the_cues() {
        let cues = cues(&[
            SpeechMark {
                time: 0,
                value: "p".into(),
            },
            SpeechMark {
                time: 100,
                value: "a".into(),
            },
            SpeechMark {
                time: 200,
                value: "sil".into(),
            },
        ]);
        let mut next = 0;
        assert_eq!(shape_at(&cues, &mut next, 0, SILENCE_VISEME), "PP");
        assert_eq!(shape_at(&cues, &mut next, 150, "PP"), "aa");
        assert_eq!(shape_at(&cues, &mut next, 900, "aa"), "sil");
        assert_eq!(next, 3);
    }

    #[test]
    fn an_empty_sentence_says_nothing() {
        let mut viseme = String::new();
        assert_eq!(
            cloud::say(String::new(), String::new(), &mut viseme),
            Status::Success
        );
        assert_eq!(viseme, SILENCE_VISEME);
    }
}
