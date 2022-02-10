use std::sync::Arc;

use arora_module_core::{Asset, Reader, Writer};
use arora_registry::Registry;
use arora_vfs::Entry;
use clap::Parser;

use arora_schema::module::high::ModuleDefinition as HighModuleDefinition;
use tokio::fs::read_to_string;

use log::debug;

use crate::resolve::resolve_module_header;

#[derive(Debug, Parser)]
pub struct Generate {
  #[clap(short, long, name = "configuration-file")]
  pub configuration_file: String,
  #[clap(short, long)]
  pub language: String,
  #[clap(short, long, name = "output-directory")]
  pub output_directory: String,

  #[clap(long, name = "dry-run")]
  pub dry_run: bool,

  pub var_args: Vec<String>,
}

fn print_entry(entry: Arc<Entry>, i: usize) {
  match *entry {
    Entry::Directory(ref directory) => {
      for (name, entry) in directory.entries.iter() {
        println!("{} {}", " ".repeat(i), name);
        print_entry(entry.clone(), i + 2);
      }
    }
    Entry::File(_) => {}
  }
}

pub async fn generate(cmd: Generate, registry: &mut Registry) -> anyhow::Result<()> {
  let module_definition: HighModuleDefinition =
    serde_yaml::from_str(&read_to_string(cmd.configuration_file).await?)?;
  let header = resolve_module_header(module_definition, registry).await?;

  let header_yaml = serde_yaml::to_string(&header)?;

  let mut generator_path = std::env::current_exe()?;
  generator_path.pop();
  generator_path.push(format!(
    "arora-module-{}{}",
    cmd.language,
    std::env::consts::EXE_SUFFIX
  ));

  let mut command = tokio::process::Command::new(&generator_path)
    .args(&["--self-id", &header.id.to_string()])
    .args(cmd.var_args)
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .spawn()
    .map_err(|_| anyhow::anyhow!("Failed to start generator {:?}", &generator_path))?;

  let mut stdin = command.stdin.as_mut().unwrap();
  let mut stdout = command.stdout.as_mut().unwrap();

  let mut writer = Writer::new(&mut stdin);
  let mut reader = Reader::new(&mut stdout);

  writer.write(Asset::Header(header.clone())).await?;

  for module in header.module_dependencies() {
    let dep_header = registry.get_module_header(&module).await?;
    writer.write(Asset::Header(dep_header.clone())).await?;
  }

  for ty in header.type_dependencies().iter() {
    if arora_schema::ty::PRIMITIVE_IDS.contains(ty) {
      continue;
    }

    let ty = registry.get_type(ty).await?;
    debug!("type {} fetched from registry", ty.name);
    writer.write(Asset::Type(ty)).await?;
  }

  for symbol in header.imports {
    writer.write(Asset::ImportSymbol(symbol)).await?;
  }

  for symbol in header.exports {
    writer.write(Asset::ExportSymbol(symbol)).await?;
  }

  writer.end().await?;

  let vfs = reader.read::<Arc<Entry>>().await?;

  assert!(reader.read::<Entry>().await?.is_none());

  let status = command.wait().await?;

  if !status.success() {
    anyhow::bail!("Generator failed with status {:?}", status);
  }

  if let Some(vfs) = vfs {
    // Now we have the vfs.

    if cmd.dry_run {
      println!("{}", cmd.output_directory);
      print_entry(vfs, 0);
      return Ok(());
    } else {
      vfs.sync(cmd.output_directory.clone().into()).await?;
    }
  } else {
    anyhow::bail!("Failed to read virtual filesystem");
  }

  let mut module_low = std::path::PathBuf::new();
  module_low.push(cmd.output_directory);
  module_low.push("module.yaml");
  tokio::fs::write(module_low, header_yaml).await?;

  Ok(())
}
