#[cfg(test)]
mod tests {
  use crate::{
    error::BehaviorTreeError, load_behavior_tree_nodes, nodes::*, run_behavior_tree,
    status::Status, BehaviorTree, BehaviorTreeRuntime,
  };
  use anyhow::Result;
  use arora::engine::{EngineBuilder, PinnedEngine};
  use arora_index::Index;
  use arora_registry::Registry;
  use arora_schema::{
    module::low::{Header, ModuleDefinition},
    value::Value,
  };
  use convert_case::{Case, Casing};

  use std::{cell::RefCell, path::Path, rc::Rc};
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
    let mut runtime = BehaviorTreeRuntime::setup(&behavior, Rc::new(index), &mut engine).unwrap();

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
    let mut runtime = BehaviorTreeRuntime::setup(&behavior, Rc::new(index), &mut engine).unwrap();
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
