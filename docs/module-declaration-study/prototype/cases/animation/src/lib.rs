//! The vizij animation module's interface, declared in Rust — the six exports
//! ARORA-81's case exercises, with the boundary types as `#[derive(AroraType,
//! AroraValue)]` structs pinned to the ids of `types/structure/*.yaml`.
//! Implementations are stand-ins for `vizij-animation-core`; the contract
//! under test is the declaration and the call boundary.

use arora_module_derive::AroraValue;
use arora_types::value::Value;
use arora_types::AroraType;
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, AroraType, AroraValue)]
#[arora(id = "76697a69-6a00-0000-0000-000000000103")]
pub struct TransitionHandle {
  #[arora(id = "76697a69-6a00-0000-0103-000000000001")]
  pub x: f32,
  #[arora(id = "76697a69-6a00-0000-0103-000000000002")]
  pub y: f32,
}

#[derive(Debug, Clone, PartialEq, AroraType, AroraValue)]
#[arora(id = "76697a69-6a00-0000-0000-000000000102")]
#[arora_version("1.1.0")]
pub struct Keypoint {
  #[arora(id = "76697a69-6a00-0000-0102-000000000001")]
  pub id: String,
  #[arora(id = "76697a69-6a00-0000-0102-000000000002")]
  pub stamp: f32,
  /// A dynamic value: any Vizij composite rides through untyped.
  #[arora(id = "76697a69-6a00-0000-0102-000000000003", keyvalue)]
  pub value: Value,
  #[arora(id = "76697a69-6a00-0000-0102-000000000004")]
  pub transitions_in: Vec<TransitionHandle>,
  #[arora(id = "76697a69-6a00-0000-0102-000000000005")]
  pub transitions_out: Vec<TransitionHandle>,
}

#[derive(Debug, Clone, PartialEq, AroraType, AroraValue)]
#[arora(id = "76697a69-6a00-0000-0000-000000000101")]
#[arora_version("1.1.0")]
pub struct AnimTrack {
  #[arora(id = "76697a69-6a00-0000-0101-000000000001")]
  pub id: String,
  #[arora(id = "76697a69-6a00-0000-0101-000000000002")]
  pub name: String,
  #[arora(id = "76697a69-6a00-0000-0101-000000000003")]
  pub animatable_id: String,
  #[arora(id = "76697a69-6a00-0000-0101-000000000004")]
  pub points: Vec<Keypoint>,
}

#[derive(Debug, Clone, PartialEq, AroraType, AroraValue)]
#[arora(id = "76697a69-6a00-0000-0000-000000000100")]
#[arora_version("1.1.0")]
pub struct AnimationClip {
  #[arora(id = "76697a69-6a00-0000-0100-000000000001")]
  pub name: String,
  #[arora(id = "76697a69-6a00-0000-0100-000000000002")]
  pub duration: u32,
  #[arora(id = "76697a69-6a00-0000-0100-000000000003")]
  pub tracks: Vec<AnimTrack>,
}

#[derive(Debug, Clone, PartialEq, AroraType, AroraValue)]
#[arora(id = "76697a69-6a00-0000-0000-000000000110")]
pub struct TrackOutput {
  #[arora(id = "76697a69-6a00-0000-0110-000000000001")]
  pub track_id: String,
  #[arora(id = "76697a69-6a00-0000-0110-000000000002")]
  pub default_key: String,
  #[arora(id = "76697a69-6a00-0000-0110-000000000003", keyvalue)]
  pub value: Value,
}

#[derive(Debug, Clone, PartialEq, AroraType, AroraValue)]
#[arora(id = "76697a69-6a00-0000-0000-000000000111")]
pub struct PlayerState {
  #[arora(id = "76697a69-6a00-0000-0111-000000000001")]
  pub player: u32,
  #[arora(id = "76697a69-6a00-0000-0111-000000000002")]
  pub state: String,
  #[arora(id = "76697a69-6a00-0000-0111-000000000003")]
  pub time_ns: u64,
  #[arora(id = "76697a69-6a00-0000-0111-000000000004")]
  pub duration_ns: u64,
  #[arora(id = "76697a69-6a00-0000-0111-000000000005")]
  pub speed: f32,
}

/// The stand-in engine: loaded clips and created players, by index.
static CLIPS: Mutex<Vec<AnimationClip>> = Mutex::new(Vec::new());
static PLAYERS: Mutex<Vec<(String, Vec<u32>)>> = Mutex::new(Vec::new());

#[arora_module_derive::module(
  id = "76697a69-6a00-0000-0d00-000000000000",
  name = "vizij-animation",
  version = "0.2.0"
)]
pub mod animation {
  use super::*;
  use arora_module_derive::export;

  #[export(id = "76697a69-6a00-0000-0f00-000000000001")]
  pub fn load_animation(
    #[param(id = "76697a69-6a00-0000-0f01-000000000001")] clip: AnimationClip,
  ) -> u32 {
    let mut clips = CLIPS.lock().unwrap();
    clips.push(clip);
    (clips.len() - 1) as u32
  }

  #[export(id = "76697a69-6a00-0000-0f00-000000000002")]
  pub fn create_player(#[param(id = "76697a69-6a00-0000-0f02-000000000001")] name: String) -> u32 {
    let mut players = PLAYERS.lock().unwrap();
    players.push((name, Vec::new()));
    (players.len() - 1) as u32
  }

  /// Returns `player * 1000 + anim`: the test can tell which argument landed
  /// where.
  #[export(id = "76697a69-6a00-0000-0f00-000000000003")]
  pub fn add_instance(
    #[param(id = "76697a69-6a00-0000-0f03-000000000001")] player: u32,
    #[param(id = "76697a69-6a00-0000-0f03-000000000002")] anim: u32,
  ) -> u32 {
    if let Some((_, instances)) = PLAYERS.lock().unwrap().get_mut(player as usize) {
      instances.push(anim);
    }
    player * 1000 + anim
  }

  /// One output per track of every loaded clip, carrying `dt_ns` as its value.
  #[export(id = "76697a69-6a00-0000-0f00-000000000004")]
  pub fn step(
    #[param(id = "76697a69-6a00-0000-0f04-000000000001")] dt_ns: u64,
  ) -> Vec<TrackOutput> {
    CLIPS
      .lock()
      .unwrap()
      .iter()
      .flat_map(|clip| clip.tracks.iter())
      .map(|track| TrackOutput {
        track_id: track.id.clone(),
        default_key: track.animatable_id.clone(),
        value: Value::U64(dt_ns),
      })
      .collect()
  }

  #[export(id = "76697a69-6a00-0000-0f00-000000000008")]
  pub fn seek(
    #[param(id = "76697a69-6a00-0000-0f08-000000000001")] player: u32,
    #[param(id = "76697a69-6a00-0000-0f08-000000000002")] time_ns: u64,
  ) -> u32 {
    let _ = time_ns;
    player
  }

  #[export(id = "76697a69-6a00-0000-0f00-00000000000d")]
  pub fn player_states() -> Vec<PlayerState> {
    PLAYERS
      .lock()
      .unwrap()
      .iter()
      .enumerate()
      .map(|(i, (name, _))| PlayerState {
        player: i as u32,
        state: name.clone(),
        time_ns: 0,
        duration_ns: 0,
        speed: 1.0,
      })
      .collect()
  }
}
