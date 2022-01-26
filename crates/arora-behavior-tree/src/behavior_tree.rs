mod schema;
use arora::{module::DispatchError, call::{Caller, Call}};
use arora_index::Index;
use arora_schema::value::StructureField;
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
  index: Index,
  /// The root node from which the tree stems.
  root: Rc<Node>,
  /// All the nodes, indexed by their ID.
  node_index: HashMap<Uuid, Rc<Node>>,
}

/// Runs a behavior tree until it reaches the status success or failure.
pub fn run_behavior_tree<C: Caller>(
  behavior: &mut BehaviorTree,
  caller: &mut C,
) -> Result<status::Status, BehaviorTreeError> {
  let mut status = Status::Running;
  while status == Status::Running {
    status = tick(caller, &behavior.index, &behavior.root)?;
  }
  return Ok(status);
}

pub fn tick<C: Caller>(caller: &mut C, index: &Index, node: &Node) -> Result<status::Status, BehaviorTreeError> {
  let function = index.find_function(&node.function)
    .map_err(|_| BehaviorTreeError::CallError(DispatchError::FunctionNotFound { id: node.function.clone() }))?;

  let mut call = Call {
    id: node.function.clone(),
    args: Vec::with_capacity(node.arguments.len()),
  };

  for (param_id, variable_id) in &node.arguments {
    let value = index.variables.get(variable_id)
      .ok_or(BehaviorTreeError::VariableNotFound {
        variable: variable_id.clone(),
        node: node.id.clone(),
      })?;
    call.args.push(StructureField {
      id: param_id.clone(),
      value: Box::new(value.clone()),
    });
  }
  
  let result = caller.arora_call(&function.module, call)
    .map_err(|e| BehaviorTreeError::CallError(e))?;

  result.try_into()
    .map_err(|e| BehaviorTreeError::ConversionError(e))
}

// Loading behavior trees.
//====================================================================
pub fn load_behavior_tree_nodes(nodes: Vec<Node>) -> Result<BehaviorTree, BehaviorTreeError> {
  let mut node_index: HashMap<Uuid, Rc<Node>> = HashMap::new();
  let mut root: Option<Rc<Node>> = None;
  for node in nodes {
    let shared_node = Rc::new(node);
    if root.is_none() { // first node is the root?
      root = Some(shared_node.clone());
    }
    let existing_node = node_index.insert(shared_node.id.clone(), shared_node);
    if let Some(existing_node) = existing_node {
      return Err(BehaviorTreeError::InconsistentTreeError {
        message: format!("duplicate node {}", existing_node.id),
      });
    }
  }
  Ok(BehaviorTree {
    index: Index::new(),
    root: root.unwrap(),
    node_index,
  })
}

// Error management
//=====================================================================
#[derive(Display, Debug)]
pub enum BehaviorTreeError {
  /// Error when parsing something, such as a behavior tree description.
  #[display(fmt = "parsing error: {}", message)]
  ParsingError {
    message: String,
  },

  /// Error in the structure of the behavior tree:
  /// cycles, duplicate nodes, dangling references....
  #[display(fmt = "inconsistent behavior tree: {}", message)]
  InconsistentTreeError {
    message: String,
  },

  /// Error when client performs a call to a module function.
  CallError(DispatchError),

  /// Client-side value conversion error.
  ConversionError(ConversionError),
  
  /// Variable referred in the behavior tree was not found.
  #[display(fmt = "variable \"{}\" used by node \"{}\" was not found", variable, node)]
  VariableNotFound {
    variable: Uuid,
    node: Uuid,
  },  
}

impl std::error::Error for BehaviorTreeError {}

impl<E: serde::de::Error> From<E> for BehaviorTreeError {
  fn from(e: E) -> Self {
    BehaviorTreeError::ParsingError { message: e.to_string() }
  }
}

#[derive(Display, Debug)]
pub struct ConversionError {}

impl std::error::Error for ConversionError {}

// Binding of custom types.
//=====================================================================
mod status {
  use arora_schema::value::Value;
  use uuid::Uuid;

  use crate::ConversionError;

  #[derive(Debug, PartialEq)]
  pub enum Status {
    Success,
    Failure,
    Running
  }
  
  pub const STATUS_TYPE_ID: Uuid = Uuid::from_bytes([0x32, 0x5a, 0x57, 0x67, 0xe3, 0x44, 0x45, 0x32, 0x86, 0x0e, 0x07, 0x49, 0xbc, 0xf2, 0xe4, 0x28]);
  pub const STATUS_SUCCESS_VARIANT_ID: Uuid = Uuid::from_bytes([0x76, 0x6e, 0x9e, 0x9a, 0x44, 0x6d, 0x4e, 0x46, 0x83, 0xe6, 0x14, 0xb7, 0xca, 0x10, 0x11, 0x69]);
  pub const STATUS_FAILURE_VARIANT_ID: Uuid = Uuid::from_bytes([0x24, 0x68, 0xf4, 0x6c, 0xbb, 0x60, 0x42, 0x5c, 0x9a, 0x4d, 0x9a, 0xd3, 0x26, 0xcc, 0xc7, 0xe2]);
  pub const STATUS_RUNNING_VARIANT_ID: Uuid = Uuid::from_bytes([0xac, 0xd7, 0x9e, 0xc6, 0x0c, 0x44, 0x40, 0x1a, 0x82, 0xf8, 0x5d, 0xa5, 0x42, 0x2d, 0x3e, 0xec]);
  
