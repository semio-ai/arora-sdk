mod arora_generated;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use vizij_animation::AnimationModuleFacade;
use vizij_api_core::{json as api_json, Shape, TypedPath, Value as ApiValue, WriteBatch};
use vizij_node_graph::NodeGraphModuleFacade;

pub const MODULE_FACADE_VERSION: u32 = 1;

static FACADE: OnceLock<Mutex<ComposedOrchestratorFacade>> = OnceLock::new();

fn facade() -> &'static Mutex<ComposedOrchestratorFacade> {
  FACADE.get_or_init(|| Mutex::new(ComposedOrchestratorFacade::new()))
}

fn dispatch_json(request_json: Option<String>) -> String {
  let request_json = match request_json {
    Some(request_json) => request_json,
    None => {
      return FacadeResponse::error(None, "missing request_json".to_string()).to_json_string();
    }
  };

  match facade().lock() {
    Ok(mut guard) => guard.dispatch_json(&request_json),
    Err(_) => FacadeResponse::error(
      None,
      "vizij composed orchestrator facade lock is poisoned".to_string(),
    )
    .to_json_string(),
  }
}

#[derive(Debug)]
pub struct ComposedOrchestratorFacade {
  runtime: Option<ComposedRuntime>,
  runtime_handle: Option<String>,
  runtime_counter: u64,
  graph_counter: u32,
  anim_counter: u32,
}

#[derive(Debug)]
struct ComposedRuntime {
  schedule: Schedule,
  epoch: u64,
  graphs: BTreeMap<String, GraphModule>,
  anims: BTreeMap<String, AnimationModuleFacade>,
  blackboard: BTreeMap<String, BlackboardEntry>,
}

#[derive(Debug)]
struct GraphModule {
  facade: NodeGraphModuleFacade,
}

#[derive(Debug, Clone)]
struct BlackboardEntry {
  value: ApiValue,
  shape: Option<Shape>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Schedule {
  SinglePass,
  TwoPass,
  RateDecoupled,
}

impl Default for ComposedOrchestratorFacade {
  fn default() -> Self {
    Self::new()
  }
}

impl ComposedOrchestratorFacade {
  pub fn new() -> Self {
    Self {
      runtime: None,
      runtime_handle: None,
      runtime_counter: 0,
      graph_counter: 0,
      anim_counter: 0,
    }
  }

  pub fn dispatch_json(&mut self, request_json: &str) -> String {
    let request: FacadeRequest = match serde_json::from_str(request_json) {
      Ok(request) => request,
      Err(error) => {
        return FacadeResponse::error(None, format!("invalid facade request: {error}"))
          .to_json_string();
      }
    };
    self.dispatch(request).to_json_string()
  }

  pub fn dispatch(&mut self, request: FacadeRequest) -> FacadeResponse {
    let request_id = request.request_id.clone();
    match self.dispatch_inner(request) {
      Ok(result) => FacadeResponse::ok(request_id, result),
      Err(error) => FacadeResponse::error(request_id, error),
    }
  }

