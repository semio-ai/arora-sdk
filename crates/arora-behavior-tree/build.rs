use std::path::PathBuf;

use anyhow::{Ok, Result};
use arora_module_rust::generate_enumeration_source;
use arora_registry::local::ROOT_ID;
use arora_status::{declare_status_enumeration, STATUS_TYPE_ID};

#[tokio::main]
pub async fn main() -> Result<()> {
  // analyze_module(header, context, registry)
  let generated_sources = generate_enumeration_source(
    &STATUS_TYPE_ID,
    &declare_status_enumeration(ROOT_ID.clone()),
    &"std".to_string(),
  )?;
  generated_sources
    .sync(PathBuf::from("src/arora_generated/"))
    .await?;
  Ok(())
}
