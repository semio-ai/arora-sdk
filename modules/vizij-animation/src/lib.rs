mod arora_generated;

use serde::Deserialize;
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use vizij_animation_core::{
  parse_stored_animation_json, AnimId, Config, Engine, Inputs, InstanceCfg, LoopMode,
  PlayerCommand, PlayerId,
};
use vizij_api_core::WriteBatch;

static FACADE: OnceLock<Mutex<AnimationModuleManager>> = OnceLock::new();

fn facade() -> &'static Mutex<AnimationModuleManager> {
  FACADE.get_or_init(|| Mutex::new(AnimationModuleManager::new()))
}

fn with_facade<T>(
  f: impl FnOnce(&mut AnimationModuleManager) -> Result<T, String>,
) -> Result<T, String> {
  let mut guard = facade()
    .lock()
    .map_err(|_| "vizij animation engine lock is poisoned".to_string())?;
  f(&mut guard)
}

fn ok(value: JsonValue) -> String {
  json!({
    "ok": true,
    "value": value,
  })
  .to_string()
}

fn err(message: impl Into<String>) -> String {
  json!({
    "ok": false,
    "error": message.into(),
  })
  .to_string()
}

fn parse_request<T: for<'de> Deserialize<'de>>(request_json: Option<String>) -> Result<T, String> {
  let request_json = request_json.ok_or_else(|| "missing request_json".to_string())?;
  serde_json::from_str(&request_json).map_err(|error| format!("invalid request_json: {error}"))
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredAnimationEnvelope {
  Wrapped {
    #[serde(
      default,
      rename = "controllerId",
      alias = "controller_id",
      alias = "id"
    )]
    controller_id: Option<String>,
    animation: JsonValue,
  },
  Direct(JsonValue),
}

#[derive(Deserialize)]
struct CreatePlayerRequest {
  #[serde(
    default,
    rename = "controllerId",
    alias = "controller_id",
    alias = "id"
  )]
  controller_id: Option<String>,
  #[serde(default = "default_player_name")]
  name: String,
  #[serde(default)]
  speed: Option<f32>,
  #[serde(default, rename = "loopMode", alias = "loop_mode", alias = "loop")]
  loop_mode: Option<JsonValue>,
}

fn default_player_name() -> String {
  "arora-vizij-player".to_string()
}

fn parse_loop_mode(value: Option<&JsonValue>) -> Result<Option<LoopMode>, String> {
  let Some(value) = value else {
    return Ok(None);
  };
  let Some(raw) = value.as_str() else {
    return Err("player loopMode must be a string".to_string());
  };
  match raw.trim().to_ascii_lowercase().as_str() {
    "once" => Ok(Some(LoopMode::Once)),
    "loop" => Ok(Some(LoopMode::Loop)),
    "pingpong" | "ping_pong" | "ping-pong" => Ok(Some(LoopMode::PingPong)),
    other => Err(format!("unsupported player loopMode '{other}'")),
  }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddInstanceRequest {
  #[serde(
    default,
    rename = "controllerId",
    alias = "controller_id",
    alias = "id"
  )]
  controller_id: Option<String>,
  player_id: u32,
  animation_id: u32,
  #[serde(default)]
  config: Option<InstanceConfigPatch>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnimationSetup {
  #[serde(default)]
  animation: Option<JsonValue>,
  #[serde(default)]
  player: Option<CreatePlayerRequest>,
  #[serde(default)]
  instance: Option<InstanceConfigPatch>,
}

#[derive(Deserialize)]
struct UpdateNodesWritesRequest {
  #[serde(
    default,
    rename = "controllerId",
    alias = "controller_id",
    alias = "id"
  )]
  controller_id: Option<String>,
  dt: f32,
  #[serde(default)]
  inputs: Option<Inputs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigureControllerRequest {
  #[serde(alias = "controller_id", alias = "id")]
  controller_id: String,
  #[serde(default)]
  setup: JsonValue,
}

