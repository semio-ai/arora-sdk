//! Browser-facing wrapper around the Arora engine.
//!
//! Provides a [`Engine`] type that JavaScript can construct, load
//! modules into (header JSON + executable bytes), and call functions
//! on. All module hosting is done via the browser's native
//! `WebAssembly` runtime — see `arora::executor::browser`.
//!
//! This crate only carries non-trivial content when built for
//! `wasm32-*` targets. On the host it is an empty shim so it can sit
//! in the workspace and participate in `cargo build --workspace`
//! without pulling wasm-only deps into a host link.

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::collections::HashMap;
use std::pin::Pin;
use std::rc::Rc;

use arora::{
  call::{CallBridge, Callable, CallableId},
  engine::EngineBuilder,
  executor::browser::BrowserExecutor,
  load::load_module_from_parts,
  schema::module::low::Header,
};
use arora_types::{
  call::Call,
  value::{Enumeration, StructureField, StructureWithoutId, Value},
};
use uuid::Uuid;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn _start() {
  console_error_panic_hook::set_once();
}

/// JS-callable handle to a configured Arora engine.
#[wasm_bindgen]
pub struct Engine {
  inner: std::pin::Pin<Box<arora::engine::Engine>>,
  function_module: HashMap<Uuid, Uuid>,
}

#[wasm_bindgen]
impl Engine {
  #[wasm_bindgen(constructor)]
  pub fn new() -> Engine {
    let inner = EngineBuilder::new().add_executor(BrowserExecutor::new()).build();
    Engine {
      inner,
      function_module: HashMap::new(),
    }
  }

  /// Load a module given its header (as JSON) and executable bytes.
  /// Returns the module's UUID as a string.
  #[wasm_bindgen(js_name = loadModule)]
  pub fn load_module(&mut self, header_json: &str, executable: &[u8]) -> Result<String, JsValue> {
    let header: Header =
      serde_json::from_str(header_json).map_err(|e| JsValue::from_str(&format!("invalid header json: {e}")))?;
    let loaded = load_module_from_parts(
      &mut *self.inner,
      header,
      executable.to_vec().into_boxed_slice(),
    )
    .map_err(|e| JsValue::from_str(&format!("load_module failed: {e}")))?;
    for fn_id in &loaded.function_ids {
      self.function_module.insert(*fn_id, loaded.id);
    }
    Ok(loaded.id.to_string())
  }

  /// Call a function. `call_json` is a JSON document matching
  /// `arora::call::Call`. Returns the result as a JSON string.
  #[wasm_bindgen]
  pub fn call(&mut self, call_json: &str) -> Result<String, JsValue> {
    let call: Call = serde_json::from_str(call_json)
      .map_err(|e| JsValue::from_str(&format!("invalid call json: {e}")))?;
    let module_id = if let Some(m) = call.module_id {
      m
    } else {
      *self
        .function_module
        .get(&call.id)
        .ok_or_else(|| JsValue::from_str("no module known for function"))?
    };
    let result = self
      .inner
      .arora_call(&module_id, call)
      .map_err(|e| JsValue::from_str(&format!("call failed: {e}")))?;
    serde_json::to_string(&result).map_err(|e| JsValue::from_str(&format!("serialize failed: {e}")))
  }
}

impl Default for Engine {
  fn default() -> Self {
    Self::new()
  }
}

// =============================================================================
// Behavior-tree runner
//
// A self-contained BT runtime built directly on arora's engine primitives —
// no dependency on the arora-behavior-tree crate.
// =============================================================================

// UUID constants from arora-behavior-tree-types generated code.
// TickId struct id: 6f49e650-84ca-4899-a9bd-1f3bf17fab51
const TICK_ID_STRUCT_BYTES: [u8; 16] = [
  0x6f, 0x49, 0xe6, 0x50, 0x84, 0xca, 0x48, 0x99, 0xa9, 0xbd, 0x1f, 0x3b, 0xf1, 0x7f, 0xab, 0x51,
];
// TickId::callable_id field: 237992d2-17d1-459f-bca1-7185fa6a69d7
const TICK_ID_CALLABLE_FIELD_BYTES: [u8; 16] = [
  0x23, 0x79, 0x92, 0xd2, 0x17, 0xd1, 0x45, 0x9f, 0xbc, 0xa1, 0x71, 0x85, 0xfa, 0x6a, 0x69, 0xd7,
];
// Status::Success variant: 766e9e9a-446d-4e46-83e6-14b7ca101169
const STATUS_SUCCESS_BYTES: [u8; 16] = [
  0x76, 0x6e, 0x9e, 0x9a, 0x44, 0x6d, 0x4e, 0x46, 0x83, 0xe6, 0x14, 0xb7, 0xca, 0x10, 0x11, 0x69,
];
// Status::Failure variant: 2468f46c-bb60-425c-9a4d-9ad326ccc7e2
const STATUS_FAILURE_BYTES: [u8; 16] = [
  0x24, 0x68, 0xf4, 0x6c, 0xbb, 0x60, 0x42, 0x5c, 0x9a, 0x4d, 0x9a, 0xd3, 0x26, 0xcc, 0xc7, 0xe2,
];
// Status enum type: 325a5767-e344-4532-860e-0749bcf2e428
const STATUS_ENUM_BYTES: [u8; 16] = [
  0x32, 0x5a, 0x57, 0x67, 0xe3, 0x44, 0x45, 0x32, 0x86, 0x0e, 0x07, 0x49, 0xbc, 0xf2, 0xe4, 0x28,
];

