use anyhow::Result;
use arora::engine::{EngineBuilder, PinnedEngine};
use arora_behavior_tree::{
  arora_generated::behavior_tree::status::Status, nodes::*, schema::Expression,
  tree_node::TreeNode, BehaviorTreeRuntime, ModuleFunction,
};
use arora_module_core::resolve::resolve_low_module;
use arora_registry::{local::LocalRegistry, EditableRegistry, ModuleFrozen, ReadableRegistry};
use arora_types::{
  module::low::{Header, ModuleDefinition},
  value::Value,
};
use convert_case::{Case, Casing};
use rand::prelude::IndexedRandom;
use rand::rng;
use semio_record::{module::v0::frozen::ExportKind, record::Freezer};
use std::str::FromStr;
use std::{
  cell::RefCell,
  collections::HashMap,
  path::{Path, PathBuf},
  rc::Rc,
};
use tokio::fs::{read_to_string, File};
use tokio::io::AsyncReadExt;
use uuid::Uuid;

#[ignore]
#[tokio::test]
async fn hello_polly() -> Result<()> {
  let behavior =
    TreeNode::action_node(Uuid::from_str("e5a41333-4848-411f-878c-f1d662ebb4a0").unwrap())
      .try_into()?;

  let (mut engine, index) = setup_engine_with_modules(&vec![
    "behavior-tree-nodes".to_string(),
    "polly".to_string(),
  ])
  .await;

  let mut runtime =
    BehaviorTreeRuntime::setup(&behavior, Rc::new(index), &mut engine, true).unwrap();
  while runtime.tick().unwrap() == Status::Running {}
  Ok(())
}

fn polly_say(text: Expression) -> TreeNode {
  TreeNode {
    function: Uuid::from_str("e1b4bda7-1c7b-4322-b9a0-552201b8a011").unwrap(),
    children: None,
    parameters: HashMap::from([(
      Uuid::from_str("fb3787f2-2151-49ce-8b61-6274984558ea").unwrap(),
      text,
    )]),
  }
}

