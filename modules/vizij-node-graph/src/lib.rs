mod arora_generated;

use serde::Deserialize;
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use vizij_api_core::{json as api_json, Shape, TypedPath, Value as ApiValue, WriteBatch};
use vizij_graph_core::{evaluate_all, evaluate_all_cached, GraphRuntime, GraphSpec};

static FACADE: OnceLock<Mutex<NodeGraphModuleManager>> = OnceLock::new();

fn facade() -> &'static Mutex<NodeGraphModuleManager> {
  FACADE.get_or_init(|| Mutex::new(NodeGraphModuleManager::new()))
}

fn with_facade<T>(
  f: impl FnOnce(&mut NodeGraphModuleManager) -> Result<T, String>,
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
  Wrapped {
    #[serde(default, rename = "graphId", alias = "graph_id", alias = "id")]
    graph_id: Option<String>,
    spec: JsonValue,
  },
  Direct(JsonValue),
}

#[derive(Deserialize)]
struct StageInputRequest {
  #[serde(default, rename = "graphId", alias = "graph_id", alias = "id")]
  graph_id: Option<String>,
  path: String,
  value: JsonValue,
  #[serde(default)]
  shape: Option<JsonValue>,
}

#[derive(Deserialize)]
struct EvaluateRequest {
  #[serde(default, rename = "graphId", alias = "graph_id", alias = "id")]
  graph_id: Option<String>,
  dt: f32,
}

