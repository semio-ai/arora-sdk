mod schema;
mod status;
use arora::call::{Call, CallBridge, CallError, Callable, CallableId};
use arora_index::Index;
use arora_schema::{
  module::low::{Parameter, TypeRef},
  value::{ConversionError, StructureField, Value},
};
use derive_more::Display;
use schema::Node;
use status::Status;
use std::{collections::HashMap, rc::Rc};
use uuid::Uuid;

// Runtime.
//====================================================================
/// The behavior tree, binding all nodes, variables and types together.
pub struct BehaviorTree {
  /// Index of modules, functions, variables and types.
  index: Rc<Index>,
  /// The root node from which the tree stems.
  root: Rc<Node>,
  /// All the nodes, indexed by their ID.
  node_index: Rc<HashMap<Uuid, Rc<Node>>>,
}

/// Runs a behavior tree until it reaches the status success or failure.
pub fn run_behavior_tree(
  behavior: &mut BehaviorTree,
  caller: &mut dyn CallBridge,
) -> Result<status::Status, BehaviorTreeError> {
  let tick = setup_tick_function(
    behavior.root.clone(),
    behavior.node_index.clone(),
    behavior.index.clone(),
    caller,
  )?;

  let mut status = Status::Running;
  while status == Status::Running {
    status = tick.tick(caller)?;
  }
  return Ok(status);
}

fn setup_tick_function(
  node: Rc<Node>,
  node_index: Rc<HashMap<Uuid, Rc<Node>>>,
  index: Rc<Index>,
  caller: &mut dyn CallBridge,
) -> Result<TickId, BehaviorTreeError> {
  let nof_children = node
    .children
    .as_ref()
    .map(|children| children.len())
    .unwrap_or(0);
  let mut children_ticks: Vec<TickId> = Vec::with_capacity(nof_children);
  if let Some(children) = &node.children {
    for child_id in children {
      let child_node = node_index
        .clone()
        .get(child_id)
        .ok_or(BehaviorTreeError::ChildNodeNotFound {
          node: node.id.clone(),
          child: child_id.clone(),
        })?
        .clone();
      let tick_function_with_id = setup_tick_function(
        child_node.clone(),
        node_index.clone(),
        index.clone(),
        caller,
      )?;
      children_ticks.push(tick_function_with_id);
    }
  }
  let tick_function: Rc<dyn Callable> = Rc::new(TickFunction {
    node: node.clone(),
    index,
    children: children_ticks,
  });
  let callable_id = caller.arora_register_callable(tick_function);
  Ok(callable_id.into())
}

fn tick(
  caller: &mut dyn CallBridge,
  index: Rc<Index>,
  child_tick_ids: &Vec<TickId>,
  node: Rc<Node>,
) -> Result<status::Status, BehaviorTreeError> {
  let function = index.find_function(&node.function).map_err(|_| {
    BehaviorTreeError::CallError(CallError::FunctionNotFound {
      id: node.function.clone(),
    })
  })?;

  let mut call = Call {
    id: node.function.clone(),
    args: Vec::with_capacity(node.arguments.len() + if node.children.is_some() { 1 } else { 0 }),
  };

  let nof_children = node
    .children
    .as_ref()
    .map(|children| children.len())
    .unwrap_or(0);
  assert_eq!(nof_children, child_tick_ids.len());

  if let Some(_) = &node.children {
    // Find the `children` parameter by its type
    let children_params: Vec<&Parameter> = function
      .parameters
      .iter()
      .filter(|parameter| {
        if let TypeRef::Array { id: param_id } = parameter.ty {
          param_id == *TICK_ID_TYPE_ID && parameter.name == "children"
        } else {
          false
        }
      })
      .collect();

    let children_param: &Parameter = if children_params.is_empty() {
      Err(BehaviorTreeError::MissingChildrenParameter {
        node: node.id.clone(),
      })
    } else if children_params.len() > 1 {
      Err(BehaviorTreeError::InternalError {
        message: "two args are named \"children\" and accept an array of TickId".to_string(),
      })
    } else {
      children_params
        .first()
        .ok_or(BehaviorTreeError::InternalError {
          message: "single child parameter cannot be accessed".to_string(),
        })
    }?;

    // Pass the tick ids of the children.
    let mut children_arg = Vec::with_capacity(child_tick_ids.len());
    for child_tick_id in child_tick_ids {
      children_arg.push(arora_schema::value::StructureWithoutId {
        fields: vec![StructureField {
          id: *TICK_ID_ID_FIELD_ID,
          value: Box::new(Value::U64(child_tick_id.callable_id)),
        }],
      });
    }
    call.args.push(StructureField {
      id: children_param.id.clone(),
      value: Box::new(Value::ArrayStructure {
        id: *TICK_ID_TYPE_ID,
        elements: children_arg,
      }),
    })
  }

  // Pass the remaining parameters from the behavior-wise variables.
  for (param_id, variable_id) in &node.arguments {
    let value = index
      .variables
      .get(variable_id)
      .ok_or(BehaviorTreeError::VariableNotFound {
        variable: variable_id.clone(),
        node: node.id.clone(),
      })?;
    call.args.push(StructureField {
      id: param_id.clone(),
      value: Box::new(value.clone()),
    });
  }

  let result = caller
    .arora_call(&function.module, call)
    .map_err(|e| BehaviorTreeError::CallError(e))?;

  result
    .try_into()
    .map_err(|e| BehaviorTreeError::ConversionError(e))
}

