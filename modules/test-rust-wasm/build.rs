use anyhow::{Ok, Result};
use arora_behavior_tree_types::{
  declare_behavior_tree_folder, declare_status_enumeration, BEHAVIOR_TREE_FOLDER_ID,
  STATUS_ENUMERATION_ID, STATUS_ENUMERATION_VERSION,
};
use arora_module_core::analyze_module_from_path;
use arora_module_rust::{generate_sources, rustfmt::apply_rustfmt};
use arora_registry::{
  local::{LocalRegistry, ROOT_ID},
  EditableRegistry,
};
use rustfmt::config::Config;
use std::path::PathBuf;

#[tokio::main]
pub async fn main() -> Result<()> {
  let mut registry = LocalRegistry::new();
  registry
    .add_folder(
      BEHAVIOR_TREE_FOLDER_ID,
      declare_behavior_tree_folder(ROOT_ID),
    )
    .await?;
  registry
    .tag_enumeration(
      STATUS_ENUMERATION_ID.to_owned(),
      STATUS_ENUMERATION_VERSION.to_owned(),
      declare_status_enumeration(BEHAVIOR_TREE_FOLDER_ID),
    )
    .await?;
  let assets = analyze_module_from_path("module.yaml", &mut registry).await?;
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
  apply_rustfmt(source_path).await?;
  Ok(())
}
