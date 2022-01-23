use arora_module_core::{AssetWriter, Asset};
use arora_registry::Registry;
use clap::Parser;

use tokio::fs::read_to_string;
use arora_schema::module::{
  high::{ModuleDefinition as HighModuleDefinition},
  low::{Header as LowHeader},
};

use crate::resolve::resolve_module_header;

#[derive(Debug, Parser)]
pub struct Generate {
  #[clap(short, long, name = "configuration-file")]
  pub configuration_file: String,
  #[clap(short, long)]
  pub language: String,
  #[clap(short, long, name = "output-directory")]
  pub output_directory: String,
}

pub async fn generate(cmd: Generate, registry: &mut Registry) -> anyhow::Result<()> {
  let module_definition: HighModuleDefinition = serde_yaml::from_str(&read_to_string(cmd.configuration_file).await?)?;
  let header = resolve_module_header(module_definition, registry).await?;

  let mut generator_path = std::env::current_exe()?;
  generator_path.pop();
  generator_path.push(format!("arora-module-{}{}", cmd.language, std::env::consts::EXE_SUFFIX));

  let mut command = tokio::process::Command::new(&generator_path)
    .arg("--output_directory")
    .arg(cmd.output_directory)
    .stdin(std::process::Stdio::piped())
    .spawn()
    .map_err(|_| anyhow::anyhow!("Failed to start generator {:?}", &generator_path))?;


  let mut stdin = command.stdin.as_mut().unwrap();

  let mut writer = AssetWriter::new(&mut stdin);
  
  for ty in header.type_dependencies().iter() {
    if arora_schema::ty::PRIMITIVE_IDS.contains(ty) {
      continue;
    }

    writer.write(Asset::Type(registry.get_type(ty).await?)).await?;
  }

  for symbol in header.imports {
    println!("write import symbol {:?}", symbol);

    writer.write(Asset::ImportSymbol(symbol)).await?;
  }

  for symbol in header.exports {
    writer.write(Asset::ExportSymbol(symbol)).await?;
  }

  writer.end().await?;

  let status = command.wait().await?;

  if !status.success() {
    anyhow::bail!("Generator failed with status {:?}", status);
  }

  Ok(())
}