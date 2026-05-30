mod arora_generated;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
#[cfg(not(target_arch = "wasm32"))]
use vizij_animation::AnimationModuleFacade;
use vizij_animation_core::Inputs;
use vizij_api_core::{json as api_json, TypedPath, WriteBatch};
use vizij_graph_core::GraphSpec;
#[cfg(not(target_arch = "wasm32"))]
use vizij_node_graph::NodeGraphModuleFacade;
use vizij_orchestrator::controllers::animation::AnimationController;
use vizij_orchestrator::module_facade::filter_unchanged_writes;
use vizij_orchestrator::{
  Blackboard, ConflictLog, GraphControllerConfig, GraphMergeOptions, OutputConflictStrategy,
  Subscriptions,
};

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
  output_version: u64,
  last_version: u64,
  last_writes: WriteBatch,
}

#[derive(Debug)]
struct ComposedRuntime {
  schedule: Schedule,
  epoch: u64,
  graphs: IndexMap<String, GraphModule>,
  anims: IndexMap<String, AnimationModuleHandle>,
  blackboard: Blackboard,
}

#[derive(Debug)]
struct GraphModule {
  #[cfg(not(target_arch = "wasm32"))]
  facade: NodeGraphModuleFacade,
  subs: Subscriptions,
}

#[derive(Debug)]
struct AnimationModuleHandle {
  #[cfg(not(target_arch = "wasm32"))]
  facade: AnimationModuleFacade,
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
      output_version: 0,
      last_version: 0,
      last_writes: WriteBatch::default(),
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
      "graph.replace" => {
        self.validate_runtime_handle(request.runtime_handle.as_deref())?;
        self.replace_graph(request.args)
      }
      "graph.merge" | "graph.registerMerged" | "graph.register_merged" => {
        self.validate_runtime_handle(request.runtime_handle.as_deref())?;
        self.register_merged_graph(request.args)
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
      "orchestrator.step" => {
        self.validate_runtime_handle(request.runtime_handle.as_deref())?;
        self.step(request.args)
      }
      "orchestrator.stepDelta" | "orchestrator.step_delta" => {
        self.validate_runtime_handle(request.runtime_handle.as_deref())?;
        self.step_delta(request.args)
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

    if let Some(runtime) = self.runtime.as_ref() {
      clear_domain_handles(runtime)?;
    }

    self.runtime = Some(ComposedRuntime {
      schedule,
      epoch: 0,
      graphs: IndexMap::new(),
      anims: IndexMap::new(),
      blackboard: Blackboard::new(),
    });
    self.runtime_handle = Some(handle.clone());
    self.graph_counter = 0;
    self.anim_counter = 0;
    self.output_version = 0;
    self.last_version = 0;
    self.last_writes = WriteBatch::default();

    Ok(json!({
      "runtimeHandle": handle,
      "schedule": schedule_name(schedule),
      "composition": "independent-modules",
    }))
  }

  fn dispose_runtime(&mut self) -> Result<JsonValue, String> {
    let disposed = if let Some(runtime) = self.runtime.as_ref() {
      clear_domain_handles(runtime)?;
      self.runtime = None;
      true
    } else {
      false
    };
    self.runtime_handle = None;
    self.output_version = 0;
    self.last_version = 0;
    self.last_writes = WriteBatch::default();
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
    normalize_graph_module(spec)
  }

  fn register_graph(&mut self, args: JsonValue) -> Result<JsonValue, String> {
    let cfg = build_graph_controller_config(
      parse_args::<GraphRegistrationArgs>(args)?,
      self.next_graph_id(),
    )?;
    let id = cfg.id.clone();
    let spec = serde_json::to_value(&cfg.spec)
      .map_err(|error| format!("failed to serialize graph spec '{id}': {error}"))?;
    let graph = load_graph_module(&id, spec, cfg.subs)?;
    self.runtime_mut()?.graphs.insert(id.clone(), graph);
    self.reset_delta_baseline();
    Ok(json!({ "graphId": id, "module": "vizij-node-graph" }))
  }

  fn replace_graph(&mut self, args: JsonValue) -> Result<JsonValue, String> {
    let cfg = build_graph_controller_config(
      parse_args::<GraphRegistrationArgs>(args)?,
      self.next_graph_id(),
    )?;
    let id = cfg.id.clone();
    let spec = serde_json::to_value(&cfg.spec)
      .map_err(|error| format!("failed to serialize graph spec '{id}': {error}"))?;
    let runtime = self.runtime_mut()?;
    let module = runtime
      .graphs
      .get_mut(&id)
      .ok_or_else(|| format!("graph '{id}' is not registered"))?;
    replace_graph_module(&id, module, spec)?;
    module.subs = cfg.subs;
    self.reset_delta_baseline();
    Ok(json!({ "graphId": id, "replaced": true, "module": "vizij-node-graph" }))
  }

  fn register_merged_graph(&mut self, args: JsonValue) -> Result<JsonValue, String> {
    let args = parse_args::<MergedGraphRegistrationArgs>(args)?;
    if args.graphs.is_empty() {
      return Err("graph.merge requires at least one graph".to_string());
    }

    let merged_id = args.id.unwrap_or_else(|| self.next_graph_id());
    let options = map_merge_options(args.strategy)?;
    let mut configs = Vec::with_capacity(args.graphs.len());
    for (idx, graph) in args.graphs.into_iter().enumerate() {
      let fallback_id = format!("{merged_id}::{idx}");
      configs.push(build_graph_controller_config(graph, fallback_id)?);
    }

    let merged_cfg =
      GraphControllerConfig::merged_with_options(merged_id.clone(), configs, options)
        .map_err(|error| format!("graph merge error: {error}"))?;
    let spec = serde_json::to_value(&merged_cfg.spec)
      .map_err(|error| format!("failed to serialize merged graph spec '{merged_id}': {error}"))?;
    let graph = load_graph_module(&merged_id, spec, merged_cfg.subs)?;
    self.runtime_mut()?.graphs.insert(merged_id.clone(), graph);
    self.reset_delta_baseline();
    Ok(json!({ "graphId": merged_id, "module": "vizij-node-graph" }))
  }

  fn remove_graph(&mut self, args: JsonValue) -> Result<JsonValue, String> {
    let args = parse_args::<RemoveControllerArgs>(args)?;
    let removed = self.runtime_mut()?.graphs.shift_remove(&args.id).is_some();
    if removed {
      remove_graph_module(&args.id)?;
      self.reset_delta_baseline();
    }
    Ok(json!({ "removed": removed }))
  }

  fn register_animation(&mut self, args: JsonValue) -> Result<JsonValue, String> {
    let args = parse_args::<AnimationRegistrationArgs>(args)?;
    let id = args.id.unwrap_or_else(|| self.next_anim_id());
    let animation = configure_animation_module(&id, args.setup.unwrap_or(JsonValue::Null))?;
    self.runtime_mut()?.anims.insert(id.clone(), animation);
    self.reset_delta_baseline();
    Ok(json!({ "animationId": id, "module": "vizij-animation" }))
  }

  fn remove_animation(&mut self, args: JsonValue) -> Result<JsonValue, String> {
    let args = parse_args::<RemoveControllerArgs>(args)?;
    let removed = self.runtime_mut()?.anims.shift_remove(&args.id).is_some();
    if removed {
      remove_animation_module(&args.id)?;
      self.reset_delta_baseline();
    }
    Ok(json!({ "removed": removed }))
  }

  fn set_input(&mut self, args: JsonValue) -> Result<JsonValue, String> {
    let args = parse_args::<SetInputArgs>(args)?;
    let path = args.path.clone();
    let runtime = self.runtime_mut()?;
    runtime
      .blackboard
      .set(
        path.clone(),
        args.value,
        args.shape,
        runtime.epoch,
        "host".to_string(),
      )
      .map_err(|error| error.to_string())?;
    Ok(json!({ "path": path }))
  }

  fn remove_input(&mut self, args: JsonValue) -> Result<JsonValue, String> {
    let args = parse_args::<RemoveInputArgs>(args)?;
    let removed = self.runtime_mut()?.blackboard.remove(&args.path).is_some();
    Ok(json!({ "removed": removed }))
  }

  fn step(&mut self, args: JsonValue) -> Result<JsonValue, String> {
    let args = parse_args::<StepArgs>(args)?;
    let frame = self.step_runtime(args.dt)?;
    frame.to_json_value()
  }

  fn step_delta(&mut self, args: JsonValue) -> Result<JsonValue, String> {
    let args = parse_args::<StepDeltaArgs>(args)?;
    let frame = self.step_runtime(args.dt)?;
    self.output_version = self.output_version.saturating_add(1);
    let version = self.output_version;
    let since = args.since_version.unwrap_or(0);

    let merged_writes = if since == self.last_version {
      filter_unchanged_writes(&frame.merged_writes, &self.last_writes)
    } else {
      frame.merged_writes.clone()
    };

    self.last_version = version;
    self.last_writes = frame.merged_writes;

    Ok(json!({
      "version": version,
      "epoch": frame.epoch,
      "dt": frame.dt,
      "merged_writes": merged_writes,
      "conflicts": frame.conflicts,
      "events": frame.events,
      "timings_ms": frame.timings_ms,
    }))
  }

  fn step_runtime(&mut self, dt: f32) -> Result<ComposedFrame, String> {
    if !dt.is_finite() || dt < 0.0 {
      return Err("dt must be finite and non-negative".to_string());
    }
    let runtime = self.runtime_mut()?;
    runtime.epoch = runtime.epoch.wrapping_add(1);
    run_schedule(runtime, dt)
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

  fn reset_delta_baseline(&mut self) {
    self.last_version = 0;
    self.last_writes = WriteBatch::default();
  }
}

fn clear_domain_handles(runtime: &ComposedRuntime) -> Result<(), String> {
  for id in runtime.graphs.keys() {
    remove_graph_module(id)?;
  }
  for id in runtime.anims.keys() {
    remove_animation_module(id)?;
  }
  Ok(())
}

#[derive(Debug)]
struct ComposedFrame {
  epoch: u64,
  dt: f32,
  merged_writes: WriteBatch,
  conflicts: Vec<ConflictLog>,
  events: Vec<JsonValue>,
  timings_ms: BTreeMap<String, f32>,
}

impl ComposedFrame {
  fn to_json_value(self) -> Result<JsonValue, String> {
    serde_json::to_value(json!({
      "epoch": self.epoch,
      "dt": self.dt,
      "merged_writes": self.merged_writes,
      "conflicts": self.conflicts,
      "events": self.events,
      "timings_ms": self.timings_ms,
    }))
    .map_err(|error| format!("failed to serialize frame: {error}"))
  }
}

#[cfg(target_arch = "wasm32")]
#[derive(Deserialize)]
struct DomainModuleResponse {
  ok: bool,
  #[serde(default)]
  value: JsonValue,
  #[serde(default)]
  error: Option<String>,
}

#[cfg(target_arch = "wasm32")]
fn unwrap_domain_value(response: String, op: &str) -> Result<JsonValue, String> {
  let parsed: DomainModuleResponse = serde_json::from_str(&response)
    .map_err(|error| format!("{op} returned invalid response json: {error}"))?;
  if parsed.ok {
    Ok(parsed.value)
  } else {
    Err(format!(
      "{op} failed: {}",
      parsed.error.unwrap_or_else(|| "unknown error".to_string())
    ))
  }
}

#[cfg(target_arch = "wasm32")]
fn writebatch_from_domain_value(value: &JsonValue, op: &str) -> Result<WriteBatch, String> {
  let writes = value
    .get("writes")
    .cloned()
    .unwrap_or_else(|| JsonValue::Array(Vec::new()));
  serde_json::from_value(writes).map_err(|error| format!("{op} returned invalid writes: {error}"))
}

fn normalize_graph_module(spec: JsonValue) -> Result<JsonValue, String> {
  #[cfg(target_arch = "wasm32")]
  {
    return unwrap_domain_value(
      arora_generated::vizij_node_graph::normalize_graph(json!({ "spec": spec }).to_string()),
      "vizij-node-graph.normalize_graph",
    );
  }

  #[cfg(not(target_arch = "wasm32"))]
  {
    NodeGraphModuleFacade::normalize_graph_value(spec)
  }
}

fn load_graph_module(
  id: &str,
  spec: JsonValue,
  subs: Subscriptions,
) -> Result<GraphModule, String> {
  #[cfg(target_arch = "wasm32")]
  {
    unwrap_domain_value(
      arora_generated::vizij_node_graph::load_graph(
        json!({ "graphId": id, "spec": spec }).to_string(),
      ),
      "vizij-node-graph.load_graph",
    )?;
    Ok(GraphModule { subs })
  }

  #[cfg(not(target_arch = "wasm32"))]
  {
    let _ = id;
    let mut graph = NodeGraphModuleFacade::new();
    graph.load_graph_value(spec)?;
    Ok(GraphModule {
      facade: graph,
      subs,
    })
  }
}

fn replace_graph_module(id: &str, graph: &mut GraphModule, spec: JsonValue) -> Result<(), String> {
  #[cfg(target_arch = "wasm32")]
  {
    let _ = graph;
    unwrap_domain_value(
      arora_generated::vizij_node_graph::replace_graph(
        json!({ "graphId": id, "spec": spec }).to_string(),
      ),
      "vizij-node-graph.replace_graph",
    )?;
    Ok(())
  }

  #[cfg(not(target_arch = "wasm32"))]
  {
    let _ = id;
    graph.facade.replace_graph_value(spec)
  }
}

fn remove_graph_module(id: &str) -> Result<(), String> {
  #[cfg(target_arch = "wasm32")]
  {
    unwrap_domain_value(
      arora_generated::vizij_node_graph::remove_graph(json!({ "graphId": id }).to_string()),
      "vizij-node-graph.remove_graph",
    )?;
  }
  #[cfg(not(target_arch = "wasm32"))]
  {
    let _ = id;
  }
  Ok(())
}

fn stage_graph_input(
  id: &str,
  graph: &mut GraphModule,
  path: String,
  value: JsonValue,
  shape: Option<JsonValue>,
) -> Result<(), String> {
  #[cfg(target_arch = "wasm32")]
  {
    let _ = graph;
    unwrap_domain_value(
      arora_generated::vizij_node_graph::stage_input(
        json!({
          "graphId": id,
          "path": path,
          "value": value,
          "shape": shape
        })
        .to_string(),
      ),
      "vizij-node-graph.stage_input",
    )?;
    Ok(())
  }

  #[cfg(not(target_arch = "wasm32"))]
  {
    let _ = id;
    graph.facade.stage_input_value(path, value, shape)
  }
}

fn evaluate_graph_module(id: &str, graph: &mut GraphModule, dt: f32) -> Result<WriteBatch, String> {
  #[cfg(target_arch = "wasm32")]
  {
    let _ = graph;
    let value = unwrap_domain_value(
      arora_generated::vizij_node_graph::evaluate(json!({ "graphId": id, "dt": dt }).to_string()),
      "vizij-node-graph.evaluate",
    )?;
    writebatch_from_domain_value(&value, "vizij-node-graph.evaluate")
  }

  #[cfg(not(target_arch = "wasm32"))]
  {
    let _ = id;
    graph.facade.evaluate_writebatch(dt)
  }
}

fn configure_animation_module(id: &str, setup: JsonValue) -> Result<AnimationModuleHandle, String> {
  #[cfg(target_arch = "wasm32")]
  {
    unwrap_domain_value(
      arora_generated::vizij_animation::configure_controller(
        json!({ "controllerId": id, "setup": setup }).to_string(),
      ),
      "vizij-animation.configure_controller",
    )?;
    Ok(AnimationModuleHandle {})
  }

  #[cfg(not(target_arch = "wasm32"))]
  {
    let _ = id;
    let mut animation = AnimationModuleFacade::new();
    animation.configure_from_setup_value(setup)?;
    Ok(AnimationModuleHandle { facade: animation })
  }
}

fn remove_animation_module(id: &str) -> Result<(), String> {
  #[cfg(target_arch = "wasm32")]
  {
    unwrap_domain_value(
      arora_generated::vizij_animation::remove_controller(
        json!({ "controllerId": id }).to_string(),
      ),
      "vizij-animation.remove_controller",
    )?;
  }
  #[cfg(not(target_arch = "wasm32"))]
  {
    let _ = id;
  }
  Ok(())
}

fn update_animation_module(
  id: &str,
  animation: &mut AnimationModuleHandle,
  dt: f32,
  inputs: Inputs,
) -> Result<(WriteBatch, Vec<JsonValue>), String> {
  #[cfg(target_arch = "wasm32")]
  {
    let _ = animation;
    let value = unwrap_domain_value(
      arora_generated::vizij_animation::update_nodes_writes(
        json!({ "controllerId": id, "dt": dt, "inputs": inputs }).to_string(),
      ),
      "vizij-animation.update_nodes_writes",
    )?;
    let batch = writebatch_from_domain_value(&value, "vizij-animation.update_nodes_writes")?;
    let events = value
      .get("events")
      .and_then(JsonValue::as_array)
      .cloned()
      .unwrap_or_default();
    Ok((batch, events))
  }

  #[cfg(not(target_arch = "wasm32"))]
  {
    let _ = id;
    animation.facade.update_outputs(dt, Some(inputs))
  }
}

fn run_schedule(runtime: &mut ComposedRuntime, dt: f32) -> Result<ComposedFrame, String> {
  let mut merged_writes = WriteBatch::new();
  let mut conflicts = Vec::new();
  let mut events = Vec::new();
  let mut timings_ms = BTreeMap::new();

  match runtime.schedule {
    Schedule::SinglePass | Schedule::RateDecoupled => {
      run_animation_pass(runtime, dt, &mut merged_writes, &mut conflicts, &mut events)?;
      if !runtime.anims.is_empty() {
        timings_ms.insert("animations_ms".to_string(), dt * 1000.0);
      }
      run_graph_pass(runtime, dt, &mut merged_writes, &mut conflicts)?;
      if !runtime.graphs.is_empty() {
        timings_ms.insert("graphs_ms".to_string(), dt * 1000.0);
      }
    }
    Schedule::TwoPass => {
      run_graph_pass(runtime, dt, &mut merged_writes, &mut conflicts)?;
      if !runtime.graphs.is_empty() {
        timings_ms.insert("graphs_pass1_ms".to_string(), dt * 1000.0);
      }
      run_animation_pass(runtime, dt, &mut merged_writes, &mut conflicts, &mut events)?;
      if !runtime.anims.is_empty() {
        timings_ms.insert("animations_ms".to_string(), dt * 1000.0);
      }
      run_graph_pass(runtime, dt, &mut merged_writes, &mut conflicts)?;
      if !runtime.graphs.is_empty() {
        timings_ms.insert("graphs_pass2_ms".to_string(), dt * 1000.0);
      }
    }
  }

  timings_ms.insert("total_ms".to_string(), dt * 1000.0);

  Ok(ComposedFrame {
    epoch: runtime.epoch,
    dt,
    merged_writes,
    conflicts,
    events,
    timings_ms,
  })
}

fn run_animation_pass(
  runtime: &mut ComposedRuntime,
  dt: f32,
  merged_writes: &mut WriteBatch,
  conflicts_out: &mut Vec<ConflictLog>,
  events_out: &mut Vec<JsonValue>,
) -> Result<(), String> {
  let ids: Vec<String> = runtime.anims.keys().cloned().collect();
  for id in ids {
    let inputs =
      AnimationController::inputs_from_blackboard_for_controller(&runtime.blackboard, &id);
    let animation = runtime
      .anims
      .get_mut(&id)
      .ok_or_else(|| format!("animation '{id}' disappeared during pass"))?;
    let (batch, events) = update_animation_module(&id, animation, dt, inputs)?;
    merged_writes.append(batch.clone());
    conflicts_out.extend(runtime.blackboard.apply_writebatch(
      batch,
      runtime.epoch,
      format!("anim:{id}"),
    ));
    events_out.extend(events);
  }
  Ok(())
}

fn run_graph_pass(
  runtime: &mut ComposedRuntime,
  dt: f32,
  merged_writes: &mut WriteBatch,
  conflicts_out: &mut Vec<ConflictLog>,
) -> Result<(), String> {
  let ids: Vec<String> = runtime.graphs.keys().cloned().collect();
  for id in ids {
    let staged = collect_graph_staged_inputs(runtime, &id)?;
    let (batch, subs) = {
      let graph = runtime
        .graphs
        .get_mut(&id)
        .ok_or_else(|| format!("graph '{id}' disappeared during pass"))?;
      for (path, value, shape) in staged {
        stage_graph_input(&id, graph, path, value, shape)?;
      }
      let batch = evaluate_graph_module(&id, graph, dt)?;
      let subs = graph.subs.clone();
      (batch, subs)
    };

    let publish_batch = filter_published_writes(&batch, &subs.outputs);
    merged_writes.append(publish_batch.clone());

    let apply_batch = if subs.mirror_writes {
      batch
    } else {
      publish_batch
    };
    conflicts_out.extend(runtime.blackboard.apply_writebatch(
      apply_batch,
      runtime.epoch,
      format!("graph:{id}"),
    ));
  }
  Ok(())
}

fn collect_graph_staged_inputs(
  runtime: &ComposedRuntime,
  id: &str,
) -> Result<Vec<(String, JsonValue, Option<JsonValue>)>, String> {
  let graph = runtime
    .graphs
    .get(id)
    .ok_or_else(|| format!("graph '{id}' disappeared during input staging"))?;
  let mut staged = Vec::with_capacity(graph.subs.inputs.len());
  for path in &graph.subs.inputs {
    if let Some(entry) = runtime.blackboard.get_tp(path) {
      let value = serde_json::to_value(&entry.value)
        .map_err(|error| format!("failed to stage graph input '{path}': {error}"))?;
      let shape = entry
        .shape
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| format!("failed to stage graph input shape '{path}': {error}"))?;
      staged.push((path.to_string(), value, shape));
    }
  }
  Ok(staged)
}

fn filter_published_writes(batch: &WriteBatch, outputs: &[TypedPath]) -> WriteBatch {
  if outputs.is_empty() {
    return batch.clone();
  }
  let mut filtered = WriteBatch::new();
  for op in batch.iter() {
    if outputs.iter().any(|path| path == &op.path) {
      filtered.push(op.clone());
    }
  }
  filtered
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
  #[serde(default)]
  subs: Option<GraphSubscriptionsArgs>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphSubscriptionsArgs {
  #[serde(default)]
  inputs: Vec<String>,
  #[serde(default)]
  outputs: Vec<String>,
  #[serde(default, alias = "mirror_writes")]
  mirror_writes: Option<bool>,
}

#[derive(Deserialize)]
struct MergedGraphRegistrationArgs {
  #[serde(default)]
  id: Option<String>,
  graphs: Vec<GraphRegistrationArgs>,
  #[serde(default)]
  strategy: Option<MergeStrategyArgs>,
}

#[derive(Default, Deserialize)]
struct MergeStrategyArgs {
  #[serde(default)]
  outputs: Option<String>,
  #[serde(default)]
  intermediate: Option<String>,
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StepDeltaArgs {
  dt: f32,
  #[serde(default, alias = "since_version")]
  since_version: Option<u64>,
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

fn build_graph_controller_config(
  mut graph: GraphRegistrationArgs,
  fallback_id: String,
) -> Result<GraphControllerConfig, String> {
  api_json::normalize_graph_spec_value(&mut graph.spec)
    .map_err(|error| format!("normalize graph spec error: {error}"))?;
  let spec = serde_json::from_value::<GraphSpec>(graph.spec)
    .map_err(|error| format!("graph spec deserialize error: {error}"))?
    .with_cache();
  Ok(GraphControllerConfig {
    id: graph.id.unwrap_or(fallback_id),
    spec,
    subs: map_graph_subscriptions(graph.subs)?,
  })
}

fn map_graph_subscriptions(cfg: Option<GraphSubscriptionsArgs>) -> Result<Subscriptions, String> {
  let mut subs = Subscriptions::default();
  if let Some(conf) = cfg {
    subs.inputs = conf
      .inputs
      .into_iter()
      .map(|input| {
        TypedPath::parse(&input)
          .map_err(|error| format!("invalid input subscription '{input}': {error}"))
      })
      .collect::<Result<Vec<_>, _>>()?;
    subs.outputs = conf
      .outputs
      .into_iter()
      .map(|output| {
        TypedPath::parse(&output)
          .map_err(|error| format!("invalid output subscription '{output}': {error}"))
      })
      .collect::<Result<Vec<_>, _>>()?;
    if let Some(mirror_writes) = conf.mirror_writes {
      subs.mirror_writes = mirror_writes;
    }
  }
  Ok(subs)
}

fn map_merge_options(cfg: Option<MergeStrategyArgs>) -> Result<GraphMergeOptions, String> {
  let mut options = GraphMergeOptions::default();
  if let Some(strategy) = cfg {
    if let Some(outputs) = strategy.outputs {
      options.output_conflicts = parse_conflict_strategy(&outputs)?;
    }
    if let Some(intermediate) = strategy.intermediate {
      options.intermediate_conflicts = parse_conflict_strategy(&intermediate)?;
    }
  }
  Ok(options)
}

fn parse_conflict_strategy(value: &str) -> Result<OutputConflictStrategy, String> {
  match value.trim().to_ascii_lowercase().as_str() {
    "error" => Ok(OutputConflictStrategy::Error),
    "namespace" => Ok(OutputConflictStrategy::Namespace),
    "blend" | "blend_equal" | "blend_equal_weights" => {
      Ok(OutputConflictStrategy::BlendEqualWeights)
    }
    "add" | "sum" | "blend_sum" | "blend-sum" | "additive" => {
      Ok(OutputConflictStrategy::Add)
    }
    "default_blend"
    | "default-blend"
    | "blend-default"
    | "blend_weights"
    | "blend-weights"
    | "weights" => Ok(OutputConflictStrategy::DefaultBlend),
    other => Err(format!(
      "unknown merge conflict strategy '{other}'; expected 'error', 'namespace', 'blend', 'add', or 'default-blend'"
    )),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use vizij_orchestrator::VizijModuleFacade;

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

  fn call_compat(facade: &mut VizijModuleFacade, name: &str, args: JsonValue) -> JsonValue {
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

  fn create_pair(schedule: &str) -> (ComposedOrchestratorFacade, VizijModuleFacade) {
    let mut composed = ComposedOrchestratorFacade::new();
    let mut compat = VizijModuleFacade::new();
    call(
      &mut composed,
      "runtime.create",
      json!({ "schedule": schedule }),
    );
    call_compat(
      &mut compat,
      "runtime.create",
      json!({ "schedule": schedule }),
    );
    (composed, compat)
  }

  fn write_paths(frame: &JsonValue) -> Vec<String> {
    frame["merged_writes"]
      .as_array()
      .expect("writes array")
      .iter()
      .map(|write| write["path"].as_str().expect("write path").to_string())
      .collect()
  }

  fn write_value_float(frame: &JsonValue, path: &str) -> f64 {
    frame["merged_writes"]
      .as_array()
      .expect("writes array")
      .iter()
      .find(|write| write["path"] == path)
      .unwrap_or_else(|| panic!("missing write for {path}: {frame}"))["value"]["data"]
      .as_f64()
      .expect("float write value")
  }

  fn fixture_animation() -> JsonValue {
    fixture_animation_for_path("face/smile.amount")
  }

  fn fixture_animation_for_path(output_path: &str) -> JsonValue {
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
          "animatableId": output_path,
          "points": [
            { "id": "smile-0", "stamp": 0, "value": 0, "transitions": { "out": "linear" } },
            { "id": "smile-1", "stamp": 1000, "value": 1, "transitions": { "in": "linear" } }
          ]
        }
      ]
    })
  }

  fn graph_constant_output(path: &str, value: f32) -> JsonValue {
    json!({
      "nodes": [
        {
          "id": "source",
          "type": "constant",
          "params": { "value": { "type": "float", "data": value } }
        },
        {
          "id": "out",
          "type": "output",
          "params": { "path": path }
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

  fn graph_input_to_output(input_path: &str, output_path: &str) -> JsonValue {
    json!({
      "nodes": [
        {
          "id": "in",
          "type": "input",
          "params": {
            "path": input_path,
            "value": { "type": "float", "data": 0.0 }
          }
        },
        {
          "id": "out",
          "type": "output",
          "params": { "path": output_path }
        }
      ],
      "edges": [
        {
          "from": { "node_id": "in", "output": "out" },
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

  fn fixture_graph() -> JsonValue {
    graph_constant_output("face/graph.value", 3.0)
  }

  #[test]
  fn module_manifest_declares_domain_imports() {
    let manifest = include_str!("../module.yaml");
    assert!(
      manifest.contains("module: aa32e080-b002-428c-9994-6143aab3bf08"),
      "composed orchestrator must import vizij-animation by module id"
    );
    assert!(
      manifest.contains("module: 098bd478-8375-4f3a-b649-d64cb1284944"),
      "composed orchestrator must import vizij-node-graph by module id"
    );
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

  #[test]
  fn composed_matches_compat_for_subscribed_graph_inputs() {
    let (mut composed, mut compat) = create_pair("SinglePass");
    let graph = json!({
      "id": "graph:input",
      "spec": graph_input_to_output("control/smile.amount", "face/smile.amount"),
      "subs": {
        "inputs": ["control/smile.amount"],
        "outputs": ["face/smile.amount"]
      }
    });

    call(&mut composed, "graph.register", graph.clone());
    call_compat(&mut compat, "graph.register", graph);
    let input = json!({
      "path": "control/smile.amount",
      "value": { "type": "float", "data": 0.75 }
    });
    call(&mut composed, "input.set", input.clone());
    call_compat(&mut compat, "input.set", input);

    let composed_frame = call(
      &mut composed,
      "orchestrator.step",
      json!({ "dt": 1.0 / 60.0 }),
    );
    let compat_frame = call_compat(
      &mut compat,
      "orchestrator.step",
      json!({ "dt": 1.0 / 60.0 }),
    );
    assert_eq!(
      composed_frame["merged_writes"],
      compat_frame["merged_writes"]
    );
    assert_eq!(write_paths(&composed_frame), vec!["face/smile.amount"]);
  }

  #[test]
  fn composed_matches_compat_for_graph_merge() {
    let (mut composed, mut compat) = create_pair("SinglePass");
    let merged = json!({
      "id": "graph:merged",
      "graphs": [
        {
          "id": "driver",
          "spec": graph_constant_output("control/driver.value", 0.5),
          "subs": {
            "outputs": ["control/driver.value"],
            "mirrorWrites": true
          }
        },
        {
          "id": "consumer",
          "spec": graph_input_to_output("control/driver.value", "face/merged.value"),
          "subs": {
            "inputs": ["control/driver.value"],
            "outputs": ["face/merged.value"]
          }
        }
      ],
      "strategy": {
        "outputs": "add",
        "intermediate": "add"
      }
    });

    call(&mut composed, "graph.merge", merged.clone());
    call_compat(&mut compat, "graph.merge", merged);
    let composed_frame = call(
      &mut composed,
      "orchestrator.step",
      json!({ "dt": 1.0 / 60.0 }),
    );
    let compat_frame = call_compat(
      &mut compat,
      "orchestrator.step",
      json!({ "dt": 1.0 / 60.0 }),
    );
    assert_eq!(
      composed_frame["merged_writes"],
      compat_frame["merged_writes"]
    );
    assert_eq!(
      write_paths(&composed_frame),
      vec!["control/driver.value", "face/merged.value"]
    );
  }

  #[test]
  fn composed_matches_compat_for_insertion_order_conflicts() {
    let (mut composed, mut compat) = create_pair("SinglePass");
    let first = json!({
      "id": "graph:z",
      "spec": graph_constant_output("face/order.value", 0.25)
    });
    let second = json!({
      "id": "graph:a",
      "spec": graph_constant_output("face/order.value", 0.75)
    });

    call(&mut composed, "graph.register", first.clone());
    call_compat(&mut compat, "graph.register", first);
    call(&mut composed, "graph.register", second.clone());
    call_compat(&mut compat, "graph.register", second);

    let composed_frame = call(
      &mut composed,
      "orchestrator.step",
      json!({ "dt": 1.0 / 60.0 }),
    );
    let compat_frame = call_compat(
      &mut compat,
      "orchestrator.step",
      json!({ "dt": 1.0 / 60.0 }),
    );
    assert_eq!(
      composed_frame["merged_writes"],
      compat_frame["merged_writes"]
    );
    assert_eq!(composed_frame["conflicts"], compat_frame["conflicts"]);
    let values: Vec<f64> = composed_frame["merged_writes"]
      .as_array()
      .expect("writes array")
      .iter()
      .map(|write| write["value"]["data"].as_f64().expect("float value"))
      .collect();
    assert_eq!(values, vec![0.25, 0.75]);
  }

  #[test]
  fn composed_matches_compat_for_graph_replace() {
    let (mut composed, mut compat) = create_pair("SinglePass");
    let initial = json!({
      "id": "graph:replace",
      "spec": graph_constant_output("face/replaced.value", 0.25)
    });
    let replacement = json!({
      "id": "graph:replace",
      "spec": graph_constant_output("face/replaced.value", 0.9)
    });

    call(&mut composed, "graph.register", initial.clone());
    call_compat(&mut compat, "graph.register", initial);
    call(&mut composed, "graph.replace", replacement.clone());
    call_compat(&mut compat, "graph.replace", replacement);

    let composed_frame = call(
      &mut composed,
      "orchestrator.step",
      json!({ "dt": 1.0 / 60.0 }),
    );
    let compat_frame = call_compat(
      &mut compat,
      "orchestrator.step",
      json!({ "dt": 1.0 / 60.0 }),
    );
    assert_eq!(
      composed_frame["merged_writes"],
      compat_frame["merged_writes"]
    );
    let value = composed_frame["merged_writes"][0]["value"]["data"]
      .as_f64()
      .expect("float write value");
    assert!((value - 0.9).abs() < 0.0001, "{value}");
  }

  #[test]
  fn composed_matches_compat_for_graph_replace_preserving_runtime_time() {
    let (mut composed, mut compat) = create_pair("SinglePass");
    let initial = json!({
      "id": "graph:time",
      "spec": graph_time_output("face/time.value")
    });
    let replacement = json!({
      "id": "graph:time",
      "spec": graph_time_output("face/time.value")
    });

    call(&mut composed, "graph.register", initial.clone());
    call_compat(&mut compat, "graph.register", initial);
    call(&mut composed, "orchestrator.step", json!({ "dt": 0.25 }));
    call_compat(&mut compat, "orchestrator.step", json!({ "dt": 0.25 }));
    call(&mut composed, "graph.replace", replacement.clone());
    call_compat(&mut compat, "graph.replace", replacement);

    let composed_frame = call(&mut composed, "orchestrator.step", json!({ "dt": 0.25 }));
    let compat_frame = call_compat(&mut compat, "orchestrator.step", json!({ "dt": 0.25 }));
    assert_eq!(
      composed_frame["merged_writes"],
      compat_frame["merged_writes"]
    );
    assert!((write_value_float(&composed_frame, "face/time.value") - 0.5).abs() < 0.0001);
  }

  #[test]
  fn composed_matches_compat_for_two_pass_animation_to_graph() {
    let (mut composed, mut compat) = create_pair("TwoPass");
    let animation = json!({
      "id": "anim:two-pass",
      "setup": {
        "animation": fixture_animation(),
        "instance": { "timescale": 1.0, "active": true }
      }
    });
    let graph = json!({
      "id": "graph:two-pass",
      "spec": graph_input_to_output("face/smile.amount", "face/two_pass.value"),
      "subs": {
        "inputs": ["face/smile.amount"],
        "outputs": ["face/two_pass.value"]
      }
    });
    call(&mut composed, "animation.register", animation.clone());
    call_compat(&mut compat, "animation.register", animation);
    call(&mut composed, "graph.register", graph.clone());
    call_compat(&mut compat, "graph.register", graph);

    let composed_frame = call(&mut composed, "orchestrator.step", json!({ "dt": 0.5 }));
    let compat_frame = call_compat(&mut compat, "orchestrator.step", json!({ "dt": 0.5 }));
    assert_eq!(
      composed_frame["merged_writes"],
      compat_frame["merged_writes"]
    );
    let paths = write_paths(&composed_frame);
    assert!(
      paths.contains(&"face/smile.amount".to_string()),
      "{paths:?}"
    );
    assert!(
      paths.contains(&"face/two_pass.value".to_string()),
      "{paths:?}"
    );
  }

  #[test]
  fn composed_matches_compat_for_studio_animation_setup_aliases() {
    let (mut composed, mut compat) = create_pair("SinglePass");
    let animation = json!({
      "id": "anim:studio-aliases",
      "setup": {
        "animation": fixture_animation(),
        "player": { "name": "studio-player", "loopMode": "once" },
        "instance": {
          "timeScale": 2.0,
          "offset": 250.0,
          "active": true
        }
      }
    });

    call(&mut composed, "animation.register", animation.clone());
    call_compat(&mut compat, "animation.register", animation);

    let composed_frame = call(&mut composed, "orchestrator.step", json!({ "dt": 0.375 }));
    let compat_frame = call_compat(&mut compat, "orchestrator.step", json!({ "dt": 0.375 }));
    assert_eq!(
      composed_frame["merged_writes"],
      compat_frame["merged_writes"]
    );
    assert!((write_value_float(&composed_frame, "face/smile.amount") - 0.25).abs() < 0.0001);
  }

  #[test]
  fn composed_matches_compat_for_legacy_animation_setup_aliases() {
    let (mut composed, mut compat) = create_pair("SinglePass");
    let animation = json!({
      "id": "anim:legacy-aliases",
      "setup": {
        "animation": fixture_animation(),
        "instance": {
          "timescale": 2.0,
          "startOffset": 0.25
        }
      }
    });

    call(&mut composed, "animation.register", animation.clone());
    call_compat(&mut compat, "animation.register", animation);

    let composed_frame = call(&mut composed, "orchestrator.step", json!({ "dt": 0.375 }));
    let compat_frame = call_compat(&mut compat, "orchestrator.step", json!({ "dt": 0.375 }));
    assert_eq!(
      composed_frame["merged_writes"],
      compat_frame["merged_writes"]
    );
    assert!((write_value_float(&composed_frame, "face/smile.amount") - 0.25).abs() < 0.0001);
  }

  #[test]
  fn composed_matches_compat_for_inactive_studio_animation_setup() {
    let (mut composed, mut compat) = create_pair("SinglePass");
    let animation = json!({
      "id": "anim:inactive",
      "setup": {
        "animation": fixture_animation(),
        "instance": { "active": false }
      }
    });

    call(&mut composed, "animation.register", animation.clone());
    call_compat(&mut compat, "animation.register", animation);

    let composed_frame = call(&mut composed, "orchestrator.step", json!({ "dt": 0.5 }));
    let compat_frame = call_compat(&mut compat, "orchestrator.step", json!({ "dt": 0.5 }));
    assert_eq!(
      composed_frame["merged_writes"],
      compat_frame["merged_writes"]
    );
    assert!(write_paths(&composed_frame).is_empty());
  }

  #[test]
  fn composed_matches_compat_for_scoped_animation_commands() {
    let (mut composed, mut compat) = create_pair("SinglePass");
    for (id, path) in [
      ("default/animation/blink", "face/blink.amount"),
      ("default/animation/smile", "face/smile.amount"),
    ] {
      let animation = json!({
        "id": id,
        "setup": {
          "animation": fixture_animation_for_path(path),
          "player": { "speed": 0.0 }
        }
      });
      call(&mut composed, "animation.register", animation.clone());
      call_compat(&mut compat, "animation.register", animation);
    }

    let command = json!({
      "path": "anim/controller/default/animation/blink/player/0/cmd/seek",
      "value": { "type": "float", "data": 0.75 }
    });
    call(&mut composed, "input.set", command.clone());
    call_compat(&mut compat, "input.set", command);

    let composed_frame = call(&mut composed, "orchestrator.step", json!({ "dt": 0.0 }));
    let compat_frame = call_compat(&mut compat, "orchestrator.step", json!({ "dt": 0.0 }));
    assert_eq!(
      composed_frame["merged_writes"],
      compat_frame["merged_writes"]
    );
    assert!((write_value_float(&composed_frame, "face/blink.amount") - 0.75).abs() < 0.0001);
    assert!(
      write_value_float(&composed_frame, "face/smile.amount").abs() < 0.0001,
      "scoped command should not move smile: {composed_frame}"
    );
  }

  #[test]
  fn composed_keeps_legacy_animation_commands_broadcast_compatible() {
    let (mut composed, mut compat) = create_pair("SinglePass");
    for (id, path) in [
      ("default/animation/blink", "face/blink.amount"),
      ("default/animation/smile", "face/smile.amount"),
    ] {
      let animation = json!({
        "id": id,
        "setup": {
          "animation": fixture_animation_for_path(path),
          "player": { "speed": 0.0 }
        }
      });
      call(&mut composed, "animation.register", animation.clone());
      call_compat(&mut compat, "animation.register", animation);
    }

    let command = json!({
      "path": "anim/player/0/cmd/seek",
      "value": { "type": "float", "data": 0.5 }
    });
    call(&mut composed, "input.set", command.clone());
    call_compat(&mut compat, "input.set", command);

    let composed_frame = call(&mut composed, "orchestrator.step", json!({ "dt": 0.0 }));
    let compat_frame = call_compat(&mut compat, "orchestrator.step", json!({ "dt": 0.0 }));
    assert_eq!(
      composed_frame["merged_writes"],
      compat_frame["merged_writes"]
    );
    assert!((write_value_float(&composed_frame, "face/blink.amount") - 0.5).abs() < 0.0001);
    assert!((write_value_float(&composed_frame, "face/smile.amount") - 0.5).abs() < 0.0001);
  }

  #[test]
  fn composed_matches_compat_for_conflicts_and_delta_frames() {
    let (mut composed, mut compat) = create_pair("SinglePass");
    let first = json!({
      "id": "graph:a",
      "spec": graph_constant_output("face/conflict.value", 0.25)
    });
    let second = json!({
      "id": "graph:b",
      "spec": graph_constant_output("face/conflict.value", 0.75)
    });
    call(&mut composed, "graph.register", first.clone());
    call_compat(&mut compat, "graph.register", first);
    call(&mut composed, "graph.register", second.clone());
    call_compat(&mut compat, "graph.register", second);

    let composed_delta = call(
      &mut composed,
      "orchestrator.stepDelta",
      json!({ "dt": 1.0 / 60.0 }),
    );
    let compat_delta = call_compat(
      &mut compat,
      "orchestrator.stepDelta",
      json!({ "dt": 1.0 / 60.0 }),
    );
    assert_eq!(
      composed_delta["merged_writes"],
      compat_delta["merged_writes"]
    );
    assert_eq!(composed_delta["conflicts"], compat_delta["conflicts"]);
    assert_eq!(composed_delta["version"], 1);

    let composed_suppressed = call(
      &mut composed,
      "orchestrator.stepDelta",
      json!({ "dt": 1.0 / 60.0, "sinceVersion": 1 }),
    );
    let compat_suppressed = call_compat(
      &mut compat,
      "orchestrator.stepDelta",
      json!({ "dt": 1.0 / 60.0, "sinceVersion": 1 }),
    );
    assert_eq!(
      composed_suppressed["merged_writes"],
      compat_suppressed["merged_writes"]
    );
    assert_eq!(
      composed_suppressed["merged_writes"]
        .as_array()
        .expect("writes array")
        .len(),
      0
    );
  }

  #[test]
  fn composed_matches_compat_for_partial_delta_frames() {
    let (mut composed, mut compat) = create_pair("SinglePass");
    let static_graph = json!({
      "id": "graph:static",
      "spec": graph_constant_output("face/static.value", 0.25)
    });
    let time_graph = json!({
      "id": "graph:time",
      "spec": graph_time_output("face/time.value")
    });
    call(&mut composed, "graph.register", static_graph.clone());
    call_compat(&mut compat, "graph.register", static_graph);
    call(&mut composed, "graph.register", time_graph.clone());
    call_compat(&mut compat, "graph.register", time_graph);

    call(
      &mut composed,
      "orchestrator.stepDelta",
      json!({ "dt": 0.25 }),
    );
    call_compat(&mut compat, "orchestrator.stepDelta", json!({ "dt": 0.25 }));

    let composed_delta = call(
      &mut composed,
      "orchestrator.stepDelta",
      json!({ "dt": 0.25, "sinceVersion": 1 }),
    );
    let compat_delta = call_compat(
      &mut compat,
      "orchestrator.stepDelta",
      json!({ "dt": 0.25, "sinceVersion": 1 }),
    );
    assert_eq!(
      composed_delta["merged_writes"],
      compat_delta["merged_writes"]
    );
    assert_eq!(write_paths(&composed_delta), vec!["face/time.value"]);
    assert!((write_value_float(&composed_delta, "face/time.value") - 0.5).abs() < 0.0001);
  }
}