  fn dispatch_inner(&mut self, request: FacadeRequest) -> Result<JsonValue, String> {
    match request.call.as_str() {
      "runtime.create" => self.create_runtime(request.args),
      "runtime.dispose" => {
        self.validate_runtime_handle(request.runtime_handle.as_deref())?;
        self.dispose_runtime()
      }
      "controllers.list" | "runtime.controllers" => {
        self.validate_runtime_handle(request.runtime_handle.as_deref())?;
        self.list_controllers()
      }
      "graph.normalize" | "graph.normalizeSpec" | "graph.normalize_spec" => {
        self.normalize_graph(request.args)
      }
      "graph.register" | "graph.load" => {
        self.validate_runtime_handle(request.runtime_handle.as_deref())?;
        self.register_graph(request.args)
      }
      "graph.remove" => {
        self.validate_runtime_handle(request.runtime_handle.as_deref())?;
        self.remove_graph(request.args)
      }
      "animation.register" | "animation.load" => {
        self.validate_runtime_handle(request.runtime_handle.as_deref())?;
        self.register_animation(request.args)
      }
      "animation.remove" => {
        self.validate_runtime_handle(request.runtime_handle.as_deref())?;
        self.remove_animation(request.args)
      }
      "input.set" => {
        self.validate_runtime_handle(request.runtime_handle.as_deref())?;
        self.set_input(request.args)
      }
      "input.remove" => {
        self.validate_runtime_handle(request.runtime_handle.as_deref())?;
        self.remove_input(request.args)
      }
      "orchestrator.step" | "orchestrator.stepDelta" | "orchestrator.step_delta" => {
        self.validate_runtime_handle(request.runtime_handle.as_deref())?;
        self.step(request.args)
      }
      other => Err(format!("unknown facade call '{other}'")),
    }
  }

  fn create_runtime(&mut self, args: JsonValue) -> Result<JsonValue, String> {
    let args = parse_args::<RuntimeCreateArgs>(args)?;
    let schedule = parse_schedule(args.schedule.as_deref())?;
    let handle = args.runtime_handle.unwrap_or_else(|| {
      let handle = format!("runtime:{}", self.runtime_counter);
      self.runtime_counter = self.runtime_counter.wrapping_add(1);
      handle
    });

    self.runtime = Some(ComposedRuntime {
      schedule,
      epoch: 0,
      graphs: BTreeMap::new(),
      anims: BTreeMap::new(),
      blackboard: BTreeMap::new(),
    });
    self.runtime_handle = Some(handle.clone());
    self.graph_counter = 0;
    self.anim_counter = 0;

    Ok(json!({
      "runtimeHandle": handle,
      "schedule": schedule_name(schedule),
      "composition": "independent-modules",
    }))
  }

  fn dispose_runtime(&mut self) -> Result<JsonValue, String> {
    let disposed = self.runtime.take().is_some();
    self.runtime_handle = None;
    Ok(json!({ "disposed": disposed }))
  }

  fn list_controllers(&mut self) -> Result<JsonValue, String> {
    let runtime = self.runtime_mut()?;
    let graphs: Vec<String> = runtime.graphs.keys().cloned().collect();
    let anims: Vec<String> = runtime.anims.keys().cloned().collect();
    Ok(json!({ "graphs": graphs, "anims": anims }))
  }

  fn normalize_graph(&mut self, args: JsonValue) -> Result<JsonValue, String> {
    let spec = parse_args::<GraphNormalizeArgs>(args)?.spec;
    NodeGraphModuleFacade::normalize_graph_value(spec)
  }

  fn register_graph(&mut self, args: JsonValue) -> Result<JsonValue, String> {
    let args = parse_args::<GraphRegistrationArgs>(args)?;
    let id = args.id.unwrap_or_else(|| self.next_graph_id());
    let mut graph = NodeGraphModuleFacade::new();
    graph.load_graph_value(args.spec)?;
    self
      .runtime_mut()?
      .graphs
      .insert(id.clone(), GraphModule { facade: graph });
    Ok(json!({ "graphId": id, "module": "vizij-node-graph" }))
  }

  fn remove_graph(&mut self, args: JsonValue) -> Result<JsonValue, String> {
    let args = parse_args::<RemoveControllerArgs>(args)?;
    let removed = self.runtime_mut()?.graphs.remove(&args.id).is_some();
    Ok(json!({ "removed": removed }))
  }

  fn register_animation(&mut self, args: JsonValue) -> Result<JsonValue, String> {
    let args = parse_args::<AnimationRegistrationArgs>(args)?;
    let id = args.id.unwrap_or_else(|| self.next_anim_id());
    let mut animation = AnimationModuleFacade::new();
    if let Some(setup) = args.setup {
      animation.configure_from_setup_value(setup)?;
    }
    self.runtime_mut()?.anims.insert(id.clone(), animation);
    Ok(json!({ "animationId": id, "module": "vizij-animation" }))
  }