/// An alternative to CallableId that refers to callables returning a Status.
#[derive(Clone, Hash, Eq, PartialEq)]
pub struct TickId {
  pub callable_id: u64,
}

impl From<&TickId> for CallableId {
  fn from(val: &TickId) -> Self {
    CallableId {
      id: val.callable_id,
    }
  }
}

impl From<TickId> for CallableId {
  fn from(val: TickId) -> Self {
    CallableId {
      id: val.callable_id,
    }
  }
}

impl From<CallableId> for TickId {
  fn from(callable_id: CallableId) -> Self {
    Self {
      callable_id: callable_id.id,
    }
  }
}

lazy_static::lazy_static! {
  pub static ref TICK_ID_TYPE_ID: Uuid = Uuid::parse_str("6f49e650-84ca-4899-a9bd-1f3bf17fab51").unwrap();
  pub static ref TICK_ID_ID_FIELD_ID: Uuid = Uuid::parse_str("237992d2-17d1-459f-bca1-7185fa6a69d7").unwrap();
}

/// Specialization of Callable that returns a Status.
trait Tickable {
  fn tick(&self, caller: &mut dyn CallBridge) -> Result<status::Status, BehaviorTreeError>;
}

impl Tickable for TickId {
  fn tick(&self, caller: &mut dyn CallBridge) -> Result<status::Status, BehaviorTreeError> {
    CallableId::from(self).tick(caller)
  }
}

impl Tickable for CallableId {
  fn tick(&self, caller: &mut dyn CallBridge) -> Result<status::Status, BehaviorTreeError> {
    let value = self
      .call(caller)
      .map_err(|e| BehaviorTreeError::CallError(e))?;
    value
      .try_into()
      .map_err(|_| BehaviorTreeError::ConversionError(ConversionError {}))
  }
}

/// The usual Tickable object in behavior trees, which is also Callable.
struct TickFunction {
  node: Rc<Node>,
  index: Rc<Index>,
  children: Vec<TickId>,
}

impl Tickable for TickFunction {
  fn tick(&self, caller: &mut dyn CallBridge) -> Result<status::Status, BehaviorTreeError> {
    tick(
      caller,
      self.index.clone(),
      &self.children,
      self.node.clone(),
    )
  }
}

impl Callable for TickFunction {
  fn call(&self, caller: &mut dyn CallBridge) -> Result<Value, CallError> {
    self
      .tick(caller)
      .map(Into::<Value>::into)
      .map_err(Into::<CallError>::into)
  }
}

// Loading behavior trees.
//====================================================================
pub fn load_behavior_tree_nodes(nodes: Vec<Node>) -> Result<BehaviorTree, BehaviorTreeError> {
  let mut node_index: HashMap<Uuid, Rc<Node>> = HashMap::new();
  let mut root: Option<Rc<Node>> = None;
  for node in nodes {
    let shared_node = Rc::new(node);
    if root.is_none() {
      // first node is the root?
      root = Some(shared_node.clone());
    }
    let existing_node = node_index.insert(shared_node.id.clone(), shared_node.clone());
    if let Some(existing_node) = existing_node {
      return Err(BehaviorTreeError::InconsistentTreeError {
        message: format!("duplicate node {}", existing_node.id),
      });
    }
  }

  Ok(BehaviorTree {
    index: Rc::new(Index::new()),
    root: root.unwrap(),
    node_index: Rc::new(node_index),
  })
}