fn value_to_status(v: &Value) -> &'static str {
  if let Value::Enumeration(e) = v {
    if *e.variant_id.as_bytes() == STATUS_SUCCESS_BYTES {
      return "success";
    }
    if *e.variant_id.as_bytes() == STATUS_FAILURE_BYTES {
      return "failure";
    }
  }
  "running"
}

/// A node argument expression: either a literal value or a reference to a
/// named variable (identified by UUID).
#[derive(serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
enum BtExpression {
  VariableId(Uuid),
  Value(Value),
}

/// A single BT node – mirrors the arora-behavior-tree YAML schema.
#[derive(serde::Deserialize, Debug)]
struct BtNode {
  id: Uuid,
  function: Uuid,
  #[serde(default)]
  children: Option<Vec<Uuid>>,
  /// Parameter arguments: maps parameter UUID to a literal value or variable.
  #[serde(default)]
  arguments: HashMap<Uuid, BtExpression>,
  /// If set, the raw return value of this node's function is stored in this
  /// variable UUID instead of being interpreted as a Status. The node always
  /// succeeds in the trace.
  #[serde(default)]
  return_binding: Option<Uuid>,
}

/// Metadata extracted from a module header for one exported function.
struct FnMeta {
  module_id: Uuid,
  /// Parameter ID of the `children: TickId[]` argument, present only for
  /// control nodes (seq, fallback, parallel …).
  children_param_id: Option<Uuid>,
}

