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
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::oneshot;

lazy_static::lazy_static! {
  // Reuse the caller's runtime if one is active (e.g. arora-cli), otherwise
  // create a dedicated one. Either way we only need a Handle for spawning.
  static ref TOKIO_HANDLE: tokio::runtime::Handle = tokio::runtime::Handle::try_current()
    .unwrap_or_else(|_| Box::leak(Box::new(tokio::runtime::Runtime::new().unwrap())).handle().clone());
  // Live utterances, keyed so concurrent `say`s do not share a slot. Each run is
  // the terminal-status channel of a synthesis+playback task spawned on the Tokio
  // runtime. `say` polls the channel each tick — that poll *is* the poll-on-tick
  // contract (see docs/async-functions.md): channel empty = Running (Pending),
  // a delivered status = terminal. Replaces the old process-global TTS_TASK /
  // TTS_STATUS singletons, whose one slot let two utterances overwrite each other
  // and whose "report the status next call" hand-off was racy.
  static ref RUNS: Mutex<HashMap<u64, oneshot::Receiver<Status>>> = Mutex::new(HashMap::new());
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

    // Poll the run (scoped so the borrow ends before we may remove it).
    let outcome = runs.get_mut(&key).expect("just inserted").try_recv();
    match outcome {
        // Still speaking — the runtime re-ticks me next step.
        Err(oneshot::error::TryRecvError::Empty) => Status::Running,
        // Terminal — drop the run so a later re-trigger of the same text respawns.
        Ok(status) => {
            runs.remove(&key);
            status
        }
        // The task ended without sending (panicked/aborted): terminal failure.
        Err(oneshot::error::TryRecvError::Closed) => {
            runs.remove(&key);
            Status::Failure
        }
    }
}

/// Spawn the AWS Polly synthesis + playback on the Tokio runtime (which owns the
/// reactor) and hand the terminal status back through a channel `say` polls. The
/// heavy work never runs on the tick thread; the tick only observes the channel.
fn spawn_say(text: String) -> oneshot::Receiver<Status> {
    let (tx, rx) = oneshot::channel();
    TOKIO_HANDLE.spawn(async move {
        let region_provider = RegionProviderChain::default_provider().or_else("eu-west-3");
        let config = aws_config::defaults(BehaviorVersion::latest())
            .region(region_provider)
            .load()
            .await;
        let client = Client::new(&config);
        let status = match synthesize(&client, text).await {
            Ok(_) => Status::Success,
            Err(_) => Status::Failure,
        };
        // The receiver is gone if the caller stopped ticking before we finished;
        // that is expected, not an error.
        let _ = tx.send(status);
    });
    rx
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