#[derive(Deserialize)]
struct RemoveGraphRequest {
  #[serde(rename = "graphId", alias = "graph_id", alias = "id")]
  graph_id: String,
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
    let spec = parse_graph_spec_value(spec)?;
    self.spec = Some(spec);
    self.runtime = GraphRuntime::default();
    self.plan_ready = false;
    Ok(())
  }

  pub fn replace_graph_value(&mut self, spec: JsonValue) -> Result<(), String> {
    self.spec = Some(parse_graph_spec_value(spec)?);
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

#[derive(Debug)]
pub struct NodeGraphModuleManager {
  default: NodeGraphModuleFacade,
  graphs: HashMap<String, NodeGraphModuleFacade>,
}

impl Default for NodeGraphModuleManager {
  fn default() -> Self {
    Self::new()
  }
}

impl NodeGraphModuleManager {
  pub fn new() -> Self {
    Self {
      default: NodeGraphModuleFacade::new(),
      graphs: HashMap::new(),
    }
  }

  pub fn reset(&mut self) {
    *self = Self::new();
  }

  fn facade_mut(&mut self, graph_id: Option<&str>) -> Result<&mut NodeGraphModuleFacade, String> {
    match graph_id {
      Some(id) => self
        .graphs
        .get_mut(id)
        .ok_or_else(|| format!("graph '{id}' is not loaded")),
      None => Ok(&mut self.default),
    }
  }

  pub fn load_graph_value(
    &mut self,
    graph_id: Option<String>,
    spec: JsonValue,
  ) -> Result<(), String> {
    match graph_id {
      Some(id) => self.graphs.entry(id).or_default().load_graph_value(spec),
      None => self.default.load_graph_value(spec),
    }
  }

  pub fn replace_graph_value(
    &mut self,
    graph_id: Option<&str>,
    spec: JsonValue,
  ) -> Result<(), String> {
    self.facade_mut(graph_id)?.replace_graph_value(spec)
  }

  pub fn stage_input_value(
    &mut self,
    graph_id: Option<&str>,
    path: String,
    value: JsonValue,
    shape: Option<JsonValue>,
  ) -> Result<(), String> {
    self
      .facade_mut(graph_id)?
      .stage_input_value(path, value, shape)
  }

  pub fn evaluate_writebatch(
    &mut self,
    graph_id: Option<&str>,
    dt: f32,
  ) -> Result<WriteBatch, String> {
    self.facade_mut(graph_id)?.evaluate_writebatch(dt)
  }

  pub fn remove_graph(&mut self, graph_id: &str) -> bool {
    self.graphs.remove(graph_id).is_some()
  }
}

fn parse_graph_spec_value(spec: JsonValue) -> Result<GraphSpec, String> {
  let normalized = NodeGraphModuleFacade::normalize_graph_value(spec)?;
  serde_json::from_value::<GraphSpec>(normalized)
    .map_err(|error| format!("graph spec deserialize error: {error}"))
    .map(GraphSpec::with_cache)
}

fn graph_value_from_envelope(envelope: GraphSpecEnvelope) -> (Option<String>, JsonValue) {
  match envelope {
    GraphSpecEnvelope::Wrapped { graph_id, spec } => (graph_id, spec),
    GraphSpecEnvelope::Direct(spec) => (None, spec),
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
  let (graph_id, spec) = graph_value_from_envelope(request);
  match with_facade(|facade| facade.load_graph_value(graph_id.clone(), spec)) {
    Ok(()) => ok(json!({ "graphId": graph_id, "loaded": true })),
    Err(error) => err(error),
  }
}

fn replace_graph(request_json: Option<String>) -> String {
  let request = match parse_request::<GraphSpecEnvelope>(request_json) {
    Ok(request) => request,
    Err(error) => return err(error),
  };
  let (graph_id, spec) = graph_value_from_envelope(request);
  match with_facade(|facade| facade.replace_graph_value(graph_id.as_deref(), spec)) {
    Ok(()) => ok(json!({ "graphId": graph_id, "replaced": true })),
    Err(error) => err(error),
  }
}

fn stage_input(request_json: Option<String>) -> String {
  let request = match parse_request::<StageInputRequest>(request_json) {
    Ok(request) => request,
    Err(error) => return err(error),
  };
  let path = request.path.clone();
  let graph_id = request.graph_id.clone();
  match with_facade(|facade| {
    facade.stage_input_value(
      graph_id.as_deref(),
      request.path,
      request.value,
      request.shape,
    )
  }) {
    Ok(()) => ok(json!({ "graphId": graph_id, "path": path })),
    Err(error) => err(error),
  }
}

fn evaluate(request_json: Option<String>) -> String {
  let request = match parse_request::<EvaluateRequest>(request_json) {
    Ok(request) => request,
    Err(error) => return err(error),
  };
  let graph_id = request.graph_id.clone();
  match with_facade(|facade| {
    let batch = facade.evaluate_writebatch(graph_id.as_deref(), request.dt)?;
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
  let (_graph_id, spec) = graph_value_from_envelope(request);
  match NodeGraphModuleFacade::normalize_graph_value(spec) {
    Ok(value) => ok(value),
    Err(error) => err(error),
  }
}

fn remove_graph(request_json: Option<String>) -> String {
  let request = match parse_request::<RemoveGraphRequest>(request_json) {
    Ok(request) => request,
    Err(error) => return err(error),
  };
  let graph_id = request.graph_id;
  match with_facade(|facade| Ok(facade.remove_graph(&graph_id))) {
    Ok(removed) => ok(json!({ "graphId": graph_id, "removed": removed })),
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

  fn graph_time_output(path: &str) -> JsonValue {
    json!({
      "nodes": [
        {
          "id": "time",
          "type": "time"
        },
        {
          "id": "out",
          "type": "output",
          "params": { "path": path }
        }
      ],
      "edges": [
        {
          "from": { "node_id": "time", "output": "out" },
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

  #[test]
  fn graph_handles_preserve_independent_runtime_state() {
    unwrap_value(&reset_graph());
    unwrap_value(&load_graph(Some(
      json!({
        "graphId": "graph:a",
        "spec": graph_time_output("face/a.time")
      })
      .to_string(),
    )));
    unwrap_value(&load_graph(Some(
      json!({
        "graphId": "graph:b",
        "spec": graph_time_output("face/b.time")
      })
      .to_string(),
    )));

    let graph_a_first = unwrap_value(&evaluate(Some(
      json!({ "graphId": "graph:a", "dt": 0.25 }).to_string(),
    )));
    assert_eq!(graph_a_first["writes"][0]["path"], "face/a.time");
    assert!(
      (graph_a_first["writes"][0]["value"]["data"]
        .as_f64()
        .unwrap()
        - 0.25)
        .abs()
        < 0.0001
    );

    let graph_b = unwrap_value(&evaluate(Some(
      json!({ "graphId": "graph:b", "dt": 0.5 }).to_string(),
    )));
    assert_eq!(graph_b["writes"][0]["path"], "face/b.time");
    assert!((graph_b["writes"][0]["value"]["data"].as_f64().unwrap() - 0.5).abs() < 0.0001);

    let graph_a_second = unwrap_value(&evaluate(Some(
      json!({ "graphId": "graph:a", "dt": 0.25 }).to_string(),
    )));
    assert_eq!(graph_a_second["writes"][0]["path"], "face/a.time");
    assert!(
      (graph_a_second["writes"][0]["value"]["data"]
        .as_f64()
        .unwrap()
        - 0.5)
        .abs()
        < 0.0001
    );
  }
}