  impl TryFrom<Value> for Status {
    type Error = ConversionError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
      if let Value::Enumeration(as_enum) = value {
        if as_enum.id == STATUS_TYPE_ID {
          match as_enum.variant_id {
            STATUS_SUCCESS_VARIANT_ID => Ok(Status::Success),
            STATUS_FAILURE_VARIANT_ID => Ok(Status::Failure),
            STATUS_RUNNING_VARIANT_ID => Ok(Status::Running),
            _ => Err(Self::Error{}),
          }
        } else {
          Err(Self::Error{})
        }
      } else {
        Err(Self::Error{})
      }
    }
  }
}

// Tests.
//=====================================================================
#[cfg(test)]
mod tests {
  use crate::status::{STATUS_TYPE_ID, STATUS_SUCCESS_VARIANT_ID, STATUS_FAILURE_VARIANT_ID, STATUS_RUNNING_VARIANT_ID};

use super::*;
  use anyhow::{Result, bail};
  use arora::engine::EngineBuilder;
  use arora_schema::{module::low::{Header, ModuleDefinition}, ty::{low::{Type, TypeKind, Enumeration, EnumerationValue}, UNIT_ID, PRIMITIVE_LOW_TYPE_REFS}};
  use std::path::Path;
  use tokio::{fs::{File, read_to_string}, io::AsyncReadExt};

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
  pub async fn run_trivial_tree() -> Result<()> {
    
    let mut engine = EngineBuilder::new()
      .add_executor(arora::executor::wasm::WebAssemblyExecutor::new()?)
      .build();

    let mut index = Index::new();

    // Register the Status type manually.
    index.add_type(Type {
      name: "Status".to_string(),
      id: STATUS_TYPE_ID,
      description: "Behavior tree node status".to_string(),
      kind: TypeKind::Enumeration(Enumeration {
        values: HashMap::from([
          (STATUS_SUCCESS_VARIANT_ID, EnumerationValue {
            name: "Success".to_string(),
            type_ref: PRIMITIVE_LOW_TYPE_REFS[&*UNIT_ID].clone()
          }),
          (STATUS_FAILURE_VARIANT_ID, EnumerationValue {
            name: "Failure".to_string(),
            type_ref: PRIMITIVE_LOW_TYPE_REFS[&*UNIT_ID].clone()
          }),
          (STATUS_RUNNING_VARIANT_ID, EnumerationValue {
            name: "Running".to_string(),
            type_ref: PRIMITIVE_LOW_TYPE_REFS[&*UNIT_ID].clone()
          }),
        ])
      })
    });

    // Find the root directory of the repository.
    let current_crate_dir = std::env::var("CARGO_MANIFEST_DIR")?;
    let mut path = Path::new(&current_crate_dir);
    loop {
      if path.join(".git").is_dir() {
        break;
      }
      path = path.parent().expect("test not implemented from its git repository");
    }
    let repo_root = path;

    // Let us load the test Rust WASM module.
    let test_rust_wasm = repo_root.join("modules").join("test-rust-wasm");
    // The header file should be directly in the sources.
    let header_path = test_rust_wasm.join("arora-generated").join("module.yaml");
    let header: Header = serde_yaml::from_str(
      &read_to_string(header_path.clone())
        .await
        .expect(format!("header file {} could not be read", header_path.display()).as_str()),
    ).expect(format!("header file {} contains invalid yaml", header_path.display()).as_str());  
    
    // Register the types involved there.
    for type_id in header.type_dependencies() {
      if index.find_type(&type_id).is_ok() {
        continue;
      } else {
        bail!("header provided in {} depends on type {} which is unknown", header_path.display(), type_id);
      }
    }
    index.add_module(&header)?;

    // Find the executable in the right target directory (debug in priority)
    let test_rust_wasm_target = test_rust_wasm.join("target").join("wasm32-wasi");
    let target_subdir = if cfg!(debug_assertions) { "debug" } else { "release" };
    let module_path = test_rust_wasm_target.join(target_subdir).join( "test_rust_wasm.wasm");
    let mut executable_file = File::open(&module_path).await
      .expect(format!("could not open executable file {}", module_path.display()).as_str());  
    let mut executable = Vec::new();
    executable_file.read_to_end(&mut executable).await?;
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

    let mut behavior = load_behavior_tree_yaml(TRIVIAL_TREE)?;
    behavior.index = index;

    let mut status = Status::Running;
    let tick_max = 100;
    let mut tick_count = 1;
    while status == Status::Running && tick_count <= tick_max {
      println!("tick {}/{}", tick_count, tick_max);
      tick_count += 1;
      status = tick(engine.as_mut().get_mut(), &behavior.index, &behavior.root)?;
    }
    assert_eq!(status, Status::Success);
    Ok(())
  }
  
  /// A tree with a single node calling test-wasm.succeed()
  pub const TRIVIAL_TREE: &'static str = "\
- id: fc8e2c43-8f0a-461f-9b44-30cc45c4357f
  function: 00cd31a8-2cf4-48e6-a957-69a55de90424
";
}
