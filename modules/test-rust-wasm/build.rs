use anyhow::{Ok, Result};
use arora_module_core::analyze_module_from_path;
use arora_module_rust::{generate_records, generate_sources, rustfmt::apply_rustfmt};
use arora_registry::local::LocalRegistry;
use std::path::PathBuf;

#[tokio::main]
pub async fn main() -> Result<()> {
    let mut registry = LocalRegistry::new();
    let assets = analyze_module_from_path("module.yaml", &mut registry).await?;
    let records =
        generate_records(&assets, &registry).map_err(|e| anyhow::anyhow!("records: {e}"))?;
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
                "failed to write generated sources to {}: {}",
                source_path.display(),
                err
            )
        })?;
    apply_rustfmt(source_path).await?;
    Ok(())
}