#[derive(Deserialize)]
struct RemoveControllerRequest {
  #[serde(rename = "controllerId", alias = "controller_id", alias = "id")]
  controller_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstanceConfigPatch {
  weight: Option<f32>,
  #[serde(alias = "time_scale", alias = "timescale")]
  time_scale: Option<f32>,
  #[serde(alias = "start_offset")]
  start_offset: Option<f32>,
  /// Studio instance offset in milliseconds. `start_offset`/`startOffset` remain seconds.
  offset: Option<f32>,
  #[serde(alias = "active")]
  enabled: Option<bool>,
}

impl From<InstanceConfigPatch> for InstanceCfg {
  fn from(patch: InstanceConfigPatch) -> Self {
    let mut config = InstanceCfg::default();
    if let Some(weight) = patch.weight {
      config.weight = weight;
    }
    if let Some(time_scale) = patch.time_scale {
      config.time_scale = time_scale;
    }
    if let Some(start_offset) = patch.start_offset {
      config.start_offset = start_offset;
    } else if let Some(offset_ms) = patch.offset {
      config.start_offset = offset_ms / 1000.0;
    }
    if let Some(enabled) = patch.enabled {
      config.enabled = enabled;
    }
    config
  }
}

#[derive(Debug)]
pub struct AnimationModuleFacade {
  engine: Engine,
}

impl Default for AnimationModuleFacade {
  fn default() -> Self {
    Self::new()
  }
}

impl AnimationModuleFacade {
  pub fn new() -> Self {
    Self {
      engine: Engine::new(Config::default()),
    }
  }

  pub fn reset(&mut self) {
    self.engine = Engine::new(Config::default());
  }

  pub fn load_stored_animation_value(&mut self, animation: JsonValue) -> Result<AnimId, String> {
    let animation_json = match animation {
      JsonValue::String(value) => value,
      other => serde_json::to_string(&other)
        .map_err(|error| format!("failed to serialize animation payload: {error}"))?,
    };
    let data = parse_stored_animation_json(&animation_json)
      .map_err(|error| format!("failed to parse stored animation: {error}"))?;
    Ok(self.engine.load_animation(data))
  }

  pub fn create_player(&mut self, name: &str) -> PlayerId {
    self.engine.create_player(name)
  }

  fn apply_player_setup(
    &mut self,
    player: PlayerId,
    setup: Option<&CreatePlayerRequest>,
  ) -> Result<(), String> {
    let Some(setup) = setup else {
      return Ok(());
    };
    let mut inputs = Inputs::default();
    if let Some(speed) = setup.speed {
      if !speed.is_finite() {
        return Err("player speed must be finite".to_string());
      }
      inputs
        .player_cmds
        .push(PlayerCommand::SetSpeed { player, speed });
    }
    if let Some(mode) = parse_loop_mode(setup.loop_mode.as_ref())? {
      inputs
        .player_cmds
        .push(PlayerCommand::SetLoopMode { player, mode });
    }
    if inputs.player_cmds.is_empty() {
      return Ok(());
    }
    let _ = self.engine.update_values(0.0, inputs);
    Ok(())
  }

  pub fn add_instance(
    &mut self,
    player: PlayerId,
    animation: AnimId,
    config: InstanceCfg,
  ) -> vizij_animation_core::ids::InstId {
    self.engine.add_instance(player, animation, config)
  }

  pub fn configure_from_setup_value(&mut self, setup: JsonValue) -> Result<(), String> {
    if setup.is_null() {
      return Ok(());
    }
    let setup = serde_json::from_value::<AnimationSetup>(setup)
      .map_err(|error| format!("invalid animation setup: {error}"))?;
    let Some(animation) = setup.animation else {
      return Ok(());
    };
    let animation_id = self.load_stored_animation_value(animation)?;
    let player_name = setup
      .player
      .as_ref()
      .map(|player| player.name.as_str())
      .unwrap_or("arora-vizij-player");
    let player_id = self.create_player(player_name);
    let config = setup.instance.map(InstanceCfg::from).unwrap_or_default();
    self.add_instance(player_id, animation_id, config);
    self.apply_player_setup(player_id, setup.player.as_ref())?;
    Ok(())
  }

  pub fn update_writebatch(
    &mut self,
    dt: f32,
    inputs: Option<Inputs>,
  ) -> Result<WriteBatch, String> {
    self
      .update_outputs(dt, inputs)
      .map(|(batch, _events)| batch)
  }

  pub fn update_outputs(
    &mut self,
    dt: f32,
    inputs: Option<Inputs>,
  ) -> Result<(WriteBatch, Vec<JsonValue>), String> {
    if !dt.is_finite() || dt < 0.0 {
      return Err("dt must be finite and non-negative".to_string());
    }
    let outputs = self.engine.update_values(dt, inputs.unwrap_or_default());
    let batch = outputs.to_writebatch();
    let events = outputs
      .events
      .iter()
      .filter_map(|event| serde_json::to_value(event).ok())
      .collect();
    Ok((batch, events))
  }

