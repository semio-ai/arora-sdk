mod arora_generated;

use serde::Deserialize;
use serde_json::{json, Value as JsonValue};
use std::sync::{Mutex, OnceLock};
use vizij_api_core::{json as api_json, Shape, TypedPath, Value as ApiValue, WriteBatch};
use vizij_graph_core::{evaluate_all, evaluate_all_cached, GraphRuntime, GraphSpec};

static FACADE: OnceLock<Mutex<NodeGraphModuleFacade>> = OnceLock::new();

fn facade() -> &'static Mutex<NodeGraphModuleFacade> {
  FACADE.get_or_init(|| Mutex::new(NodeGraphModuleFacade::new()))
}

fn with_facade<T>(
  f: impl FnOnce(&mut NodeGraphModuleFacade) -> Result<T, String>,
) -> Result<T, String> {
  let mut guard = facade()
    .lock()
    .map_err(|_| "vizij node graph facade lock is poisoned".to_string())?;
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
enum GraphSpecEnvelope {
  Wrapped { spec: JsonValue },
  Direct(JsonValue),
}

#[derive(Deserialize)]
struct StageInputRequest {
  path: String,
  value: JsonValue,
  #[serde(default)]
  shape: Option<JsonValue>,
}

#[derive(Deserialize)]
struct EvaluateRequest {
  dt: f32,
}

#[derive(Debug)]
pub struct NodeGraphModuleFacade {
  spec: Option<GraphSpec>,
  runtime: GraphRuntime,
  plan_ready: bool,
}

impl Default for NodeGraphModuleFacade {
  fn default() -> Self {
    Self::new()
  }
}

impl NodeGraphModuleFacade {
  pub fn new() -> Self {
    Self {
      spec: None,
      runtime: GraphRuntime::default(),
      plan_ready: false,
    }
  }

  pub fn reset(&mut self) {
    *self = Self::new();
  }

  pub fn normalize_graph_value(mut spec: JsonValue) -> Result<JsonValue, String> {
    api_json::normalize_graph_spec_value(&mut spec)
      .map_err(|error| format!("normalize graph spec error: {error}"))?;
    Ok(spec)
  }

  pub fn load_graph_value(&mut self, spec: JsonValue) -> Result<(), String> {
    let normalized = Self::normalize_graph_value(spec)?;
    let spec = serde_json::from_value::<GraphSpec>(normalized)
      .map_err(|error| format!("graph spec deserialize error: {error}"))?
      .with_cache();
    self.spec = Some(spec);
    self.runtime = GraphRuntime::default();
    self.plan_ready = false;
    Ok(())
  }

  pub fn stage_input_value(
    &mut self,
    path: String,
    value: JsonValue,
    shape: Option<JsonValue>,
  ) -> Result<(), String> {
    let typed_path =
      TypedPath::parse(&path).map_err(|error| format!("invalid input path '{path}': {error}"))?;
    let normalized = api_json::normalize_value_json_staging(value);
    let value = serde_json::from_value::<ApiValue>(normalized)
      .map_err(|error| format!("invalid input value for '{path}': {error}"))?;
    let shape = match shape {
      Some(shape_value) => Some(
        serde_json::from_value::<Shape>(shape_value)
          .map_err(|error| format!("invalid input shape for '{path}': {error}"))?,
      ),
      None => None,
    };
    self.runtime.set_input(typed_path, value, shape);
    Ok(())
  }

  pub fn evaluate_writebatch(&mut self, dt: f32) -> Result<WriteBatch, String> {
    if !dt.is_finite() || dt < 0.0 {
      return Err("dt must be finite and non-negative".to_string());
    }
    let spec = self
      .spec
      .as_ref()
      .ok_or_else(|| "graph is not loaded; call load_graph first".to_string())?;
    self.runtime.dt = dt;
    self.runtime.t = if self.runtime.t.is_finite() {
      self.runtime.t + dt
    } else {
      dt
    };

    let result = if self.plan_ready {
      evaluate_all_cached(&mut self.runtime, spec)
    } else {
      evaluate_all(&mut self.runtime, spec)
    };
    match result {
      Ok(()) => self.plan_ready = true,
      Err(error) => {
        self.plan_ready = false;
        return Err(format!("evaluate_all error: {error}"));
      }
    }

    Ok(std::mem::take(&mut self.runtime.writes))
  }
}

fn graph_value_from_envelope(envelope: GraphSpecEnvelope) -> JsonValue {
  match envelope {
    GraphSpecEnvelope::Wrapped { spec } | GraphSpecEnvelope::Direct(spec) => spec,
  }
}

fn reset_graph() -> String {
  match facade().lock() {
    Ok(mut guard) => {
      guard.reset();
      ok(json!({ "reset": true }))
    }
    Err(_) => err("vizij node graph facade lock is poisoned"),
  }
}

fn load_graph(request_json: Option<String>) -> String {
  let request = match parse_request::<GraphSpecEnvelope>(request_json) {
    Ok(request) => request,
    Err(error) => return err(error),
  };
  let spec = graph_value_from_envelope(request);
  match with_facade(|facade| facade.load_graph_value(spec)) {
    Ok(()) => ok(json!({ "loaded": true })),
    Err(error) => err(error),
  }
}

fn stage_input(request_json: Option<String>) -> String {
  let request = match parse_request::<StageInputRequest>(request_json) {
    Ok(request) => request,
    Err(error) => return err(error),
  };
  let path = request.path.clone();
  match with_facade(|facade| facade.stage_input_value(request.path, request.value, request.shape)) {
    Ok(()) => ok(json!({ "path": path })),
    Err(error) => err(error),
  }
}

fn evaluate(request_json: Option<String>) -> String {
  let request = match parse_request::<EvaluateRequest>(request_json) {
    Ok(request) => request,
    Err(error) => return err(error),
  };
  match with_facade(|facade| {
    let batch = facade.evaluate_writebatch(request.dt)?;
    serde_json::to_value(&batch)
      .map(|writes| json!({ "nodes": {}, "writes": writes }))
      .map_err(|error| format!("failed to serialize write batch: {error}"))
  }) {
    Ok(value) => ok(value),
    Err(error) => err(error),
  }
}

fn normalize_graph(request_json: Option<String>) -> String {
  let request = match parse_request::<GraphSpecEnvelope>(request_json) {
    Ok(request) => request,
    Err(error) => return err(error),
  };
  match NodeGraphModuleFacade::normalize_graph_value(graph_value_from_envelope(request)) {
    Ok(value) => ok(value),
    Err(error) => err(error),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn fixture_graph() -> JsonValue {
    json!({
      "nodes": [
        {
          "id": "source",
          "type": "constant",
          "params": { "value": { "type": "float", "data": 3.0 } }
        },
        {
          "id": "out",
          "type": "output",
          "params": { "path": "face/smile.amount" }
        }
      ],
      "edges": [
        {
          "from": { "node_id": "source", "output": "out" },
          "to": { "node_id": "out", "input": "in" }
        }
      ]
    })
  }

  fn unwrap_value(response: &str) -> JsonValue {
    let parsed: JsonValue = serde_json::from_str(response).expect("response json");
    assert_eq!(parsed["ok"], true, "{parsed}");
    parsed["value"].clone()
  }

  #[test]
  fn facade_loads_and_evaluates_graph_writes() {
    unwrap_value(&reset_graph());
    unwrap_value(&load_graph(Some(
      json!({ "spec": fixture_graph() }).to_string(),
    )));

    let update = unwrap_value(&evaluate(Some(json!({ "dt": 1.0 / 60.0 }).to_string())));
    let writes = update["writes"].as_array().expect("writes array");
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0]["path"], "face/smile.amount");
    assert_eq!(writes[0]["value"], json!({ "type": "float", "data": 3.0 }));
  }

  #[test]
  fn reusable_facade_stages_inputs() {
    let mut facade = NodeGraphModuleFacade::new();
    facade
      .load_graph_value(json!({
        "nodes": [
          {
            "id": "in",
            "type": "input",
            "params": { "path": "control/smile.amount" }
          },
          {
            "id": "out",
            "type": "output",
            "params": { "path": "face/smile.amount" }
          }
        ],
        "edges": [
          {
            "from": { "node_id": "in", "output": "out" },
            "to": { "node_id": "out", "input": "in" }
          }
        ]
      }))
      .expect("load graph");
    facade
      .stage_input_value(
        "control/smile.amount".to_string(),
        json!({ "type": "float", "data": 0.75 }),
        None,
      )
      .expect("stage input");

    let writes = facade.evaluate_writebatch(1.0 / 60.0).expect("evaluate");
    let write = writes.iter().next().expect("write");
    assert_eq!(write.path.to_string(), "face/smile.amount");
    assert_eq!(write.value, ApiValue::Float(0.75));
  }
}
