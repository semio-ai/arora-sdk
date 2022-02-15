use std::{collections::HashMap, path::Path};

use arora_registry::Registry;
use arora_schema::{
  module::high::TypeRef as HighTypeRef,
  module::low::TypeRef as LowTypeRef,
  ty::{
    high::{Type as HighType, TypeKind as HighTypeKind},
    low::{
      Enumeration as LowEnumeration, EnumerationValue as LowEnumerationValue,
      Structure as LowStructure, StructureField as LowStructureField, Type as LowType,
      TypeKind as LowTypeKind,
    },
  },
};
use clap::{AppSettings, Parser, Subcommand};

use tokio::{
  fs::{read_to_string, File},
  io::AsyncWriteExt,
};
use url::Url;
use uuid::Uuid;

mod generate;
mod resolve;

use generate::generate;

#[derive(Debug, Parser)]
struct ExportType {
  #[clap(short, long, name = "input-file")]
  input_file: String,

  #[clap(short, long, name = "no-resolution")]
  no_resolution: bool,

  #[clap(short, long, name = "output-directory")]
  output_directory: String,
}

#[derive(Debug, Parser)]
struct ExportModule {
  #[clap(short, long)]
  configuration_file: String,

  #[clap(short, long)]
  executable_file: String,

  #[clap(short, long)]
  output_directory: String,
}

#[derive(Subcommand, Debug)]
enum Commands {
  Generate(generate::Generate),
  #[clap(name = "export-type")]
  ExportType(ExportType),
  #[clap(name = "export-module")]
  ExportModule(ExportModule),
}

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[clap(global_setting(AppSettings::PropagateVersion))]
#[clap(global_setting(AppSettings::UseLongFormatForHelpSubcommand))]
#[clap(global_setting(AppSettings::TrailingVarArg))]
struct Args {
  #[clap(short, long)]
  registry_uri: Option<String>,

  #[clap(subcommand)]
  command: Commands,
}

async fn lookup_type_ref(
  type_ref: &HighTypeRef,
  registry: &mut Registry,
) -> anyhow::Result<LowTypeRef> {
  Ok(match type_ref {
    HighTypeRef::Scalar { id } => LowTypeRef::Scalar {
      id: registry.lookup_type(&id).await?,
    },
    HighTypeRef::Array { id } => LowTypeRef::Array {
      id: registry.lookup_type(&id).await?,
    },
    HighTypeRef::Map { key_id, value_id } => LowTypeRef::Map {
      key_id: registry.lookup_type(&key_id).await?,
      value_id: registry.lookup_type(&value_id).await?,
    },
  })
}

async fn export_type(cmd: ExportType, registry: &mut Registry) -> anyhow::Result<()> {
  let low_type = if !cmd.no_resolution {
    let high_type: HighType = serde_yaml::from_str(&read_to_string(cmd.input_file).await?)?;
    let id = if let Ok(id) = registry.lookup_type(&high_type.name).await {
      id
    } else {
      Uuid::new_v4()
    };

    let kind = match high_type.kind {
      HighTypeKind::Structure(high_structure) => {
        let mut low_fields = HashMap::new();
        for (id, field) in high_structure.fields.iter() {
          match lookup_type_ref(&field.ty, registry).await {
            Ok(type_ref) => {
              low_fields.insert(
                *id,
                LowStructureField {
                  name: field.name.clone(),
                  type_ref,
                },
              );
            }
            Err(err) => {
              eprintln!("Failed to lookup type {:?}: {}", field.ty, err);
              std::process::exit(1);
            }
          }
        }
        LowTypeKind::Structure(LowStructure { fields: low_fields })
      }
      HighTypeKind::Enumeration(high_enumeration) => {
        let mut low_values = HashMap::new();

        for (id, value) in high_enumeration.values.iter() {
          match lookup_type_ref(&value.ty, registry).await {
            Ok(type_ref) => {
              low_values.insert(
                *id,
                LowEnumerationValue {
                  name: value.name.clone(),
                  type_ref,
                },
              );
            }
            Err(err) => {
              eprintln!("Failed to lookup type {:?}: {}", &value.ty, err);
              std::process::exit(1);
            }
          }
        }

        LowTypeKind::Enumeration(LowEnumeration { values: low_values })
      }
      HighTypeKind::Primitive(_) =>  {
        eprintln!("Forbidden to register primitive type {}", &high_type.name);
        std::process::exit(1);
      }
    };

    LowType {
      id,
      name: high_type.name,
      description: high_type.description,
      kind: kind,
    }
  } else {
    serde_yaml::from_str(&read_to_string(cmd.input_file).await?)?
  };

  let output_path =
    Path::new(&cmd.output_directory).join(format!("types/by-uuid/{}.yaml", low_type.id));

  let mut output_file = File::create(&output_path).await?;
  output_file
    .write_all(serde_yaml::to_string(&low_type)?.as_bytes())
    .await?;

  let name_output_path =
    Path::new(&cmd.output_directory).join(format!("types/by-name/{}", low_type.name));

  let mut name_output_file = File::create(&name_output_path).await?;
  name_output_file
    .write_all(format!("{}", low_type.id).as_bytes())
    .await?;

  Ok(())
}

async fn export_module(_: ExportModule, _: &mut Registry) -> anyhow::Result<()> {
  todo!("not implemented");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  env_logger::builder().init();

  let args = Args::parse();

  let mut registry = if let Some(uri) = args.registry_uri {
    Registry::new_with_base_uri(Url::parse(&uri)?)
  } else {
    Registry::new()
  };

  match args.command {
    Commands::Generate(cmd) => {
      generate(cmd, &mut registry).await?;
    }
    Commands::ExportType(export_type_data) => {
      export_type(export_type_data, &mut registry).await?;
    }
    Commands::ExportModule(export_module_data) => {
      println!(
        "Exporting module to {}",
        export_module_data.output_directory
      );
      export_module(export_module_data, &mut registry).await?;
    }
  }

  Ok(())
}
