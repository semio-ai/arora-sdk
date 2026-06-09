use anyhow::{Ok, Result};
use arora_behavior_tree_types::{
  declare_status_enumeration, declare_tick_id_structure, STATUS_ENUMERATION_ID,
  STATUS_ENUMERATION_VERSION, TICK_ID_STRUCTURE_ID, TICK_ID_STRUCTURE_VERSION,
};
use arora_module_rust::{
  generate_common_sources, generate_enumeration_source, generate_mods_in_directories,
  generate_structure_source, rustfmt::apply_rustfmt,
};
use arora_registry::{
  local::{LocalRegistry, ROOT_ID},
  EditableRegistry,
};
use std::path::PathBuf;

#[tokio::main]
pub async fn main() -> Result<()> {
  let mut registry = LocalRegistry::new();

  // Generate common sources.
  let mut generated_sources = generate_common_sources()?;

  // Generate sources for [`behavior_tree.Status`].
  let status = registry
    .tag_enumeration(
      STATUS_ENUMERATION_ID.to_owned(),
      STATUS_ENUMERATION_VERSION.to_owned(),
      declare_status_enumeration(ROOT_ID),
    )
    .await?;
  generated_sources = generate_enumeration_source(
    &STATUS_ENUMERATION_ID,
    &status,
    &"behavior_tree".to_string(),
  )?
  .merge_with(&generated_sources);

  // Generate sources for [`behavior_tree.TickId`]
  let tick_id = registry
    .tag_structure(
      TICK_ID_STRUCTURE_ID.to_owned(),
      TICK_ID_STRUCTURE_VERSION.to_owned(),
      declare_tick_id_structure(ROOT_ID),
    )
    .await?;
  generated_sources = generate_structure_source(
    &TICK_ID_STRUCTURE_ID,
    &tick_id,
    &mut registry,
    &"behavior_tree".to_string(),
  )
  .await?
  .merge_with(&generated_sources);

  // Generate mods.
  assert!(generate_mods_in_directories(&mut generated_sources)?);

  // Write to disk.
  let source_path = PathBuf::from("src/arora_generated/");
  generated_sources.sync(source_path.clone()).await?;

  // Apply rusfmt.
  apply_rustfmt(source_path).await?;

  // Forward artifact dependency paths to the test code
  // Cargo provides CARGO_CDYLIB_FILE_<normalized_dep_name> for artifact dependencies
  // We need to forward this to tests using cargo::rustc-env
  forward_env_var("CARGO_CDYLIB_FILE_BEHAVIOR_TREE_NODES_behavior_tree_nodes");

  Ok(())
}

fn forward_env_var(name: &str) {
  if let std::result::Result::Ok(val) = std::env::var(name) {
    println!("cargo::rustc-env={}={}", name, val);
  }
}