/// An arora Callable that wraps one BT node.
struct NodeCallable {
  node_id: Uuid,
  fn_id: Uuid,
  module_id: Uuid,
  children_param_id: Option<Uuid>,
  children_callable_ids: Vec<u64>,
  arguments: HashMap<Uuid, BtExpression>,
  return_binding: Option<Uuid>,
  variables: Rc<RefCell<HashMap<Uuid, Value>>>,
  trace: Rc<RefCell<Vec<(Uuid, &'static str)>>>,
}

impl Callable for NodeCallable {
  fn call(&self, caller: &mut dyn CallBridge) -> Result<Value, arora::call::CallError> {
    let tick_id_type = Uuid::from_bytes(TICK_ID_STRUCT_BYTES);
    let callable_field = Uuid::from_bytes(TICK_ID_CALLABLE_FIELD_BYTES);

    let mut args = Vec::new();

    if let Some(children_param_id) = self.children_param_id {
      let elements: Vec<StructureWithoutId> = self
        .children_callable_ids
        .iter()
        .map(|&id| StructureWithoutId {
          fields: vec![StructureField {
            id: callable_field,
            value: Box::new(Value::U64(id)),
          }],
        })
        .collect();
      args.push(StructureField {
        id: children_param_id,
        value: Box::new(Value::ArrayStructure {
          id: tick_id_type,
          elements,
        }),
      });
    }

    for (&param_id, expr) in &self.arguments {
      let value = match expr {
        BtExpression::Value(v) => v.clone(),
        BtExpression::VariableId(var_id) => {
          self.variables.borrow().get(var_id).cloned().unwrap_or(Value::Unit)
        }
      };
      args.push(StructureField {
        id: param_id,
        value: Box::new(value),
      });
    }

    // Build a map of param_id -> variable_id for mutable arguments so we can
    // write mutated values back to the variable store after the call.
    let mutable_param_vars: HashMap<Uuid, Uuid> = self
      .arguments
      .iter()
      .filter_map(|(&param_id, expr)| {
        if let BtExpression::VariableId(var_id) = expr {
          Some((param_id, *var_id))
        } else {
          None
        }
      })
      .collect();

    let result = caller.arora_call(
      &self.module_id,
      Call {
        module_id: None,
        id: self.fn_id,
        args,
      },
    )?;

    // Write back mutated parameter values to bound variables.
    for mutated in &result.mutated {
      if let Some(&var_id) = mutable_param_vars.get(&mutated.id) {
        self.variables.borrow_mut().insert(var_id, *mutated.value.clone());
      }
    }

    if let Some(var_id) = &self.return_binding {
      self.variables.borrow_mut().insert(*var_id, result.ret.clone());
    }

    let s = if self.return_binding.is_some() {
      "success"
    } else {
      value_to_status(&result.ret)
    };
    self.trace.borrow_mut().push((self.node_id, s));

    if self.return_binding.is_some() {
      Ok(Value::Enumeration(Enumeration {
        id: Uuid::from_bytes(STATUS_ENUM_BYTES),
        variant_id: Uuid::from_bytes(STATUS_SUCCESS_BYTES),
        value: Box::new(Value::Unit),
      }))
    } else {
      Ok(result.ret)
    }
  }
}

/// Recursively registers callables for `node_id` and all descendants.
/// Returns the callable id of the registered root callable.
fn register_node(
  engine: &mut dyn CallBridge,
  node_id: Uuid,
  node_index: &HashMap<Uuid, BtNode>,
  fn_meta: &HashMap<Uuid, FnMeta>,
  trace: &Rc<RefCell<Vec<(Uuid, &'static str)>>>,
  variables: &Rc<RefCell<HashMap<Uuid, Value>>>,
) -> Result<u64, String> {
  let node = node_index
    .get(&node_id)
    .ok_or_else(|| format!("node {node_id} not found in tree"))?;
  let meta = fn_meta
    .get(&node.function)
    .ok_or_else(|| format!("function {} not registered in fn_meta", node.function))?;

  let children_callable_ids = match &node.children {
    None => vec![],
    Some(ids) => ids
      .iter()
      .map(|&child_id| register_node(engine, child_id, node_index, fn_meta, trace, variables))
      .collect::<Result<Vec<_>, _>>()?,
  };

  let callable: Rc<dyn Callable> = Rc::new(NodeCallable {
    node_id,
    fn_id: node.function,
    module_id: meta.module_id,
    children_param_id: meta.children_param_id,
    children_callable_ids,
    arguments: node.arguments.clone(),
    return_binding: node.return_binding,
    variables: variables.clone(),
    trace: trace.clone(),
  });
  let id = engine.arora_register_callable(callable);
  Ok(id.id)
}

/// JS-callable handle for loading modules and executing behavior trees.
///
/// Usage:
/// 1. `new BehaviorTreeRunner()`
/// 2. `runner.loadModule(headerJson, wasmBytes)` – can be called for multiple modules
/// 3. `runner.run(nodesJson)` – returns `{status, trace}`
/// 4. Or `runner.setVariable(varId, valueJson)` + `runner.tick(nodesJson)` for
///    stateful tick-by-tick execution with variable bindings.
#[wasm_bindgen]
pub struct BehaviorTreeRunner {
  inner: Pin<Box<arora::engine::Engine>>,
  fn_meta: HashMap<Uuid, FnMeta>,
  variables: Rc<RefCell<HashMap<Uuid, Value>>>,
}

#[wasm_bindgen]
impl BehaviorTreeRunner {
  #[wasm_bindgen(constructor)]
  pub fn new() -> BehaviorTreeRunner {
    let inner = EngineBuilder::new().add_executor(BrowserExecutor::new()).build();
    BehaviorTreeRunner {
      inner,
      fn_meta: HashMap::new(),
      variables: Rc::new(RefCell::new(HashMap::new())),
    }
  }

  /// Load a WASM module. `header_json` must be the module's YAML header
  /// converted to JSON (the JS side can use js-yaml for that).
  /// Returns the module UUID string.
  #[wasm_bindgen(js_name = loadModule)]
  pub fn load_module(&mut self, header_json: &str, executable: &[u8]) -> Result<String, JsValue> {
    let header: Header =
      serde_json::from_str(header_json).map_err(|e| JsValue::from_str(&format!("invalid header: {e}")))?;
    let module_id = header.id;

    for export in &header.exports {
      let arora::schema::module::low::ExportSymbol::Function(f) = export;
      let children_param_id = f.parameters.first().and_then(|p| {
        if let arora::schema::module::low::TypeRef::Array { id } = &p.ty {
          if id == &Uuid::from_bytes(TICK_ID_STRUCT_BYTES) {
            Some(p.id)
          } else {
            None
          }
        } else {
          None
        }
      });

      self.fn_meta.insert(
        f.id,
        FnMeta {
          module_id,
          children_param_id,
        },
      );
    }

    load_module_from_parts(&mut *self.inner, header, executable.to_vec().into_boxed_slice())
      .map(|m| m.id.to_string())
      .map_err(|e| JsValue::from_str(&format!("load failed: {e}")))
  }

  /// Initialize or update a variable. `var_id` is a UUID string; `value_json`
  /// is the serialized `Value` (e.g. `{"f32": 0.0}`).
  #[wasm_bindgen(js_name = setVariable)]
  pub fn set_variable(&mut self, var_id: &str, value_json: &str) -> Result<(), JsValue> {
    let var_id: Uuid = var_id
      .parse()
      .map_err(|_| JsValue::from_str("bad var_id: not a valid UUID"))?;
    let value: Value = serde_json::from_str(value_json)
      .map_err(|e| JsValue::from_str(&format!("bad value JSON: {e}")))?;
    self.variables.borrow_mut().insert(var_id, value);
    Ok(())
  }

  /// Run one tick of the behavior tree. Variables persist across calls.
  ///
  /// `nodes_json` is a JSON array where each element is:
  ///   `{ id, function, children?, arguments?, return_binding? }`
  ///
  /// Returns: `{ "status": "...", "trace": [...], "variables": { varId: value } }`
  pub fn tick(&mut self, nodes_json: &str) -> Result<String, JsValue> {
    let nodes: Vec<BtNode> =
      serde_json::from_str(nodes_json).map_err(|e| JsValue::from_str(&format!("bad nodes JSON: {e}")))?;
    if nodes.is_empty() {
      return Err(JsValue::from_str("tree has no nodes"));
    }

    let root_id = nodes[0].id;
    let node_index: HashMap<Uuid, BtNode> = nodes.into_iter().map(|n| (n.id, n)).collect();
    let trace: Rc<RefCell<Vec<(Uuid, &'static str)>>> = Rc::new(RefCell::new(Vec::new()));

    let fn_meta = &self.fn_meta;
    let root_callable_id =
      register_node(&mut *self.inner, root_id, &node_index, fn_meta, &trace, &self.variables)
        .map_err(|e| JsValue::from_str(&format!("setup error: {e}")))?;

    let callable_id = CallableId { id: root_callable_id };
    let result = Callable::call(&callable_id, &mut *self.inner)
      .map_err(|e| JsValue::from_str(&format!("tick error: {e}")))?;
    let status = value_to_status(&result);

    let trace_json: Vec<serde_json::Value> = trace
      .borrow()
      .iter()
      .map(|(id, s)| serde_json::json!({ "nodeId": id.to_string(), "status": s }))
      .collect();

    let vars_json: serde_json::Map<String, serde_json::Value> = self
      .variables
      .borrow()
      .iter()
      .filter_map(|(id, v)| serde_json::to_value(v).ok().map(|jv| (id.to_string(), jv)))
      .collect();

    Ok(serde_json::json!({
      "status": status,
      "trace": trace_json,
      "variables": vars_json,
    })
    .to_string())
  }

  /// Run a behavior tree to completion (ticks until not Running).
  ///
  /// `nodes_json` is a JSON array where each element is:
  ///   `{ id: "<uuid>", function: "<uuid>", children?: ["<uuid>", ...] }`
  /// The first element is the root node.
  ///
  /// Returns a JSON string:
  ///   `{ "status": "success"|"failure"|"running",
  ///      "trace": [{"nodeId": "<uuid>", "status": "..."}] }`
  pub fn run(&mut self, nodes_json: &str) -> Result<String, JsValue> {
    let nodes: Vec<BtNode> =
      serde_json::from_str(nodes_json).map_err(|e| JsValue::from_str(&format!("bad nodes JSON: {e}")))?;
    if nodes.is_empty() {
      return Err(JsValue::from_str("tree has no nodes"));
    }

    let root_id = nodes[0].id;
    let node_index: HashMap<Uuid, BtNode> = nodes.into_iter().map(|n| (n.id, n)).collect();
    let trace: Rc<RefCell<Vec<(Uuid, &'static str)>>> = Rc::new(RefCell::new(Vec::new()));

    let fn_meta = &self.fn_meta;
    let root_callable_id =
      register_node(&mut *self.inner, root_id, &node_index, fn_meta, &trace, &self.variables)
        .map_err(|e| JsValue::from_str(&format!("setup error: {e}")))?;

    let callable_id = CallableId { id: root_callable_id };

    let mut last_status = "running";
    for _ in 0..10_000 {
      let result = Callable::call(&callable_id, &mut *self.inner)
        .map_err(|e| JsValue::from_str(&format!("tick error: {e}")))?;
      last_status = value_to_status(&result);
      if last_status != "running" {
        break;
      }
    }

    let trace_json: Vec<serde_json::Value> = trace
      .borrow()
      .iter()
      .map(|(id, s)| serde_json::json!({ "nodeId": id.to_string(), "status": s }))
      .collect();

    let out = serde_json::json!({
      "status": last_status,
      "trace": trace_json,
    });
    Ok(out.to_string())
  }
}

impl Default for BehaviorTreeRunner {
  fn default() -> Self {
    Self::new()
  }
}
