mod error;
mod nodes;
mod schema;
mod status;
mod tick_id;
use arora::call::{Call, CallBridge, CallError, Callable, CallableId};
use arora_index::Index;
use arora_schema::{
  module::low::{Parameter, TypeRef},
  value::{ConversionError, StructureField, Value},
};
use error::BehaviorTreeError;
use schema::Node;
use status::Status;
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use tick_id::TickId;
use uuid::Uuid;

use crate::tick_id::{TICK_ID_ID_FIELD_ID, TICK_ID_TYPE_ID};

// Runtime.
//====================================================================
/// The behavior tree, binding all nodes, variables and types together.
pub struct BehaviorTree {
  /// The root node from which the tree stems.
  root: Rc<Node>,
  /// All the nodes, indexed by their ID.
  node_index: HashMap<Uuid, Rc<Node>>,
  /// The local variables.
  locals: Rc<RefCell<HashMap<Uuid, Rc<RefCell<Value>>>>>,
}

struct BehaviorTreeRuntime<'a> {
  caller: &'a mut dyn CallBridge,
  tick: TickId,
}

impl<'a> BehaviorTreeRuntime<'a> {
  fn setup(
    tree: &'a BehaviorTree,
    index: Rc<Index>,
    caller: &'a mut dyn CallBridge,
  ) -> Result<Self, BehaviorTreeError> {
    let tick = setup_tick_function(
      tree.root.clone(),
      &tree.node_index,
      index.clone(),
      tree.locals.clone(),
      caller,
    )?;
    Ok(Self { caller, tick })
  }

  fn tick(&mut self) -> Result<Status, BehaviorTreeError> {
    self.tick.tick(self.caller)
  }
}

/// Runs a behavior tree until it reaches the status success or failure.
pub fn run_behavior_tree(
  behavior: &BehaviorTree,
  index: Rc<Index>,
  caller: &mut dyn CallBridge,
) -> Result<status::Status, BehaviorTreeError> {
  let mut runtime = BehaviorTreeRuntime::setup(behavior, index, caller)?;
  let mut status = Status::Running;
  while status == Status::Running {
    status = runtime.tick()?;
  }
  return Ok(status);
}

fn setup_tick_function(
  node: Rc<Node>,
  node_index: &HashMap<Uuid, Rc<Node>>,
  index: Rc<Index>,
  locals: Rc<RefCell<HashMap<Uuid, Rc<RefCell<Value>>>>>,
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
        .get(child_id)
        .ok_or(BehaviorTreeError::ChildNodeNotFound {
          node: node.id.clone(),
          child: child_id.clone(),
        })?
        .clone();
      let tick_function_with_id = setup_tick_function(
        child_node.clone(),
        &node_index,
        index.clone(),
        locals.clone(),
        caller,
      )?;
      children_ticks.push(tick_function_with_id);
    }
  }
  let tick_function: Rc<dyn Callable> = Rc::new(TickFunction {
    node: node.clone(),
    index,
    locals: locals.to_owned(),
    children: children_ticks,
  });
  let callable_id = caller.arora_register_callable(tick_function);
  Ok(callable_id.into())
}

fn tick(
  caller: &mut dyn CallBridge,
  index: Rc<Index>,
  locals: Rc<RefCell<HashMap<Uuid, Rc<RefCell<Value>>>>>,
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
  {
    let locals = locals.borrow();
    for (param_id, variable_id) in &node.arguments {
      let value = locals
        .get(variable_id)
        .ok_or(BehaviorTreeError::VariableNotFound {
          variable: variable_id.clone(),
          node: node.id.clone(),
        })?;
      call.args.push(StructureField {
        id: param_id.clone(),
        value: Box::new(value.borrow().clone()),
      });
    }
  }

  let result = caller
    .arora_call(&function.module, call)
    .map_err(|e| BehaviorTreeError::CallError(e))?;

  let mut mutable_locals = locals.borrow_mut();
  let mut return_value: Option<Status> = None;
  if let Value::Structure(result_structure) = result {
    if result_structure.id != node.function {
      return Err(BehaviorTreeError::ConversionError(ConversionError {
        message: format!(
          "node function's result id differs from function id ({}, vs. {})",
          result_structure.id.to_string(),
          node.function.to_string(),
        )
        .to_string(),
      }));
    }
    for field in result_structure.fields {
      if field.id == node.function {
        return_value = Some(
          (*field.value)
            .try_into()
            .map_err(|e| BehaviorTreeError::ConversionError(e))?,
        );
      } else {
        let variable_id =
          node
            .arguments
            .get(&field.id)
            .ok_or(BehaviorTreeError::ConversionError(ConversionError {
              message: "node function mutated an unknown argument".to_string(),
            }))?;
        let variable =
          mutable_locals
            .get_mut(variable_id)
            .ok_or(BehaviorTreeError::VariableNotFound {
              node: node.id,
              variable: variable_id.clone(),
            })?;
        println!("{} = {}", variable_id, field.value);
        *variable.borrow_mut() = *field.value;
      }
    }
  } else {
    return Err(BehaviorTreeError::ConversionError(ConversionError {
      message: "node function's result is not a structure".to_string(),
    }));
  }
  return_value.ok_or(BehaviorTreeError::ConversionError(ConversionError {
    message: "node function's result does not contain a return value".to_string(),
  }))
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
    value.try_into().map_err(|_| {
      BehaviorTreeError::ConversionError(ConversionError {
        message: "return value cannot be interpreted as a Status".to_string(),
      })
    })
  }
}