  pub fn list_animations(&self) -> Vec<vizij_animation_core::engine::AnimationInfo> {
    self.engine.list_animations()
  }
}

#[derive(Debug)]
pub struct AnimationModuleManager {
  default: AnimationModuleFacade,
  controllers: HashMap<String, AnimationModuleFacade>,
}

impl Default for AnimationModuleManager {
  fn default() -> Self {
    Self::new()
  }
}

impl AnimationModuleManager {
  pub fn new() -> Self {
    Self {
      default: AnimationModuleFacade::new(),
      controllers: HashMap::new(),
    }
  }

  pub fn reset(&mut self) {
    *self = Self::new();
  }

  fn facade_mut(
    &mut self,
    controller_id: Option<&str>,
  ) -> Result<&mut AnimationModuleFacade, String> {
    match controller_id {
      Some(id) => self
        .controllers
        .get_mut(id)
        .ok_or_else(|| format!("animation controller '{id}' is not configured")),
      None => Ok(&mut self.default),
    }
  }

  fn facade_ref(&self, controller_id: Option<&str>) -> Result<&AnimationModuleFacade, String> {
    match controller_id {
      Some(id) => self
        .controllers
        .get(id)
        .ok_or_else(|| format!("animation controller '{id}' is not configured")),
      None => Ok(&self.default),
    }
  }

  pub fn configure_controller(
    &mut self,
    controller_id: String,
    setup: JsonValue,
  ) -> Result<(), String> {
    let mut controller = AnimationModuleFacade::new();
    controller.configure_from_setup_value(setup)?;
    self.controllers.insert(controller_id, controller);
    Ok(())
  }

  pub fn remove_controller(&mut self, controller_id: &str) -> bool {
    self.controllers.remove(controller_id).is_some()
  }

  pub fn load_stored_animation_value(
    &mut self,
    controller_id: Option<&str>,
    animation: JsonValue,
  ) -> Result<AnimId, String> {
    self
      .facade_mut(controller_id)?
      .load_stored_animation_value(animation)
  }

  pub fn create_player(
    &mut self,
    controller_id: Option<&str>,
    name: &str,
  ) -> Result<PlayerId, String> {
    Ok(self.facade_mut(controller_id)?.create_player(name))
  }

  pub fn add_instance(
    &mut self,
    controller_id: Option<&str>,
    player: PlayerId,
    animation: AnimId,
    config: InstanceCfg,
  ) -> Result<vizij_animation_core::ids::InstId, String> {
    Ok(
      self
        .facade_mut(controller_id)?
        .add_instance(player, animation, config),
    )
  }

  pub fn update_outputs(
    &mut self,
    controller_id: Option<&str>,
    dt: f32,
    inputs: Option<Inputs>,
  ) -> Result<(WriteBatch, Vec<JsonValue>), String> {
    self.facade_mut(controller_id)?.update_outputs(dt, inputs)
  }

