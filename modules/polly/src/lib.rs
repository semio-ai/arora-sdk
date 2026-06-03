mod arora_generated;

use arora_generated::behavior_tree::status::Status;
use aws_config::{meta::region::RegionProviderChain, BehaviorVersion};
use aws_sdk_polly::{
  types::{OutputFormat, VoiceId},
  Client, Error,
};
use bytes::Buf;
use soloud::*;
use std::sync::Mutex;
use tokio::task::JoinHandle;

lazy_static::lazy_static! {
  static ref TOKIO_RUNTIME: tokio::runtime::Runtime = tokio::runtime::Runtime::new().unwrap();
  static ref TTS_TASK: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);
  static ref TTS_STATUS: Mutex<Status> = Mutex::new(Status::Running);
}

fn hello_world() -> Status {
  say(Some("Hello, world!".to_string()))
}

fn say(text: Option<String>) -> Status {
  let text = match text {
    Some(text) => text,
    None => return Status::Failure,
  };
  let mut locked_task = match TTS_TASK.lock() {
    Ok(task) => task,
    Err(_) => return Status::Failure,
  };
  let mut locked_status = match TTS_STATUS.lock() {
    Ok(status) => status,
    Err(_) => return Status::Failure,
  };
  let ret = locked_status.clone();
  if locked_task.is_none() || *locked_status == Status::Failure {
    if *locked_status == Status::Running {
      // the task was finished and status was reset to running, let's respawn it
      *locked_task = Some(TOKIO_RUNTIME.spawn(async move {
        let region_provider = RegionProviderChain::default_provider().or_else("eu-west-3");
        let config = aws_config::defaults(BehaviorVersion::latest())
          .region(region_provider)
          .load()
          .await;
        let client = Client::new(&config);
        let result = synthesize(&client, text).await;
        let mut locked_status = TTS_STATUS.lock().expect("failed to lock status");
        let mut locked_task = match TTS_TASK.lock() {
          Ok(task) => task,
          Err(_) => {
            *locked_status = Status::Failure;
            return;
          }
        };
        *locked_task = None;
        *locked_status = match result {
          Ok(_) => Status::Success,
          Err(_) => Status::Failure,
        }; // will be reported next call
      }));
    } // else the task finished at previous call but the status still needs to be reported
    *locked_status = Status::Running;
  }
  ret
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
    while sl.voice_count() > 0 {
      std::thread::sleep(std::time::Duration::from_millis(30));
    }
  }

  Ok(())
}