// Error management
//=====================================================================
#[derive(Display, Debug)]
pub enum BehaviorTreeError {
  /// Error when parsing something, such as a behavior tree description.
  #[display(fmt = "parsing error: {}", message)]
  ParsingError { message: String },

  /// Error in the structure of the behavior tree:
  /// cycles, duplicate nodes, dangling references....
  #[display(fmt = "inconsistent behavior tree: {}", message)]
  InconsistentTreeError { message: String },

  /// Error when client performs a call to a module function.
  CallError(CallError),

  /// Client-side value conversion error.
  ConversionError(ConversionError),

  /// Variable referred in the behavior tree was not found.
  #[display(
    fmt = "variable \"{}\" used by node \"{}\" was not found",
    variable,
    node
  )]
  VariableNotFound { variable: Uuid, node: Uuid },

  #[display(fmt = "node \"{}\", child of node \"{}\" was not found", child, node)]
  ChildNodeNotFound { child: Uuid, node: Uuid },

  #[display(
    fmt = "children were specified for node \"{}\", but it does not accept them as a parameter",
    node
  )]
  MissingChildrenParameter { node: Uuid },

  #[display(fmt = "internal error: {}", message)]
  InternalError { message: String },
}

impl std::error::Error for BehaviorTreeError {}

impl<E: serde::de::Error> From<E> for BehaviorTreeError {
  fn from(e: E) -> Self {
    BehaviorTreeError::ParsingError {
      message: e.to_string(),
    }
  }
}

impl Into<CallError> for BehaviorTreeError {
  fn into(self) -> CallError {
    match self {
      BehaviorTreeError::CallError(e) => e,
      _ => CallError::Generic {
        message: self.to_string(),
      },
    }
  }
}

// Tests.
//=====================================================================
#[cfg(test)]
mod tests {
  use super::*;
  use anyhow::Result;
  use arora::engine::{EngineBuilder, PinnedEngine};
  use arora_registry::Registry;
  use arora_schema::module::low::{Header, ModuleDefinition};
  use std::path::Path;
  use tokio::{
    fs::{read_to_string, File},
    io::AsyncReadExt,
  };
  use url::Url;

  pub fn load_behavior_tree_yaml(yaml: &str) -> Result<BehaviorTree, BehaviorTreeError> {
    return load_behavior_tree_nodes(serde_yaml::from_str(yaml)?);
  }

  #[test]
  pub fn load_parse_error() -> Result<()> {
    let tree_yaml = "I'm singing in the rain...";
    assert!(load_behavior_tree_yaml(tree_yaml).is_err());
    return Ok(());
  }

  #[test]
  pub fn load_simple_tree() -> Result<()> {
    let tree_yaml = &super::schema::tests::SIMPLE_TREE_YAML;
    load_behavior_tree_yaml(tree_yaml)?;
    return Ok(());
  }

  #[tokio::test]
  pub async fn run_trivial_tree() {
    run_to_success(TRIVIAL_TREE, &vec!["test-rust-wasm".to_string()]).await;
  }

  #[tokio::test]
  pub async fn seq_of_success() {
    run_to_success(SEQ_OF_SUCCESS, &vec!["behavior-tree-nodes".to_string()]).await;
  }

  #[tokio::test]
  pub async fn seq_fail_middle() {
    assert_eq!(Status::Failure, run_base(SEQ_FAIL_MIDDLE).await);
  }

  async fn run_base(tree_yaml: &str) -> Status {
    run(tree_yaml, &vec!["behavior-tree-nodes".to_string()]).await
  }

  async fn run_to_success(tree_yaml: &str, modules: &Vec<String>) {
    let status = run(tree_yaml, modules).await;
    assert_eq!(status, Status::Success);
  }

  async fn run(tree_yaml: &str, modules: &Vec<String>) -> Status {
    let mut engine = EngineBuilder::new()
      .add_executor(arora::executor::wasm::WebAssemblyExecutor::new().unwrap())
      .build();
    let mut index = Index::new();

    let registry_uri = "https://raw.githubusercontent.com/semio-ai/arora-registry/behavior_tree/";
    let mut registry = Registry::new_with_base_uri(
      Url::parse(registry_uri).expect(format!("malformed registry URI: {}", registry_uri).as_str()),
    );

    for module in modules {
      load_module(&mut engine, &mut index, &mut registry, module).await;
    }
    let mut behavior = load_behavior_tree_yaml(tree_yaml).unwrap();
    behavior.index = Rc::new(index);
    run_behavior_tree(&mut behavior, &mut engine).unwrap()
  }

