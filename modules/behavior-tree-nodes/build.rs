use anyhow::{Ok, Result};
use arora_behavior_tree_types::{
  declare_behavior_tree_folder, declare_status_enumeration, declare_tick_id_structure,
  BEHAVIOR_TREE_FOLDER_ID, STATUS_ENUMERATION_ID, STATUS_ENUMERATION_VERSION, TICK_ID_STRUCTURE_ID,
  TICK_ID_STRUCTURE_VERSION,
};
use arora_module_core::{analyze_module_from_path, header::module_frozen_from_header_file};
use arora_module_rust::{generate_records, generate_sources, rustfmt::apply_rustfmt};
use arora_registry::{
  local::{LocalRegistry, ROOT_ID},
  EditableRegistry,
};
use std::path::{Path, PathBuf};

#[tokio::main]
pub async fn main() -> Result<()> {
  let mut registry = LocalRegistry::new();

  // behavior_tree
  registry
    .add_folder(
      BEHAVIOR_TREE_FOLDER_ID,
      declare_behavior_tree_folder(ROOT_ID),
    )
    .await?;

  // behavior_tree.Status
  registry
    .tag_enumeration(
      STATUS_ENUMERATION_ID.to_owned(),
      STATUS_ENUMERATION_VERSION.to_owned(),
      declare_status_enumeration(BEHAVIOR_TREE_FOLDER_ID),
    )
    .await?;

  // behavior_tree.TickId
  registry
    .tag_structure(
      TICK_ID_STRUCTURE_ID.to_owned(),
      TICK_ID_STRUCTURE_VERSION.to_owned(),
      declare_tick_id_structure(BEHAVIOR_TREE_FOLDER_ID),
    )
    .await?;

  // test_rust_wasm
  let test_rust_wasm_module_path = Path::new("../test-rust-wasm/src/arora_generated/module.yaml");
  let (test_rust_wasm_id, test_rust_wasm_version, test_rust_wasm_module) =
    module_frozen_from_header_file(
      test_rust_wasm_module_path.to_owned(),
      &mut registry,
    )
    .await?;
  println!("cargo:rerun-if-changed={}", test_rust_wasm_module_path.display());
  registry
    .add_module(
      test_rust_wasm_id,
      test_rust_wasm_version,
      test_rust_wasm_module.module,
    )
    .await?;

  // Generate sources for the module
  let assets = analyze_module_from_path("module.yaml", &mut registry).await?;
  println!("cargo:rerun-if-changed={}", "module.yaml");
  let records = generate_records(&assets, &registry)
    .map_err(|e| anyhow::anyhow!("records: {e}"))?;
  let records_path = PathBuf::from("records/");
  records
    .sync(records_path.clone())
    .await
    .map_err(|e| anyhow::anyhow!("sync records: {e}"))?;
  println!("cargo:rerun-if-changed={}", records_path.display());
  let generated_sources = generate_sources(assets, &mut registry).await?;
  let source_path = PathBuf::from("src/arora_generated/");
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
  apply_rustfmt(source_path.to_owned()).await?;
  println!("cargo:rerun-if-changed={}", source_path.display());
  Ok(())
}
