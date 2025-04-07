#[cfg(test)]
pub mod tests {
  use crate::schema_groot;
  use crate::tree_node::TreeNode;
  use crate::{
    arora_generated::behavior_tree::status::Status, load_behavior_tree_yaml, nodes::*,
    run_behavior_tree, schema::Expression, BehaviorTree, BehaviorTreeRuntime, ModuleFunction,
  };
  use anyhow::Result;
  use arora::engine::{EngineBuilder, PinnedEngine};
  use arora_module_core::resolve::resolve_low_module;
  use arora_registry::local_yaml::load_records_from_yaml_dir;
  use arora_registry::ModuleFrozen;
  use arora_registry::{local::LocalRegistry, EditableRegistry, ReadableRegistry};
  use arora_schema::{
    module::low::{Header, ModuleDefinition},
    value::Value,
  };
  use assert_float_eq::*;
  use convert_case::{Case, Casing};
  use rand::seq::SliceRandom;
  use rand::thread_rng;
  use semio_record::{module::v0::frozen::ExportKind, record::Freezer};
  use semver::Version;
  use std::str::FromStr;
  use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
  };
  use tokio::{
    fs::{read_to_string, File},
    io::AsyncReadExt,
  };
  use uuid::Uuid;

  #[test]
  pub fn load_parse_error() -> Result<()> {
    let tree_yaml = "I'm singing in the rain...";
    assert!(load_behavior_tree_yaml(tree_yaml).is_err());
    return Ok(());
  }

  #[test]
  pub fn load_simple_tree() -> Result<()> {
    let tree_yaml = &crate::schema::tests::SIMPLE_TREE_YAML;
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
  pub async fn status_identity_update() -> Result<()> {
    let mut status_value: Rc<RefCell<Value>> = Rc::new(RefCell::new(Status::Success.into()));
    let behavior = status_identity(status_value.clone()).try_into()?;

    let (mut engine, index) = setup_engine_with_modules(&BASE_MODULE_NAMES).await;
    let mut runtime =
      BehaviorTreeRuntime::setup(&behavior, Rc::new(index), &mut engine, true).unwrap();

    assert_eq!(Status::Success, runtime.tick().unwrap());
    set_value(&mut status_value, Status::Running);
    assert_eq!(Status::Running, runtime.tick().unwrap());
    set_value(&mut status_value, Status::Failure);
    assert_eq!(Status::Failure, runtime.tick().unwrap());

    Ok(())
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
  pub async fn seq_run_last() -> Result<()> {
    let behavior = seq(vec![succeed(), succeed(), run()]).try_into()?;
    assert_eq!(Status::Running, tick_base(&behavior).await);
    Ok(())
  }

  #[tokio::test]
  pub async fn seq_run_middle_fail_last() -> Result<()> {
    let behavior = seq(vec![succeed(), run(), fail()]).try_into()?;
    assert_eq!(Status::Running, tick_base(&behavior).await);
    Ok(())
  }

  #[tokio::test]
  pub async fn seq_star_succeed() -> Result<()> {
    let behavior = seq_star(vec![succeed(), succeed(), succeed()]).try_into()?;
    assert_eq!(Status::Success, tick_base(&behavior).await);
    Ok(())
  }

  #[tokio::test]
  pub async fn seq_star_resumes() -> Result<()> {
    let mut first_status: Rc<RefCell<Value>> = Rc::new(RefCell::new(Status::Success.into()));
    let mut second_status: Rc<RefCell<Value>> = Rc::new(RefCell::new(Status::Running.into()));
    let behavior = seq_star(vec![
      status_identity(first_status.clone()),
      status_identity(second_status.clone()),
    ])
    .try_into()?;

    let (mut engine, index) = setup_engine_with_modules(&BASE_MODULE_NAMES).await;
    let mut runtime =
      BehaviorTreeRuntime::setup(&behavior, Rc::new(index), &mut engine, true).unwrap();
    // First tick moves the sequence to the second node.
    assert_eq!(Status::Running, runtime.tick().unwrap());

    // Second tick ignores the first node, and will only process the second one.
    set_value(&mut first_status, Status::Running);
    set_value(&mut second_status, Status::Success);
    assert_eq!(Status::Success, runtime.tick().unwrap());

    Ok(())
  }

  #[tokio::test]
  pub async fn fallback_succeeds() -> Result<()> {
    let behavior = fallback(vec![succeed(), fail()]).try_into()?;
    assert_eq!(Status::Success, tick_base(&behavior).await);
    Ok(())
  }

  #[tokio::test]
  pub async fn fallback_falls_back() -> Result<()> {
    let behavior = fallback(vec![fail(), succeed()]).try_into()?;
    assert_eq!(Status::Success, tick_base(&behavior).await);
    Ok(())
  }

  #[tokio::test]
  pub async fn parallel_succeeds() -> Result<()> {
    let behavior = parallel(vec![succeed(), succeed(), succeed()]).try_into()?;
    assert_eq!(Status::Success, tick_base(&behavior).await);
    Ok(())
  }

  #[tokio::test]
  pub async fn parallel_fails() -> Result<()> {
    let behavior = parallel(vec![run(), succeed(), fail()]).try_into()?;
    assert_eq!(Status::Failure, tick_base(&behavior).await);
    Ok(())
  }

  #[tokio::test]
  pub async fn parallel_runs() -> Result<()> {
    let behavior = parallel(vec![run(), succeed(), succeed()]).try_into()?;
    assert_eq!(Status::Running, tick_base(&behavior).await);
    Ok(())
  }

  #[tokio::test]
  pub async fn store_float() -> Result<()> {
    let storage: Rc<RefCell<Value>> = Rc::new(RefCell::new(Value::F32(0f32)));
    let expected_value = Value::F32(42f32);
    let behavior = store(
      Expression::Variable(storage.to_owned()),
      Expression::Value(expected_value.to_owned()),
    )
    .try_into()?;
    assert_eq!(Status::Success, tick_base(&behavior).await);
    assert_eq!(expected_value, *storage.borrow());
    Ok(())
  }

  #[tokio::test]
  pub async fn increasing_float() -> Result<()> {
    let storage: Rc<RefCell<Value>> = Rc::new(RefCell::new(Value::F32(0f32)));
    let delta = 42f32;
    let delta_value = Value::F32(delta);
    let behavior = increase(
      Expression::Variable(storage.to_owned()),
      Expression::Value(delta_value.to_owned()),
    )
    .try_into()?;

    let (mut engine, index) = setup_engine_with_modules(&BASE_MODULE_NAMES).await;
    let mut runtime =
      BehaviorTreeRuntime::setup(&behavior, Rc::new(index), &mut engine, true).unwrap();
    for i in 1..10 {
      println!("storage = {}", storage.borrow());
      assert_eq!(Status::Success, runtime.tick().unwrap());
      assert_eq!(Value::F32(delta * i as f32), *storage.borrow());
    }
    Ok(())
  }

  #[tokio::test]
  pub async fn cosine_signal() -> Result<()> {
    let angle: Rc<RefCell<Value>> = Rc::new(RefCell::new(Value::F32(0f32)));
    let cosine: Rc<RefCell<Value>> = Rc::new(RefCell::new(Value::F32(1f32)));
    let behavior = seq(vec![
      increase(
        Expression::Variable(angle.to_owned()),
        Expression::Value(Value::F32(0.1f32)),
      ),
      cos(
        Expression::Variable(angle.to_owned()),
        Expression::Variable(cosine.to_owned()),
      ),
    ])
    .try_into()?;

    let (mut engine, index) = setup_engine_with_modules(&vec![
      "test-rust-wasm".to_string(),
      "behavior-tree-nodes".to_string(),
    ])
    .await;
    let mut runtime =
      BehaviorTreeRuntime::setup(&behavior, Rc::new(index), &mut engine, true).unwrap();

    for i in 1..11 {
      assert_eq!(Status::Success, runtime.tick().unwrap());
      println!("angle={}, cosine={}", angle.borrow(), cosine.borrow());
      let expected_angle = i as f32 * 0.1f32;
      if let Value::F32(angle_value) = *angle.borrow() {
        assert_f32_near!(expected_angle, angle_value);
      } else {
        panic!("angle variable does not hold an f32");
      }
      let expected_cosine = expected_angle.cos();
      if let Value::F32(cosine_value) = *cosine.borrow() {
        assert_f32_near!(expected_cosine, cosine_value);
      } else {
        panic!("cosine variable does not hold an f32");
      }
    }
    Ok(())
  }

  #[ignore]
  #[tokio::test]
  pub async fn hello_polly() -> Result<()> {
    let behavior =
      TreeNode::action_node(Uuid::from_str("e5a41333-4848-411f-878c-f1d662ebb4a0").unwrap())
        .try_into()?;

    let (mut engine, index) = setup_engine_with_modules(&vec![
      "test-rust-wasm".to_string(),
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
  pub async fn polly_sequence_of_speech() -> Result<()> {
    const NAMES: [&'static str; 3] = ["Ross", "Braden", "Victor"];
    let name = NAMES.choose(&mut thread_rng()).unwrap();
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
      "test-rust-wasm".to_string(),
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

  #[tokio::test]
  pub async fn fake_listen_regex_dispatch() -> Result<()> {
    let name: Rc<RefCell<Value>> = Rc::new(RefCell::new(Value::String(String::new())));
    let name_expr = Expression::Variable(name.to_owned());
    let feeling: Rc<RefCell<Value>> = Rc::new(RefCell::new(Value::String(String::new())));
    let feeling_expr = Expression::Variable(feeling.to_owned());
    let input: Rc<RefCell<Value>> = Rc::new(RefCell::new(Value::String(String::new())));
    let input_expr = Expression::Variable(input.to_owned());

    let behavior: BehaviorTree = fallback(vec![
      seq(vec![
        is_str_set(name_expr.to_owned()),
        is_str_set(feeling_expr.to_owned()),
      ]),
      seq(vec![
        wait_str_set(input_expr.to_owned()),
        fallback(vec![
          regex_match(
            input_expr.to_owned(),
            Expression::Value(Value::String("(Ross|Braden|Victor)".to_string())),
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
      "test-rust-wasm".to_string(),
      "behavior-tree-nodes".to_string(),
    ])
    .await;

    let mut runtime =
      BehaviorTreeRuntime::setup(&behavior, Rc::new(index), &mut engine, true).unwrap();
    let mut status = Status::Running;
    let mut tick_count = 0;
    let expected_name = "Ross".to_string();
    let expected_feeling = "great".to_string();
    while status == Status::Running {
      tick_count += 1;
      println!("tick {}", tick_count);
      if tick_count == 5 {
        *input.borrow_mut() = Value::String(expected_name.to_owned());
      }
      if tick_count == 10 {
        *input.borrow_mut() = Value::String(expected_feeling.to_owned());
      }
      status = runtime.tick().unwrap();
      std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(status, Status::Success);

    if let Value::String(name_value) = &*name.borrow() {
      assert_eq!(&expected_name, name_value);
    } else {
      panic!("name variable does not hold a string");
    }

    if let Value::String(feeling_value) = &*feeling.borrow() {
      assert_eq!(&expected_feeling, feeling_value);
    } else {
      panic!("feeling variable does not hold a string");
    }
    Ok(())
  }

  #[ignore]
  #[tokio::test]
  pub async fn fake_listen_regex_dispatch_to_groot() -> Result<()> {
    let name = Uuid::new_v4();
    let name_expr = Expression::VariableId(name.to_owned());
    let feeling = Uuid::new_v4();
    let feeling_expr = Expression::VariableId(feeling.to_owned());
    let input = Uuid::new_v4();
    let input_expr = Expression::VariableId(input.to_owned());

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
    ]);

    let index = schema_groot::tests::setup_index().await;
    let mut variables = HashMap::new();
    variables.insert(name.to_owned(), "name".to_string());
    variables.insert(feeling.to_owned(), "feeling".to_string());
    variables.insert(input.to_owned(), "input".to_string());
    let behavior = schema_groot::BehaviorTree {
      root: schema_groot::Node::try_from_tree_node(&behavior, &index, &mut variables)?,
    };

    println!("{}", String::from_utf8(behavior.to_groot_xml()).unwrap());
    Ok(())
  }

  #[ignore]
  #[tokio::test]
  pub async fn fake_listen_polly_dialogue() -> Result<()> {
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
      "test-rust-wasm".to_string(),
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
    let mut runtime =
      BehaviorTreeRuntime::setup(behavior, Rc::new(index), &mut engine, true).unwrap();
    runtime.tick().unwrap()
  }

  async fn run_with_modules(behavior: &BehaviorTree, modules: &Vec<String>) -> Status {
    let (mut engine, index) = setup_engine_with_modules(&modules).await;
    run_behavior_tree(&behavior, Rc::new(index), &mut engine, true).unwrap()
  }

  async fn run_yaml_with_modules(tree_yaml: &str, modules: &Vec<String>) -> Status {
    let behavior = load_behavior_tree_yaml(tree_yaml).unwrap();
    run_with_modules(&behavior, &modules).await
  }

  async fn setup_engine_with_modules<'a>(
    modules: &Vec<String>,
  ) -> (PinnedEngine, HashMap<Uuid, ModuleFunction>) {
    let mut engine = EngineBuilder::new()
      .add_executor(arora::executor::wasm::WebAssemblyExecutor::new().unwrap())
      .add_executor(arora::executor::native::NativeExecutor::new())
      .build();
    let mut index = HashMap::new();
    let mut registry = LocalRegistry::new();
    let behavior_tree_types_yaml_dir =
      crate_root_path("arora-behavior-tree-types-yaml").join("records");
    load_records_from_yaml_dir(behavior_tree_types_yaml_dir, &mut registry)
      .await
      .unwrap();
    for module in modules {
      load_module(&mut engine, &mut index, &mut registry, module).await;
    }
    (engine, index)
  }

  /// Load a module built by this project, from under the `module/` directory
  async fn load_module<R: ReadableRegistry + EditableRegistry + Freezer>(
    engine: &mut PinnedEngine,
    index: &mut HashMap<Uuid, ModuleFunction>,
    registry: &mut R,
    name: &String,
  ) {
    println!("loading module {:#?}", name);
    let start_time = std::time::Instant::now();

    let module_root = module_root_path(name);
    let header = read_header_from_module_root(module_root.to_owned()).await;
    let module_id = header.id.to_owned();
    let module_version = header.version.to_owned();
    let module = resolve_low_module(header.to_owned(), registry)
      .await
      .expect("failed to resolve module info from header")
      .module;
    let actual_module_name = module.name.to_owned();
    add_module_functions_to_index(&module_id, &module, index);
    registry
      .add_module(module_id.to_owned(), module_version.into(), module)
      .await
      .expect(format!("failed to add module {} to registry", module_id).as_str());

    // Find the executable in the right target directory (debug in priority)
    let (module_target_dir, executable_prefix, executable_extension) =
      match header.executor.name.as_str() {
        "wasm" => (module_root.join("target").join("wasm32-wasip1"), "", "wasm"),
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
          (module_root.join("target"), "lib", executable_extension) // supposes it's the host
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

  async fn read_header_from_module_root(module_root: PathBuf) -> Header {
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
      module_root.file_name().unwrap(),
      &actual_module_name
    );
    header
  }

  pub async fn read_header_to_index<R: ReadableRegistry + EditableRegistry + Freezer>(
    name: &String,
    index: &mut HashMap<Uuid, ModuleFunction>,
    registry: &mut R,
  ) {
    let (module_id, module_version, module) = read_header(name.as_str(), registry).await;
    add_module_functions_to_index(&module_id, &module, index);
    registry
      .add_module(module_id.to_owned(), module_version, module)
      .await
      .expect(format!("failed to add module {} to registry", module_id).as_str());
  }

  async fn read_header<R: ReadableRegistry + EditableRegistry + Freezer>(
    name: &str,
    registry: &mut R,
  ) -> (Uuid, Version, ModuleFrozen) {
    let module_root = module_root_path(&name.to_string());
    let header = read_header_from_module_root(module_root).await;
    let module_id = header.id.to_owned();
    let module_version = header.version.to_owned().into();
    let module = resolve_low_module(header, registry)
      .await
      .expect("failed to resolve module info from header")
      .module;
    (module_id, module_version, module)
  }

  pub fn add_module_functions_to_index(
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
    let repo_root = repo_root_path();
    repo_root.join("modules").join(name)
  }

  pub fn crate_root_path<P: AsRef<Path>>(name: P) -> PathBuf {
    let repo_root = repo_root_path();
    repo_root.join("crates").join(name)
  }

  fn repo_root_path() -> PathBuf {
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
    path.to_path_buf()
  }

  lazy_static::lazy_static! {
    pub static ref BASE_MODULE_NAMES: Vec<String> = vec!["test-rust-wasm".to_string(), "behavior-tree-nodes".to_string()];
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
