use anyhow::{Ok, Result};
use arora_behavior_tree_types::{
  declare_status_enumeration, declare_tick_id_structure, STATUS_ENUMERATION_ID,
  TICK_ID_ENUMERATION_ID,
};
use arora_module_rust::{
  generate_common_sources, generate_enumeration_source, generate_mods_in_directories,
  generate_structure_source, rustfmt::apply_rustfmt,
};
use arora_registry::local::{LocalRegistry, ROOT_ID};
use rustfmt::config::Config;
use std::path::PathBuf;

#[tokio::main]
pub async fn main() -> Result<()> {
  let mut registry = LocalRegistry::new();

  // Generate common sources.
  let mut generated_sources = generate_common_sources()?;

  // Generate sources for [`behavior_tree.Status`].
  generated_sources = generate_enumeration_source(
    &STATUS_ENUMERATION_ID,
    &declare_status_enumeration(ROOT_ID.clone()),
    &"behavior_tree".to_string(),
  )?
  .merge_with(&generated_sources);

  // Generate sources for [`behavior_tree.TickId`]
  generated_sources = generate_structure_source(
    &TICK_ID_ENUMERATION_ID,
    &declare_tick_id_structure(ROOT_ID.clone()),
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
  Ok(())
}