  /// Load a module built by this project, from under the `module/` directory
  async fn load_module(
    engine: &mut PinnedEngine,
    index: &mut Index,
    registry: &mut Registry,
    name: &String,
  ) {
    // Find the root directory of the repository.
    let current_crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or(file!().to_string());
    let mut path = Path::new(&current_crate_dir);
    loop {
      if path.join(".git").is_dir() {
        break;
      }
      path = path
        .parent()
        .expect("test not implemented from its git repository");
    }
    let repo_root = path;

    // Let us load the test Rust WASM module.
    let test_rust_wasm = repo_root.join("modules").join(name);
    // The header file should be directly in the sources.
    let header_path = test_rust_wasm
      .join("src")
      .join("arora_generated")
      .join("module.yaml");
    let header: Header = serde_yaml::from_str(
      &read_to_string(header_path.clone())
        .await
        .expect(format!("header file {} could not be read", header_path.display()).as_str()),
    )
    .expect(
      format!(
        "header file {} contains invalid yaml",
        header_path.display()
      )
      .as_str(),
    );

    // Register the types involved there.
    for type_id in header.type_dependencies() {
      if index.find_type(&type_id).is_ok() {
        continue;
      } else {
        let ty = registry.get_type(&type_id).await.expect(
          format!(
            "header provided in {} depends on type {} which is unknown",
            header_path.display(),
            type_id
          )
          .as_str(),
        );
        index.add_type(ty);
      }
    }
    index.add_module(&header).unwrap();

    // Find the executable in the right target directory (debug in priority)
    let test_rust_wasm_target = test_rust_wasm.join("target").join("wasm32-wasi");
    let target_subdir = if cfg!(debug_assertions) {
      "debug"
    } else {
      "release"
    };
    let module_path = test_rust_wasm_target
      .join(target_subdir)
      .join("test_rust_wasm.wasm");
    let mut executable_file = File::open(&module_path)
      .await
      .expect(format!("could not open executable file {}", module_path.display()).as_str());
    let mut executable = Vec::new();
    executable_file.read_to_end(&mut executable).await.unwrap();
    let executable = executable.into_boxed_slice();

    // Loading the module.
    let module_name = header.name.clone();
    engine
      .load_module(ModuleDefinition {
        schema_version: 0,
        header,
        executable,
      })
      .expect(format!("failed to load module {}", module_name).as_str());
  }

  /// A tree with a single node calling test-wasm.succeed()
  pub const TRIVIAL_TREE: &'static str = "\
- id: fc8e2c43-8f0a-461f-9b44-30cc45c4357f
  function: 00cd31a8-2cf4-48e6-a957-69a55de90424
";

  pub const SEQ_OF_SUCCESS: &'static str = "\
- id: fc8e2c43-8f0a-461f-9b44-30cc45c4357f
  function: 32246df6-ab5d-4f18-9221-23e28731de93
  children:
    - d50638bf-c44b-4f6e-a5f2-925fcfff71a8
    - 817e45e3-26ca-45a4-8537-ad70e3de1298
- id: d50638bf-c44b-4f6e-a5f2-925fcfff71a8
  function: 6696F0BD-E781-40CD-AEB5-8DC616F810D2
- id: 817e45e3-26ca-45a4-8537-ad70e3de1298
  function: 6696F0BD-E781-40CD-AEB5-8DC616F810D2
";

  pub const SEQ_FAIL_MIDDLE: &'static str = "\
- id: fc8e2c43-8f0a-461f-9b44-30cc45c4357f
  function: 32246df6-ab5d-4f18-9221-23e28731de93
  children:
    - d50638bf-c44b-4f6e-a5f2-925fcfff71a8
    - 817e45e3-26ca-45a4-8537-ad70e3de1298
    - 26aa23ea-85e9-4571-89d5-6f9656c344cb
- id: d50638bf-c44b-4f6e-a5f2-925fcfff71a8
  function: 6696F0BD-E781-40CD-AEB5-8DC616F810D2
- id: 817e45e3-26ca-45a4-8537-ad70e3de1298
  function: 3abbbfb6-d00d-41eb-88bb-97874267eaf6
- id: 26aa23ea-85e9-4571-89d5-6f9656c344cb
  function: 41ae5ed0-1d12-4b71-aab8-02e7efedf177
";
}
