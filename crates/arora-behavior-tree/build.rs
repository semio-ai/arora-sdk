use std::path::PathBuf;

use anyhow::{Ok, Result};
use arora_module_rust::{
  generate_common_sources, generate_enumeration_source, generate_mods_in_directories,
};
use arora_registry::local::ROOT_ID;
use arora_status::{declare_status_enumeration, STATUS_TYPE_ID};
use rustfmt::config::Config;

#[tokio::main]
pub async fn main() -> Result<()> {
  // analyze_module(header, context, registry)
  let mut generated_sources = generate_common_sources()?;
  generated_sources = generate_enumeration_source(
    &STATUS_TYPE_ID,
    &declare_status_enumeration(ROOT_ID.clone()),
    &"std".to_string(),
  )?
  .merge_with(&generated_sources);
  assert!(generate_mods_in_directories(&mut generated_sources)?);
  let source_path = PathBuf::from("src/arora_generated/");
  generated_sources.sync(source_path.clone()).await?;

  let rust_files: Vec<String> = generated_sources
    .list_all_mut()
    .into_iter()
    .filter_map(|(path, _)| {
      if path.ends_with(".rs") {
        Some(source_path.join(path).display().to_string())
      } else {
        None
      }
    })
    .collect();
  let rustfmt_status = tokio::process::Command::new("rustfmt")
    .args(&rust_files)
    .spawn()?
    .wait()
    .await?;
  if !rustfmt_status.success() {
    return Err(anyhow::anyhow!(
      "rustfmt exited with non-zero status: {:?}",
      rustfmt_status.code()
    ));
  }

  Ok(())
}