  fn remove_animation(&mut self, args: JsonValue) -> Result<JsonValue, String> {
    let args = parse_args::<RemoveControllerArgs>(args)?;
    let removed = self.runtime_mut()?.anims.remove(&args.id).is_some();
    Ok(json!({ "removed": removed }))
  }

  fn set_input(&mut self, args: JsonValue) -> Result<JsonValue, String> {
    let args = parse_args::<SetInputArgs>(args)?;
    let path = TypedPath::parse(&args.path)
      .map_err(|error| format!("invalid input path '{}': {error}", args.path))?;
    let normalized = api_json::normalize_value_json_staging(args.value);
    let value = serde_json::from_value::<ApiValue>(normalized)
      .map_err(|error| format!("invalid input value for '{}': {error}", args.path))?;
    let shape = match args.shape {
      Some(shape) => Some(
        serde_json::from_value::<Shape>(shape)
          .map_err(|error| format!("invalid input shape for '{}': {error}", args.path))?,
      ),
      None => None,
    };
    let path_string = path.to_string();
    self
      .runtime_mut()?
      .blackboard
      .insert(path_string.clone(), BlackboardEntry { value, shape });
    Ok(json!({ "path": path_string }))
  }

  fn remove_input(&mut self, args: JsonValue) -> Result<JsonValue, String> {
    let args = parse_args::<RemoveInputArgs>(args)?;
    let removed = self.runtime_mut()?.blackboard.remove(&args.path).is_some();
    Ok(json!({ "removed": removed }))
  }

  fn step(&mut self, args: JsonValue) -> Result<JsonValue, String> {
    let args = parse_args::<StepArgs>(args)?;
    if !args.dt.is_finite() || args.dt < 0.0 {
      return Err("dt must be finite and non-negative".to_string());
    }
    let runtime = self.runtime_mut()?;
    runtime.epoch = runtime.epoch.wrapping_add(1);
    let mut merged_writes = WriteBatch::new();

    match runtime.schedule {
      Schedule::SinglePass | Schedule::RateDecoupled => {
        run_animation_pass(runtime, args.dt, &mut merged_writes)?;
        run_graph_pass(runtime, args.dt, &mut merged_writes)?;
      }
      Schedule::TwoPass => {
        run_graph_pass(runtime, args.dt, &mut merged_writes)?;
        run_animation_pass(runtime, args.dt, &mut merged_writes)?;
        run_graph_pass(runtime, args.dt, &mut merged_writes)?;
      }
    }

    Ok(json!({
      "epoch": runtime.epoch,
      "dt": args.dt,
      "merged_writes": merged_writes,
      "conflicts": [],
      "events": [],
      "timings_ms": {
        "total_ms": args.dt * 1000.0,
        "composition": 1.0
      },
    }))
  }

  fn runtime_mut(&mut self) -> Result<&mut ComposedRuntime, String> {
    self
      .runtime
      .as_mut()
      .ok_or_else(|| "runtime is not created; call runtime.create first".to_string())
  }

  fn validate_runtime_handle(&self, requested: Option<&str>) -> Result<(), String> {
    let Some(requested) = requested else {
      return Ok(());
    };
    let Some(current) = self.runtime_handle.as_deref() else {
      return Err("runtime is not created; call runtime.create first".to_string());
    };
    if requested != current {
      return Err(format!(
        "runtime handle mismatch: request targeted '{requested}' but active runtime is '{current}'"
      ));
    }
    Ok(())
  }

  fn next_graph_id(&mut self) -> String {
    let id = format!("graph:{}", self.graph_counter);
    self.graph_counter = self.graph_counter.wrapping_add(1);
    id
  }

