mod arora_generated;

use serde::Deserialize;
use serde_json::{json, Value as JsonValue};
use std::sync::{Mutex, OnceLock};
use vizij_animation_core::{
  parse_stored_animation_json, AnimId, Config, Engine, Inputs, InstanceCfg, PlayerId,
};

static ENGINE: OnceLock<Mutex<Engine>> = OnceLock::new();

fn engine() -> &'static Mutex<Engine> {
  ENGINE.get_or_init(|| Mutex::new(Engine::new(Config::default())))
}

fn with_engine<T>(f: impl FnOnce(&mut Engine) -> Result<T, String>) -> Result<T, String> {
  let mut guard = engine()
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
  Wrapped { animation: JsonValue },
  Direct(JsonValue),
}

#[derive(Deserialize)]
struct CreatePlayerRequest {
  #[serde(default = "default_player_name")]
  name: String,
}

fn default_player_name() -> String {
  "arora-vizij-player".to_string()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddInstanceRequest {
  player_id: u32,
  animation_id: u32,
  #[serde(default)]
  config: Option<InstanceConfigPatch>,
}

#[derive(Deserialize)]
struct UpdateNodesWritesRequest {
  dt: f32,
  #[serde(default)]
  inputs: Option<Inputs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstanceConfigPatch {
  weight: Option<f32>,
  #[serde(alias = "time_scale")]
  time_scale: Option<f32>,
  #[serde(alias = "start_offset")]
  start_offset: Option<f32>,
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
    }
    if let Some(enabled) = patch.enabled {
      config.enabled = enabled;
    }
    config
  }
}

fn reset_engine() -> String {
  match engine().lock() {
    Ok(mut guard) => {
      *guard = Engine::new(Config::default());
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
  let animation = match request {
    StoredAnimationEnvelope::Wrapped { animation } | StoredAnimationEnvelope::Direct(animation) => {
      animation
    }
  };
  let animation_json = match animation {
    JsonValue::String(value) => value,
    other => match serde_json::to_string(&other) {
      Ok(value) => value,
      Err(error) => return err(format!("failed to serialize animation payload: {error}")),
    },
  };

  let data = match parse_stored_animation_json(&animation_json) {
    Ok(data) => data,
    Err(error) => return err(format!("failed to parse stored animation: {error}")),
  };

  match with_engine(|engine| Ok(engine.load_animation(data))) {
    Ok(id) => ok(json!({ "animationId": id.0 })),
    Err(error) => err(error),
  }
}

fn create_player(request_json: Option<String>) -> String {
  let request = match parse_request::<CreatePlayerRequest>(request_json) {
    Ok(request) => request,
    Err(error) => return err(error),
  };

  match with_engine(|engine| Ok(engine.create_player(&request.name))) {
    Ok(id) => ok(json!({ "playerId": id.0 })),
    Err(error) => err(error),
  }
}

fn add_instance(request_json: Option<String>) -> String {
  let request = match parse_request::<AddInstanceRequest>(request_json) {
    Ok(request) => request,
    Err(error) => return err(error),
  };
  let config = request.config.map(InstanceCfg::from).unwrap_or_default();

  match with_engine(|engine| {
    Ok(engine.add_instance(
      PlayerId(request.player_id),
      AnimId(request.animation_id),
      config,
    ))
  }) {
    Ok(id) => ok(json!({ "instanceId": id.0 })),
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

  match with_engine(|engine| {
    let batch = engine.update_writebatch(request.dt, request.inputs.unwrap_or_default());
    serde_json::to_value(&batch)
      .map(|writes| json!({ "nodes": {}, "writes": writes }))
      .map_err(|error| format!("failed to serialize write batch: {error}"))
  }) {
    Ok(value) => ok(value),
    Err(error) => err(error),
  }
}

fn list_animations() -> String {
  match with_engine(|engine| {
    serde_json::to_value(engine.list_animations())
      .map_err(|error| format!("failed to serialize animation list: {error}"))
  }) {
    Ok(value) => ok(value),
    Err(error) => err(error),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::Value;

  fn fixture_animation() -> Value {
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
          "animatableId": "face/smile.amount",
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
}