  pub fn list_animations(
    &self,
    controller_id: Option<&str>,
  ) -> Result<Vec<vizij_animation_core::engine::AnimationInfo>, String> {
    Ok(self.facade_ref(controller_id)?.list_animations())
  }
}

fn reset_engine() -> String {
  match facade().lock() {
    Ok(mut guard) => {
      guard.reset();
      ok(json!({ "reset": true }))
    }
    Err(_) => err("vizij animation engine lock is poisoned"),
  }
}

fn load_stored_animation(request_json: Option<String>) -> String {
  let request = match parse_request::<StoredAnimationEnvelope>(request_json) {
    Ok(request) => request,
    Err(error) => return err(error),
  };
  let (controller_id, animation) = match request {
    StoredAnimationEnvelope::Wrapped {
      controller_id,
      animation,
    } => (controller_id, animation),
    StoredAnimationEnvelope::Direct(animation) => (None, animation),
  };
  match with_facade(|facade| {
    facade.load_stored_animation_value(controller_id.as_deref(), animation)
  }) {
    Ok(id) => ok(json!({ "controllerId": controller_id, "animationId": id.0 })),
    Err(error) => err(error),
  }
}

fn create_player(request_json: Option<String>) -> String {
  let request = match parse_request::<CreatePlayerRequest>(request_json) {
    Ok(request) => request,
    Err(error) => return err(error),
  };

  let controller_id = request.controller_id.clone();
  match with_facade(|facade| facade.create_player(controller_id.as_deref(), &request.name)) {
    Ok(id) => ok(json!({ "controllerId": controller_id, "playerId": id.0 })),
    Err(error) => err(error),
  }
}

fn add_instance(request_json: Option<String>) -> String {
  let request = match parse_request::<AddInstanceRequest>(request_json) {
    Ok(request) => request,
    Err(error) => return err(error),
  };
  let config = request.config.map(InstanceCfg::from).unwrap_or_default();
  let controller_id = request.controller_id.clone();

  match with_facade(|facade| {
    facade.add_instance(
      controller_id.as_deref(),
      PlayerId(request.player_id),
      AnimId(request.animation_id),
      config,
    )
  }) {
    Ok(id) => ok(json!({ "controllerId": controller_id, "instanceId": id.0 })),
    Err(error) => err(error),
  }
}

fn update_nodes_writes(request_json: Option<String>) -> String {
  let request = match parse_request::<UpdateNodesWritesRequest>(request_json) {
    Ok(request) => request,
    Err(error) => return err(error),
  };
  if !request.dt.is_finite() || request.dt < 0.0 {
    return err("dt must be finite and non-negative");
  }
  let controller_id = request.controller_id.clone();

  match with_facade(|facade| {
    let (batch, events) =
      facade.update_outputs(controller_id.as_deref(), request.dt, request.inputs)?;
    serde_json::to_value(&batch)
      .map(|writes| json!({ "nodes": {}, "writes": writes, "events": events }))
      .map_err(|error| format!("failed to serialize write batch: {error}"))
  }) {
    Ok(value) => ok(value),
    Err(error) => err(error),
  }
}

fn list_animations() -> String {
  match with_facade(|facade| {
    serde_json::to_value(facade.list_animations(None)?)
      .map_err(|error| format!("failed to serialize animation list: {error}"))
  }) {
    Ok(value) => ok(value),
    Err(error) => err(error),
  }
}

fn configure_controller(request_json: Option<String>) -> String {
  let request = match parse_request::<ConfigureControllerRequest>(request_json) {
    Ok(request) => request,
    Err(error) => return err(error),
  };
  let controller_id = request.controller_id;
  match with_facade(|facade| facade.configure_controller(controller_id.clone(), request.setup)) {
    Ok(()) => ok(json!({ "controllerId": controller_id, "configured": true })),
    Err(error) => err(error),
  }
}

fn remove_controller(request_json: Option<String>) -> String {
  let request = match parse_request::<RemoveControllerRequest>(request_json) {
    Ok(request) => request,
    Err(error) => return err(error),
  };
  let controller_id = request.controller_id;
  match with_facade(|facade| Ok(facade.remove_controller(&controller_id))) {
    Ok(removed) => ok(json!({ "controllerId": controller_id, "removed": removed })),
    Err(error) => err(error),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::Value;

  fn fixture_animation() -> Value {
    fixture_animation_for_path("face/smile.amount")
  }

  fn fixture_animation_for_path(path: &str) -> Value {
    json!({
      "id": "arora-module-smoke",
      "name": "Arora Module Smoke",
      "formatVersion": 2,
      "defaultViewportExtent": 1000,
      "groups": [],
      "tracks": [
        {
          "id": "smile-track",
          "name": "Smile",
          "animatableId": path,
          "points": [
            { "id": "smile-0", "stamp": 0, "value": 0, "transitions": { "out": "linear" } },
            { "id": "smile-1", "stamp": 1000, "value": 1, "transitions": { "in": "linear" } }
          ]
        }
      ]
    })
  }

  fn unwrap_value(response: &str) -> Value {
    let parsed: Value = serde_json::from_str(response).expect("response json");
    assert_eq!(parsed["ok"], true, "{parsed}");
    parsed["value"].clone()
  }

  #[test]
  fn instance_config_accepts_studio_settings_aliases() {
    let request: AddInstanceRequest = serde_json::from_value(json!({
      "playerId": 7,
      "animationId": 13,
      "config": {
        "timescale": 2.0,
        "offset": 250.0,
        "active": false
      }
    }))
    .expect("deserialize Studio-shaped add-instance request");

    let config = InstanceCfg::from(request.config.expect("config patch"));
    assert_eq!(config.time_scale, 2.0);
    assert_eq!(config.start_offset, 0.25);
    assert!(!config.enabled);
  }

  #[test]
  fn module_facade_loads_and_steps_a_studio_animation() {
    unwrap_value(&reset_engine());

    let load_response = load_stored_animation(Some(
      json!({
        "animation": fixture_animation()
      })
      .to_string(),
    ));
    let animation_id = unwrap_value(&load_response)["animationId"]
      .as_u64()
      .expect("animation id") as u32;

    let player_response = create_player(Some(json!({ "name": "smoke" }).to_string()));
    let player_id = unwrap_value(&player_response)["playerId"]
      .as_u64()
      .expect("player id") as u32;

    let instance_response = add_instance(Some(
      json!({
        "playerId": player_id,
        "animationId": animation_id,
        "config": { "weight": 1.0 }
      })
      .to_string(),
    ));
    assert!(unwrap_value(&instance_response)["instanceId"].is_number());

    let update_response = update_nodes_writes(Some(json!({ "dt": 0.5 }).to_string()));
    let update = unwrap_value(&update_response);
    let writes = update["writes"].as_array().expect("writes array");
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0]["path"], "face/smile.amount");
    assert_eq!(writes[0]["value"]["type"], "float");
    assert!(writes[0]["value"]["data"].as_f64().unwrap() > 0.0);
  }