  fn next_anim_id(&mut self) -> String {
    let id = format!("anim:{}", self.anim_counter);
    self.anim_counter = self.anim_counter.wrapping_add(1);
    id
  }
}

fn run_animation_pass(
  runtime: &mut ComposedRuntime,
  dt: f32,
  merged_writes: &mut WriteBatch,
) -> Result<(), String> {
  let ids: Vec<String> = runtime.anims.keys().cloned().collect();
  for id in ids {
    let batch = runtime
      .anims
      .get_mut(&id)
      .ok_or_else(|| format!("animation '{id}' disappeared during pass"))?
      .update_writebatch(dt, None)?;
    apply_writes(runtime, batch, merged_writes)?;
  }
  Ok(())
}

fn run_graph_pass(
  runtime: &mut ComposedRuntime,
  dt: f32,
  merged_writes: &mut WriteBatch,
) -> Result<(), String> {
  let ids: Vec<String> = runtime.graphs.keys().cloned().collect();
  for id in ids {
    let staged = runtime.blackboard.clone();
    let graph = runtime
      .graphs
      .get_mut(&id)
      .ok_or_else(|| format!("graph '{id}' disappeared during pass"))?;
    for (path, entry) in staged {
      let value = serde_json::to_value(&entry.value)
        .map_err(|error| format!("failed to stage graph input '{path}': {error}"))?;
      let shape = match entry.shape {
        Some(shape) => Some(
          serde_json::to_value(shape)
            .map_err(|error| format!("failed to stage graph input shape '{path}': {error}"))?,
        ),
        None => None,
      };
      graph.facade.stage_input_value(path, value, shape)?;
    }
    let batch = graph.facade.evaluate_writebatch(dt)?;
    apply_writes(runtime, batch, merged_writes)?;
  }
  Ok(())
}

fn apply_writes(
  runtime: &mut ComposedRuntime,
  batch: WriteBatch,
  merged_writes: &mut WriteBatch,
) -> Result<(), String> {
  for op in batch.iter() {
    runtime.blackboard.insert(
      op.path.to_string(),
      BlackboardEntry {
        value: op.value.clone(),
        shape: op.shape.clone(),
      },
    );
  }
  merged_writes.append(batch);
  Ok(())
}

#[derive(Debug, Clone, Deserialize)]
pub struct FacadeRequest {
  pub call: String,
  #[serde(default, rename = "runtimeHandle", alias = "runtime_handle")]
  pub runtime_handle: Option<String>,
  #[serde(default, rename = "requestId", alias = "request_id")]
  pub request_id: Option<String>,
  #[serde(default)]
  pub args: JsonValue,
}

#[derive(Debug, Clone, Serialize)]
pub struct FacadeResponse {
  pub ok: bool,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub result: Option<JsonValue>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub error: Option<String>,
  pub version: u32,
  #[serde(skip_serializing_if = "Option::is_none", rename = "requestId")]
  pub request_id: Option<String>,
}

impl FacadeResponse {
  fn ok(request_id: Option<String>, result: JsonValue) -> Self {
    Self {
      ok: true,
      result: Some(result),
      error: None,
      version: MODULE_FACADE_VERSION,
      request_id,
    }
  }

  fn error(request_id: Option<String>, error: String) -> Self {
    Self {
      ok: false,
      result: None,
      error: Some(error),
      version: MODULE_FACADE_VERSION,
      request_id,
    }
  }

