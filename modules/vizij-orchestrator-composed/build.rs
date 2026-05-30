use anyhow::Result;
use arora_module_core::{analyze_module_from_path, header::module_frozen_from_header_file};
use arora_module_rust::{generate_sources, rustfmt::apply_rustfmt};
use arora_registry::{local::LocalRegistry, EditableRegistry};
use std::path::{Path, PathBuf};

#[tokio::main]
pub async fn main() -> Result<()> {
  let mut registry = LocalRegistry::new();
  register_module_header(
    &mut registry,
    Path::new("../vizij-animation/src/arora_generated/module.yaml"),
  )
  .await?;
  register_module_header(
    &mut registry,
    Path::new("../vizij-node-graph/src/arora_generated/module.yaml"),
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
  println!("cargo:rerun-if-changed=module.yaml");
  Ok(())
}

async fn register_module_header(registry: &mut LocalRegistry, path: &Path) -> Result<()> {
  let (module_id, module_version, module) =
    module_frozen_from_header_file(path.to_owned(), registry).await?;
  registry
    .add_module(module_id, module_version, module.module)
    .await?;
  println!("cargo:rerun-if-changed={}", path.display());
  Ok(())
}