/// The usual Tickable object in behavior trees, which is also Callable.
struct TickFunction {
  node: Rc<Node>,
  index: Rc<Index>,
  locals: Rc<RefCell<HashMap<Uuid, Rc<RefCell<Value>>>>>,
  children: Vec<TickId>,
}

impl Tickable for TickFunction {
  fn tick(&self, caller: &mut dyn CallBridge) -> Result<status::Status, BehaviorTreeError> {
    tick(
      caller,
      self.index.clone(),
      self.locals.clone(),
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
    root: root.unwrap(),
    node_index,
    locals: Rc::new(RefCell::new(HashMap::new())),
  })
}

// Tests.
//=====================================================================
#[cfg(test)]
mod tests {
  use super::*;
  use crate::nodes::*;
  use anyhow::Result;
  use arora::engine::{EngineBuilder, PinnedEngine};
  use arora_registry::Registry;
  use arora_schema::module::low::{Header, ModuleDefinition};
  use convert_case::{Case, Casing};

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
    assert_eq!(
      Status::Success,
      run_yaml_with_modules(TRIVIAL_TREE, &vec!["test-rust-wasm".to_string()]).await
    );
  }

  #[tokio::test]
  pub async fn status_identity_update() {
    let mut status_value: Rc<RefCell<Value>> = Rc::new(RefCell::new(Status::Success.into()));
    let behavior = status_identity(status_value.clone()).into();

    let (mut engine, index) = setup_engine_with_modules(&BASE_MODULE_NAMES).await;
    let mut runtime = BehaviorTreeRuntime::setup(&behavior, Rc::new(index), &mut engine).unwrap();

    assert_eq!(Status::Success, runtime.tick().unwrap());
    set_value(&mut status_value, Status::Running);
    assert_eq!(Status::Running, runtime.tick().unwrap());
    set_value(&mut status_value, Status::Failure);
    assert_eq!(Status::Failure, runtime.tick().unwrap());
  }

  #[tokio::test]
  pub async fn seq_of_success() {
    assert_eq!(Status::Success, run_yaml_base(SEQ_OF_SUCCESS).await);
  }

  #[tokio::test]
  pub async fn seq_fail_middle() {
    assert_eq!(Status::Failure, run_yaml_base(SEQ_FAIL_MIDDLE).await);
  }

  #[tokio::test]
  pub async fn seq_run_last() {
    let behavior = seq(vec![succeed(), succeed(), run()]).into();
    assert_eq!(Status::Running, tick_base(&behavior).await);
  }

  #[tokio::test]
  pub async fn seq_run_middle_fail_last() {
    let behavior = seq(vec![succeed(), run(), fail()]).into();
    assert_eq!(Status::Running, tick_base(&behavior).await);
  }

  #[tokio::test]
  pub async fn seq_star_succeed() {
    let behavior = seq_star(vec![succeed(), succeed(), succeed()]).into();
    assert_eq!(Status::Success, tick_base(&behavior).await);
  }

  #[tokio::test]
  pub async fn seq_star_resumes() {
    let mut first_status: Rc<RefCell<Value>> = Rc::new(RefCell::new(Status::Success.into()));
    let mut second_status: Rc<RefCell<Value>> = Rc::new(RefCell::new(Status::Running.into()));
    let behavior = seq_star(vec![
      status_identity(first_status.clone()),
      status_identity(second_status.clone()),
    ])
    .into();

    let (mut engine, index) = setup_engine_with_modules(&BASE_MODULE_NAMES).await;
    let mut runtime = BehaviorTreeRuntime::setup(&behavior, Rc::new(index), &mut engine).unwrap();
    // First tick moves the sequence to the second node.
    assert_eq!(Status::Running, runtime.tick().unwrap());

    // Second tick ignores the first node, and will only process the second one.
    set_value(&mut first_status, Status::Running);
    set_value(&mut second_status, Status::Success);
    assert_eq!(Status::Success, runtime.tick().unwrap());
  }

  #[tokio::test]
  pub async fn fallback_succeeds() {
    let behavior = fallback(vec![succeed(), fail()]).into();
    assert_eq!(Status::Success, tick_base(&behavior).await);
  }

  #[tokio::test]
  pub async fn fallback_falls_back() {
    let behavior = fallback(vec![fail(), succeed()]).into();
    assert_eq!(Status::Success, tick_base(&behavior).await);
  }

  #[tokio::test]
  pub async fn parallel_succeeds() {
    let behavior = parallel(vec![succeed(), succeed(), succeed()]).into();
    assert_eq!(Status::Success, tick_base(&behavior).await);
  }

  #[tokio::test]
  pub async fn parallel_fails() {
    let behavior = parallel(vec![run(), succeed(), fail()]).into();
    assert_eq!(Status::Failure, tick_base(&behavior).await);
  }

  #[tokio::test]
  pub async fn parallel_runs() {
    let behavior = parallel(vec![run(), succeed(), succeed()]).into();
    assert_eq!(Status::Running, tick_base(&behavior).await);
  }

  // Test helpers and data
  //==============================================================================
  fn set_value<T: Into<Value>>(rc: &mut Rc<RefCell<Value>>, v: T) {
    *rc.borrow_mut() = v.into()
  }

  async fn tick_base(behavior: &BehaviorTree) -> Status {
    tick_with_modules(&behavior, &BASE_MODULE_NAMES).await
  }

  async fn run_yaml_base(tree_yaml: &str) -> Status {
    run_yaml_with_modules(tree_yaml, &BASE_MODULE_NAMES).await
  }

  async fn tick_with_modules(behavior: &BehaviorTree, modules: &Vec<String>) -> Status {
    let (mut engine, index) = setup_engine_with_modules(modules).await;
    let mut runtime = BehaviorTreeRuntime::setup(behavior, Rc::new(index), &mut engine).unwrap();
    runtime.tick().unwrap()
  }

  async fn run_with_modules(behavior: &BehaviorTree, modules: &Vec<String>) -> Status {
    let (mut engine, index) = setup_engine_with_modules(&modules).await;
    run_behavior_tree(&behavior, Rc::new(index), &mut engine).unwrap()
  }

  async fn run_yaml_with_modules(tree_yaml: &str, modules: &Vec<String>) -> Status {
    let behavior = load_behavior_tree_yaml(tree_yaml).unwrap();
    run_with_modules(&behavior, &modules).await
  }

  async fn setup_engine_with_modules<'a>(modules: &Vec<String>) -> (PinnedEngine, Index) {
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
    (engine, index)
  }

  /// Load a module built by this project, from under the `module/` directory
  async fn load_module(
    engine: &mut PinnedEngine,
    index: &mut Index,
    registry: &mut Registry,
    name: &String,
  ) {
    println!("loading module {:#?}", name);
    let start_time = std::time::Instant::now();

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

    // Let us load the WASM module.
    let module_root = repo_root.join("modules").join(name);
    // The header file should be directly in the sources.
    let header_path = module_root
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
    let actual_module_name = header.name.clone();
    println!(
      "actual name of module {:?} is {:?}",
      name, &actual_module_name
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
    let module_target_dir = module_root.join("target").join("wasm32-wasi");
    let target_subdir = if cfg!(debug_assertions) {
      "debug"
    } else {
      "release"
    };
    let module_path = module_target_dir
      .join(target_subdir)
      .join(format!("{}.wasm", name.to_case(Case::Snake)));
    println!("reading executable {:#?}", module_path);
    let mut executable_file = File::open(&module_path)
      .await
      .expect(format!("could not open executable file {}", module_path.display()).as_str());
    let mut executable = Vec::new();
    executable_file.read_to_end(&mut executable).await.unwrap();
    let executable = executable.into_boxed_slice();

    // Loading the module.
    println!("loading module {:#?} into the engine", &actual_module_name);
    engine
      .load_module(ModuleDefinition {
        schema_version: 0,
        header,
        executable,
      })
      .expect(format!("failed to load module {:#?}", &actual_module_name).as_str());

    let total_duration = std::time::Instant::now() - start_time;
    println!(
      "module {:#?} loaded in {:#?}",
      &actual_module_name, total_duration
    );
  }

  lazy_static::lazy_static! {
    static ref BASE_MODULE_NAMES: Vec<String> = vec!["behavior-tree-nodes".to_string()];
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