#[ignore]
#[tokio::test]
async fn polly_sequence_of_speech() -> Result<()> {
  const NAMES: [&'static str; 3] = ["Ross", "Braden", "Victor"];
  let name = NAMES.choose(&mut rng()).unwrap();
  let behavior = seq_star(vec![
    polly_say(Expression::Value(Value::String("Hello!".to_string()))),
    polly_say(Expression::Value(Value::String(format!(
      "Oh, it's you, {}",
      name
    )))),
    polly_say(Expression::Value(Value::String(
      "How have you been?".to_string(),
    ))),
  ])
  .try_into()?;

  let (mut engine, index) = setup_engine_with_modules(&vec![
    "behavior-tree-nodes".to_string(),
    "polly".to_string(),
  ])
  .await;

  let mut runtime =
    BehaviorTreeRuntime::setup(&behavior, Rc::new(index), &mut engine, true).unwrap();
  let mut status = Status::Running;
  while status == Status::Running {
    status = runtime.tick().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
  }
  assert_eq!(status, Status::Success);
  Ok(())
}

#[ignore]
#[tokio::test]
async fn fake_listen_polly_dialogue() -> Result<()> {
  let name: Rc<RefCell<Value>> = Rc::new(RefCell::new(Value::String(String::new())));
  let name_expr = Expression::Variable(name.to_owned());
  let feeling: Rc<RefCell<Value>> = Rc::new(RefCell::new(Value::String(String::new())));
  let feeling_expr = Expression::Variable(feeling.to_owned());
  let input: Rc<RefCell<Value>> = Rc::new(RefCell::new(Value::String(String::new())));
  let input_expr = Expression::Variable(input.to_owned());

  let behavior = fallback(vec![
    seq(vec![
      is_str_set(name_expr.to_owned()),
      is_str_set(feeling_expr.to_owned()),
    ]),
    seq(vec![
      wait_str_set(input_expr.to_owned()),
      fallback(vec![
        regex_match(
          input_expr.to_owned(),
          Expression::Value(Value::String("(Ross|Brad|Victor)".to_string())),
          name_expr.to_owned(),
        ),
        regex_match(
          input_expr.to_owned(),
          Expression::Value(Value::String(
            "(fine|good|well|great|bad|terrible|tired|awkward)".to_string(),
          )),
          feeling_expr.to_owned(),
        ),
      ]),
      run(),
    ]),
  ])
  .try_into()?;

  let (mut engine, index) = setup_engine_with_modules(&vec![
    "behavior-tree-nodes".to_string(),
    "polly".to_string(),
  ])
  .await;

  let mut runtime =
    BehaviorTreeRuntime::setup(&behavior, Rc::new(index), &mut engine, true).unwrap();
  let mut status = Status::Running;
  let mut tick_count = 0;
  while status == Status::Running {
    if tick_count == 5 {
      *input.borrow_mut() = Value::String("Ross".to_string());
    }
    if tick_count == 10 {
      *input.borrow_mut() = Value::String("great".to_string());
    }
    status = runtime.tick().unwrap();
    tick_count += 1;
    std::thread::sleep(std::time::Duration::from_millis(50));
  }
  assert_eq!(status, Status::Success);
  Ok(())
}

// Test helpers (mirror of arora-behavior-tree test helpers)
//==============================================================================
async fn setup_engine_with_modules(
  modules: &Vec<String>,
) -> (PinnedEngine, HashMap<Uuid, ModuleFunction>) {
  let mut engine = EngineBuilder::new()
    .add_executor(arora::executor::wasm::WebAssemblyExecutor::new().unwrap())
    .add_executor(arora::executor::native::NativeExecutor::new())
    .build();

  let mut registry = LocalRegistry::new();
  let mut index = HashMap::new();

  for name in modules {
    load_module(name, &mut registry, &mut index, &mut engine).await;
  }

  (engine, index)
}

async fn load_module<R: ReadableRegistry + EditableRegistry + Freezer>(
  name: &String,
  registry: &mut R,
  index: &mut HashMap<Uuid, ModuleFunction>,
  engine: &mut PinnedEngine,
) {
  let module_root = module_root_path(name);
  let header = read_header_from_module_root(module_root.to_owned()).await;
  let module_id = header.id.to_owned();
  let module_version = header.version.to_owned();
  let module = resolve_low_module(header.to_owned(), registry)
    .await
    .expect("failed to resolve module info from header")
    .module;
  add_module_functions_to_index(&module_id, &module, index);
  registry
    .add_module(module_id.to_owned(), module_version.into(), module)
    .await
    .expect(format!("failed to add module {} to registry", module_id).as_str());

  let (module_target_dir, executable_prefix, executable_extension) =
    match header.executor.name.as_str() {
      "wasm" => (
        repo_root_path().join("target").join("wasm32-wasip1"),
        "",
        "wasm",
      ),
      "native" => {
        let executable_extension = if cfg!(target_os = "macos") || cfg!(target_os = "ios") {
          "dylib"
        } else if cfg!(target_family = "unix") {
          "so"
        } else if cfg!(target_family = "windows") {
          "dll"
        } else {
          panic!("unsupported platform")
        };
        (module_root.join("target"), "lib", executable_extension)
      }
      _ => panic!("unsupported executor"),
    };
  let target_subdir = if cfg!(debug_assertions) {
    "debug"
  } else {
    "release"
  };
  let module_path = module_target_dir.join(target_subdir).join(format!(
    "{}{}.{}",
    executable_prefix,
    name.to_case(Case::Snake),
    executable_extension
  ));
  let mut executable_file = File::open(&module_path)
    .await
    .expect(format!("could not open executable file {}", module_path.display()).as_str());
  let mut executable = Vec::new();
  executable_file.read_to_end(&mut executable).await.unwrap();
  let executable = executable.into_boxed_slice();

  engine
    .load_module(ModuleDefinition {
      schema_version: 0,
      header,
      executable,
    })
    .expect("failed to load module");
}

async fn read_header_from_module_root(module_root: PathBuf) -> Header {
  let header_path = module_root
    .join("src")
    .join("arora_generated")
    .join("module.yaml");
  serde_yaml::from_str(
    &read_to_string(header_path.clone())
      .await
      .expect(format!("header file {} could not be read", header_path.display()).as_str()),
  )
  .expect("invalid yaml")
}

fn add_module_functions_to_index(
  module_id: &Uuid,
  module: &ModuleFrozen,
  index: &mut HashMap<Uuid, ModuleFunction>,
) {
  for (export_id, export) in &module.exports {
    let ExportKind::Function(function) = &export.kind;
    index.insert(
      export_id.to_owned(),
      ModuleFunction {
        module_id: module_id.to_owned(),
        function_id: export_id.to_owned(),
        function_name: export.name.to_owned(),
        function: function.to_owned(),
      },
    );
  }
}

fn module_root_path<P: AsRef<Path>>(name: P) -> PathBuf {
  repo_root_path().join("modules").join(name)
}

fn repo_root_path() -> PathBuf {
  // tests/ dir is one level inside the module root
  std::env::current_dir()
    .unwrap()
    .parent()
    .unwrap()
    .parent()
    .unwrap()
    .to_path_buf()
}
