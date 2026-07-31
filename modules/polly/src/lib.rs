// Generated code: lint hygiene is the generator's responsibility, not this
// repo's. Allow clippy/dead_code over the whole generated subtree.
#[allow(clippy::all, dead_code)]
mod arora_generated;

use arora_generated::behavior_tree::status::Status;
use aws_config::{meta::region::RegionProviderChain, BehaviorVersion};
use aws_sdk_polly::{
    types::{OutputFormat, VoiceId},
    Client, Error,
};
use bytes::Buf;
use soloud::*;
use std::collections::HashMap;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};
use std::time::Duration;
use tokio::task::JoinHandle;

lazy_static::lazy_static! {
  // Reuse the caller's runtime if one is active (e.g. arora-cli), otherwise
  // create a dedicated one. Either way we only need a Handle for spawning.
  static ref TOKIO_HANDLE: tokio::runtime::Handle = tokio::runtime::Handle::try_current()
    .unwrap_or_else(|_| Box::leak(Box::new(tokio::runtime::Runtime::new().unwrap())).handle().clone());
  // Live utterances, keyed so concurrent `say`s do not share a slot. Each run is
  // the `JoinHandle` of a synthesis+playback task spawned on the Tokio runtime. A
  // `JoinHandle` is itself a `Future`, so `say` drives the poll-on-tick contract
  // (see docs/async-functions.md) by polling it directly each tick with a no-op
  // waker: `Poll::Pending` = Running, `Poll::Ready` = terminal. The Tokio runtime
  // owns the reactor that advances the task off the tick thread; the tick only
  // samples its readiness. Replaces the old process-global TTS_TASK / TTS_STATUS
  // singletons, whose one slot let two utterances overwrite each other and whose
  // "report the status next call" hand-off was racy.
  static ref RUNS: Mutex<HashMap<u64, JoinHandle<Status>>> = Mutex::new(HashMap::new());
}

fn hello_world() -> Status {
    say(Some("Hello, world!".to_string()))
}

/// The run key for an utterance. The module ABI hands `say` only its arguments —
/// no per-invocation identity — so runs are keyed by content. Distinct utterances
/// no longer collide (the old singleton bug); same-text callers still share one
/// run. True per-caller keying needs a run id in the ABI — see the open question
/// in docs/async-functions.md.
fn utterance_key(text: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

fn say(text: Option<String>) -> Status {
    let text = match text {
        Some(text) => text,
        None => return Status::Failure,
    };
    let key = utterance_key(&text);
    let mut runs = match RUNS.lock() {
        Ok(runs) => runs,
        Err(_) => return Status::Failure,
    };

    // First tick for this utterance: spawn synthesis + playback off the tick
    // thread and record the run. Later ticks fall straight through to the poll.
    if !runs.contains_key(&key) {
        runs.insert(key, spawn_say(text));
    }

    // Poll the run's `JoinHandle` — the `Future` — once. This call is one `poll`:
    // the tick loop is the executor, and a no-op waker suffices because the runtime
    // re-ticks us every step regardless of any wake-up. A completed `JoinHandle`
    // must never be polled again, so a terminal result drops the run.
    let mut cx = Context::from_waker(Waker::noop());
    let handle = runs.get_mut(&key).expect("just inserted");
    match Future::poll(Pin::new(handle), &mut cx) {
        // Still speaking — the runtime re-ticks me next step.
        Poll::Pending => Status::Running,
        // Terminal — drop the run so a later re-trigger of the same text respawns.
        Poll::Ready(Ok(status)) => {
            runs.remove(&key);
            status
        }
        // The task panicked or was aborted: terminal failure.
        Poll::Ready(Err(_join_error)) => {
            runs.remove(&key);
            Status::Failure
        }
    }
}

/// Spawn the AWS Polly synthesis + playback on the Tokio runtime (which owns the
/// reactor) and return its `JoinHandle` — the `Future` `say` polls each tick. The
/// heavy work never runs on the tick thread; the tick only samples the handle. The
/// task resolves to the terminal `Status`, so the join value *is* the outcome.
fn spawn_say(text: String) -> JoinHandle<Status> {
    TOKIO_HANDLE.spawn(async move {
        let region_provider = RegionProviderChain::default_provider().or_else("eu-west-3");
        let config = aws_config::defaults(BehaviorVersion::latest())
            .region(region_provider)
            .load()
            .await;
        let client = Client::new(&config);
        match synthesize(&client, text).await {
            Ok(_) => Status::Success,
            Err(_) => Status::Failure,
        }
    })
}

async fn synthesize(client: &Client, content: String) -> Result<(), Error> {
    let resp = client
        .synthesize_speech()
        .output_format(OutputFormat::Mp3)
        .text(content)
        .voice_id(VoiceId::Ivy)
        .send()
        .await?;

    // Get MP3 data from response and save it
    let mut blob = resp
        .audio_stream
        .collect()
        .await
        .expect("failed to read data");

    let sl = Soloud::default().unwrap();
    let mut wav_stream = audio::WavStream::default();

    while blob.remaining() > 0 {
        let size = {
            let chunk = blob.chunk();
            wav_stream.load_mem(chunk).unwrap();
            chunk.len()
        };
        blob.advance(size);
        sl.play(&wav_stream);
        // Wait for playback WITHOUT pinning a worker thread: yield to the runtime
        // between polls (`tokio::time::sleep().await`) instead of the old
        // `std::thread::sleep`, which blocked a Tokio thread for the whole
        // utterance.
        while sl.voice_count() > 0 {
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }

    Ok(())
}