  fn to_json_string(&self) -> String {
    serde_json::to_string(self).unwrap_or_else(|error| {
      format!(
        "{{\"ok\":false,\"error\":\"failed to serialize facade response: {error}\",\"version\":{MODULE_FACADE_VERSION}}}"
      )
    })
  }
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeCreateArgs {
  #[serde(default)]
  schedule: Option<String>,
  #[serde(default, alias = "runtime_handle")]
  runtime_handle: Option<String>,
}

#[derive(Deserialize)]
struct GraphNormalizeArgs {
  spec: JsonValue,
}

#[derive(Deserialize)]
struct GraphRegistrationArgs {
  #[serde(default)]
  id: Option<String>,
  spec: JsonValue,
}

#[derive(Deserialize)]
struct AnimationRegistrationArgs {
  #[serde(default)]
  id: Option<String>,
  #[serde(default)]
  setup: Option<JsonValue>,
}

#[derive(Deserialize)]
struct RemoveControllerArgs {
  id: String,
}

#[derive(Deserialize)]
struct SetInputArgs {
  path: String,
  value: JsonValue,
  #[serde(default)]
  shape: Option<JsonValue>,
}

#[derive(Deserialize)]
struct RemoveInputArgs {
  path: String,
}

#[derive(Deserialize)]
struct StepArgs {
  dt: f32,
}

fn parse_args<T: for<'de> Deserialize<'de>>(args: JsonValue) -> Result<T, String> {
  serde_json::from_value(args).map_err(|error| format!("invalid facade args: {error}"))
}

fn parse_schedule(schedule: Option<&str>) -> Result<Schedule, String> {
  match schedule {
    None | Some("SinglePass") | Some("singlePass") | Some("single_pass") => {
      Ok(Schedule::SinglePass)
    }
    Some("TwoPass") | Some("twoPass") | Some("two_pass") => Ok(Schedule::TwoPass),
    Some("RateDecoupled") | Some("rateDecoupled") | Some("rate_decoupled") => {
      Ok(Schedule::RateDecoupled)
    }
    Some(other) => Err(format!("unknown schedule option '{other}'")),
  }
}

fn schedule_name(schedule: Schedule) -> &'static str {
  match schedule {
    Schedule::SinglePass => "SinglePass",
    Schedule::TwoPass => "TwoPass",
    Schedule::RateDecoupled => "RateDecoupled",
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn unwrap_result(response: &str) -> JsonValue {
    let parsed: JsonValue = serde_json::from_str(response).expect("response json");
    assert_eq!(parsed["ok"], true, "{parsed}");
    parsed["result"].clone()
  }

  fn call(facade: &mut ComposedOrchestratorFacade, name: &str, args: JsonValue) -> JsonValue {
    unwrap_result(
      &facade.dispatch_json(
        &json!({
          "call": name,
          "requestId": format!("req:{name}"),
          "args": args,
        })
        .to_string(),
      ),
    )
  }

  fn fixture_animation() -> JsonValue {
    json!({
      "id": "composed-animation",
      "name": "Composed Animation",
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
          "params": { "path": "face/graph.value" }
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

  #[test]
  fn composed_facade_steps_animation_and_graph_modules() {
    let mut facade = ComposedOrchestratorFacade::new();
    let runtime = call(
      &mut facade,
      "runtime.create",
      json!({ "schedule": "SinglePass" }),
    );
    assert_eq!(runtime["composition"], "independent-modules");

    let graph = call(
      &mut facade,
      "graph.register",
      json!({ "id": "graph:smoke", "spec": fixture_graph() }),
    );
    assert_eq!(graph["module"], "vizij-node-graph");

    let animation = call(
      &mut facade,
      "animation.register",
      json!({
        "id": "anim:smoke",
        "setup": {
          "animation": fixture_animation(),
          "instance": { "timescale": 1.0, "active": true }
        }
      }),
    );
    assert_eq!(animation["module"], "vizij-animation");

    let frame = call(&mut facade, "orchestrator.step", json!({ "dt": 0.5 }));
    let writes = frame["merged_writes"].as_array().expect("writes array");
    assert!(
      writes
        .iter()
        .any(|write| write["path"] == "face/smile.amount"),
      "animation write missing: {writes:?}"
    );
    assert!(
      writes
        .iter()
        .any(|write| write["path"] == "face/graph.value"),
      "graph write missing: {writes:?}"
    );
  }
}