  #[test]
  fn configure_controller_applies_player_speed_setup() {
    unwrap_value(&reset_engine());
    unwrap_value(&configure_controller(Some(
      json!({
        "controllerId": "anim:paused",
        "setup": {
          "animation": fixture_animation(),
          "player": { "speed": 0.0, "loopMode": "loop" },
          "instance": { "weight": 1.0 }
        }
      })
      .to_string(),
    )));

    let first = unwrap_value(&update_nodes_writes(Some(
      json!({ "controllerId": "anim:paused", "dt": 0.5 }).to_string(),
    )));
    let first_value = first["writes"][0]["value"]["data"].as_f64().unwrap();
    assert!(
      first_value.abs() < 0.0001,
      "configured speed 0 should keep the player paused: {first}"
    );

    let play_inputs = Inputs {
      player_cmds: vec![PlayerCommand::Play {
        player: PlayerId(0),
      }],
      instance_updates: Vec::new(),
    };
    let after_play = unwrap_value(&update_nodes_writes(Some(
      json!({
        "controllerId": "anim:paused",
        "dt": 0.5,
        "inputs": play_inputs
      })
      .to_string(),
    )));
    assert!(
      after_play["writes"][0]["value"]["data"].as_f64().unwrap() > 0.0,
      "play command should resume a speed-0 player: {after_play}"
    );
  }

  #[test]
  fn controller_handles_preserve_independent_animation_state() {
    unwrap_value(&reset_engine());
    unwrap_value(&configure_controller(Some(
      json!({
        "controllerId": "anim:a",
        "setup": {
          "animation": fixture_animation_for_path("face/a.smile"),
          "instance": { "weight": 1.0 }
        }
      })
      .to_string(),
    )));
    unwrap_value(&configure_controller(Some(
      json!({
        "controllerId": "anim:b",
        "setup": {
          "animation": fixture_animation_for_path("face/b.smile"),
          "instance": { "weight": 1.0 }
        }
      })
      .to_string(),
    )));

    let anim_a_first = unwrap_value(&update_nodes_writes(Some(
      json!({ "controllerId": "anim:a", "dt": 0.25 }).to_string(),
    )));
    assert_eq!(anim_a_first["writes"][0]["path"], "face/a.smile");
    assert!((anim_a_first["writes"][0]["value"]["data"].as_f64().unwrap() - 0.25).abs() < 0.0001);

    let anim_b = unwrap_value(&update_nodes_writes(Some(
      json!({ "controllerId": "anim:b", "dt": 0.5 }).to_string(),
    )));
    assert_eq!(anim_b["writes"][0]["path"], "face/b.smile");
    assert!((anim_b["writes"][0]["value"]["data"].as_f64().unwrap() - 0.5).abs() < 0.0001);

    let anim_a_second = unwrap_value(&update_nodes_writes(Some(
      json!({ "controllerId": "anim:a", "dt": 0.25 }).to_string(),
    )));
    assert_eq!(anim_a_second["writes"][0]["path"], "face/a.smile");
    assert!(
      (anim_a_second["writes"][0]["value"]["data"]
        .as_f64()
        .unwrap()
        - 0.5)
        .abs()
        < 0.0001
    );
  }
}
