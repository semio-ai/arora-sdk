use arora_module_core::{ModuleAsset, Reader, Writer};
use arora_module_rust::generate_sources;
use arora_registry::{local::LocalRegistry, EditableRegistry, TypeDefinition};
use arora_vfs::Entry;
use clap::Parser;
use std::fmt::Debug;
use tokio::io::{stdin, stdout, AsyncWriteExt};

#[derive(Parser, Debug)]
#[clap(long_about = None)]
pub struct Args {
  #[clap(short, long, name = "self-id")]
  pub self_id: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let mut registry = LocalRegistry::new();
  let mut types = Vec::new();
  let mut imports = Vec::new();
  let mut assets = Vec::<ModuleAsset>::new();
  let mut stdin = stdin();
  let mut reader = Reader::new(&mut stdin);
  while let Ok(Some(asset)) = reader.read::<ModuleAsset>().await {
    match &asset {
      ModuleAsset::Type(id, type_def) => {
        match type_def {
          TypeDefinition::Enumeration(enumeration) => {
            registry
              .add_enumeration(id.to_owned(), enumeration.to_owned())
              .await?;
          }
          TypeDefinition::Structure(structure) => {
            registry
              .add_structure(id.to_owned(), structure.to_owned())
              .await?;
          }
          TypeDefinition::Primitive(_) => {}
        }
        types.push((id.to_owned(), type_def.to_owned()));
      }
      ModuleAsset::Import(import) => imports.push(import.to_owned()),
      ModuleAsset::Module(id, module) => {
        registry
          .add_module(id.to_owned(), module.to_owned())
          .await?
      }
    };
    assets.push(asset);
  }

  let generated_sources = generate_sources(assets, &mut registry).await?;

  let mut stdout = stdout();
  let mut writer = Writer::new(&mut stdout);
  writer
    .write::<arora_vfs::Entry>(Entry::Directory(generated_sources))
    .await?;
  writer.end().await?;
  stdout.flush().await?;

  Ok(())
}
