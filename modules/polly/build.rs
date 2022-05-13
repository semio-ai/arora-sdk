use anyhow::{Ok, Result};
use arora_module_core::analyze_module_from_path;
use arora_module_rust::{generate_sources, rustfmt::apply_rustfmt};
use arora_registry::{local::LocalRegistry, local_yaml::load_records_from_yaml_dir};
use rustfmt::config::Config;
use std::path::PathBuf;

#[tokio::main]
pub async fn main() -> Result<()> {
  // Use a local registry aware of behavior tree types.
  let mut registry = LocalRegistry::new();
  let behavior_tree_records_path = "../../crates/arora-behavior-tree-types-yaml/records";
  load_records_from_yaml_dir(behavior_tree_records_path, &mut registry).await?;
  println!("cargo:rerun-if-changed={}", behavior_tree_records_path);

  // Generate sources for the module
  let module_path = "module.yaml";
  let assets = analyze_module_from_path(module_path, &mut registry).await?;
  println!("cargo:rerun-if-changed={}", module_path);
  let generated_sources = generate_sources(assets, &mut registry).await?;
  let generated_sources_path = "src/arora_generated/";
  let source_path = PathBuf::from(generated_sources_path);

  generated_sources
    .sync(source_path.clone())
    .await
    .map_err(|err| {
      anyhow::anyhow!(
        "failed to write generated source files to {}: {}",
        source_path.display(),
        err
      )
    })?;
  apply_rustfmt(source_path).await?;
  println!("cargo:rerun-if-changed={}", generated_sources_path);
  
  Ok(())
}
