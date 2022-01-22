use std::{path::{Path, PathBuf}, any, collections::HashMap};

use clap::{Parser, Subcommand, AppSettings};
use engine_registry::Registry;
use engine_schema::ty::{
  low::{
    Type as LowType,
    TypeKind as LowTypeKind,
    Structure as LowStructure,
    Enumeration as LowEnumeration,
    StructureField as LowStructureField,
    EnumerationValue as LowEnumerationValue
  },
  high::{
    Type as HighType,
    TypeKind as HighTypeKind,
    Structure as HighStructure,
    Enumeration as HighEnumeration,
    StructureField as HighStructureField,
    EnumerationValue as HighEnumerationValue
  }
};

use tokio::{fs::{copy, read_to_string, File}, io::AsyncReadExt, io::AsyncWriteExt};
use url::Url;
use uuid::Uuid;

#[derive(Debug, Parser)]
struct Generate {
  #[clap(short, long)]
  language: String,
}

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
  Generate(Generate),
  #[clap(name = "export-type")]
  ExportType(ExportType),
  #[clap(name = "export-module")]
  ExportModule(ExportModule)
}


#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[clap(global_setting(AppSettings::PropagateVersion))]
#[clap(global_setting(AppSettings::UseLongFormatForHelpSubcommand))]
struct Args {
  #[clap(short, long)]
  registry_uri: Option<String>,

  #[clap(subcommand)]
  command: Commands,
}

async fn generate(cmd: Generate, registry: Registry) -> anyhow::Result<()> {
  Ok(())
}

async fn export_type(cmd: ExportType, registry: Registry) -> anyhow::Result<()> {
  let low_type = if !cmd.no_resolution {
    let high_type: HighType = serde_yaml::from_str(&read_to_string(cmd.input_file).await?)?;
    let id = if let Ok(id) = registry.lookup_type(&high_type.name).await {
      id
    } else {
      Uuid::new_v4()
    };

    LowType {
      id,
      name: high_type.name,
      description: high_type.description,
      kind: match high_type.kind {
        HighTypeKind::Structure(high_structure) => {
          let mut low_fields = HashMap::new();
          for (id, field) in high_structure.fields.iter() {
            match registry.lookup_type(&field.ty).await {
              Ok(ty_id) => {
                low_fields.insert(*id, LowStructureField {
                  name: field.name.clone(),
                  ty_id,
                });
              },
              Err(err) => {
                eprintln!("Failed to lookup type {}: {}", field.ty, err);
                std::process::exit(1);
              }
            }
          }
          LowTypeKind::Structure(LowStructure {
            fields: low_fields,
          })
        },
        HighTypeKind::Enumeration(high_enumeration) => {
          let mut low_values = HashMap::new();
          
          for (id, value) in high_enumeration.values.iter() {
            match registry.lookup_type(&value.ty).await {
              Ok(ty_id) => {
                low_values.insert(*id, LowEnumerationValue {
                  name: value.name.clone(),
                  ty_id,
                });
              },
              Err(err) => {
                eprintln!("Failed to lookup type {}: {}", value.ty, err);
                std::process::exit(1);
              }
            }
          }

          LowTypeKind::Enumeration(LowEnumeration {
            values: low_values,
          })
        }
      },
    }
  } else {
    serde_yaml::from_str(&read_to_string(cmd.input_file).await?)?
  };

  let output_path = Path::new(&cmd.output_directory)
    .join(format!("types/by-uuid/{}.yaml", low_type.id));

  let mut output_file = File::create(&output_path).await?;
  output_file.write_all(serde_yaml::to_string(&low_type)?.as_bytes()).await?;


  let name_output_path = Path::new(&cmd.output_directory)
    .join(format!("types/by-name/{}", low_type.name));

  let mut name_output_file = File::create(&name_output_path).await?;
  name_output_file.write_all(format!("{}", low_type.id).as_bytes()).await?;

  Ok(())
}

async fn export_module(cmd: ExportModule, registry: Registry) -> anyhow::Result<()> {
  Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let args = Args::parse();

  let registry = if let Some(uri) = args.registry_uri {
    Registry::new_with_base_uri(Url::parse(&uri)?)
  } else {
    Registry::new()
  };

  match args.command {
    Commands::Generate(generate) => {
      println!("Building {}", generate.language);
    },
    Commands::ExportType(export_type_data) => {
      println!("Exporting type {}", export_type_data.input_file);
      export_type(export_type_data, registry).await?;
    },
    Commands::ExportModule(export_module_data) => {
      println!("Exporting module to {}", export_module_data.output_directory);
    }
  }

  Ok(())
}
