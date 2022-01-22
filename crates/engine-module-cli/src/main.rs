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

use tokio::fs::{copy, read_to_string};
use url::Url;
use uuid::Uuid;

#[derive(Debug, Parser)]
struct Build {
  #[clap(short, long)]
  language: String,
}

#[derive(Debug, Parser)]
struct ExportType {
  #[clap(short, long)]
  input_file: String,

  #[clap(short, long)]
  no_resolution: bool,

  #[clap(short, long)]
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
  Build(Build),
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

async fn build(cmd: Build, registry: Registry) -> anyhow::Result<()> {
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

    let low_type = LowType {
      id,
      name: high_type.name,
      description: high_type.description,
      kind: match high_type.kind {
        HighTypeKind::Structure(high_structure) => {
          for (id, field) in high_structure.fields.iter() {
            match registry.lookup_type(&field.ty).await {
              Ok(id) => {

              },
              Err(err) => {
                eprintln!("Failed to lookup type {}: {}", field.ty, err);
                std::process::exit(1);
              }
            }
          }
        },
        HighTypeKind::Enumeration(high_enumeration) => {
          let low_values = HashMap::new();
          
          for (id, value) in high_enumeration.values.iter() {
            match registry.lookup_type(&value.ty).await {
              Ok(id) => {

              },
              Err(err) => {
                eprintln!("Failed to lookup type {}: {}", value.ty, err);
                std::process::exit(1);
              }
            }
          }
        }
      },
    };
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
    Commands::Build(build) => {
      println!("Building {}", build.language);
    },
    Commands::ExportType(export_type) => {
      
      } else {
        serde_yaml::from_str(&read_to_string(input_file).await?)?
      };

      let mut output_path = Path::new(&output_directory).to_path_buf();
      output_path.push("");
      output_path.push();
      copy(, )
    },
    Commands::ExportModule { configuration_file, executable_file, output_directory } => {
      println!("Exporting module to {}", output_directory);
    }
  }

  Ok(())
}
