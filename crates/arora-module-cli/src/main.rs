mod generate;
use std::{
  ffi::OsStr,
  path::{Path, PathBuf},
  str::FromStr,
};

use anyhow::bail;
use arora_registry::{
  remote_cached::RemoteCachedRegistry, EditableRegistry, EnumerationPublic, FolderPublic,
  ModulePublic, StructurePublic,
};
use clap::{Parser, Subcommand};
use generate::generate;
use reqwest::{
  header::{self, HeaderValue},
  Client, Url,
};
use semio_client::{
  authentication::{access_token, Config, ConfigMutation},
  context::Context,
  mutation::Mutation,
  user::{self, Login},
};
use tokio::fs::read_to_string;
use uuid::Uuid;

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
}

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[clap(propagate_version = true)]
#[clap(trailing_var_arg = true)]
struct Args {
  #[clap(
    short,
    long,
    help = "Path to a semio-cli configuration file to reuse and potentially update."
  )]
  config: Option<String>,

  #[clap(
    short,
    long,
    default_value = "http://localhost:8080",
    help = "URL of the registry to use. Overrides and updates the configuration file if provided."
  )]
  registry_url: String,

  #[clap(
    short,
    long,
    name = "user-name",
    help = "User name to authenticate with. Overrides and updates the configuration file if provided."
  )]
  user_name: Option<String>,

  #[clap(
    short,
    long,
    help = "Password to authenticate with. Updates the configuration file if provided."
  )]
  password: Option<String>,

  #[clap(subcommand)]
  command: Commands,

  #[clap(
    short,
    long,
    help = "Include entities in the registry. It should be the path to a directory of entities."
  )]
  include: Vec<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  env_logger::builder().init();

  let args = Args::parse();

  // Read the configuration file if specified.
  let mut config: Option<Config> = if let Some(config_path) = &args.config {
    let config_str = read_to_string(&config_path)
      .await
      .expect(format!("Failed to read configuration file {}", config_path).as_str());
    Some(serde_yaml::from_str(&config_str)?)
  } else {
    None
  };

  // Set the registry URL, update configuration file if specified.
  let url = Url::parse(&args.registry_url).map_err(|_| anyhow::anyhow!("Invalid registry URL"))?;
  if let Some(conf) = config {
    config = Some(
      ConfigMutation {
        url: Mutation::Set(url.to_string()),
        ..Default::default()
      }
      .next(conf),
    );
  }

  // Authentication.
  let mut token = None;

  // User name is provided, update configuration file if specified.
  if let Some(user_name) = args.user_name {
    let password = args.password.clone().unwrap_or("".to_string());
    let login = Login {
      user_name,
      password,
    };
    let context = Context::new(url.to_owned(), Client::builder().build()?);
    let login_result = user::login(&context, login).await?;
    token = Some(login_result.access_token.token.to_owned());
    if let Some(conf) = config {
      config = Some(
        ConfigMutation {
          access: Mutation::Set(login_result.access_token),
          refresh: if let Some(refresh_token) = login_result.refresh_token {
            Mutation::Set(refresh_token)
          } else {
            Mutation::Unset
          },
          user_id: Mutation::Set(login_result.id),
          ..Default::default()
        }
        .next(conf),
      );
    }
  }

  // Password is provided without user name, we can't authenticate.
  if args.password.is_some() {
    bail!("Password provided without user name");
  }

  // If not yet authenticated, try to authenticate with the configuration file.
  if token.is_none() {
    if let Some(conf) = config {
      let (new_token, config_mutation) = access_token(&conf).await.map_err(|err| {
        anyhow::anyhow!(
          "error while refreshing authentication token from configuration: {}",
          err
        )
      })?;
      token = new_token;
      config = Some(config_mutation.next(conf));
    } else {
      bail!("No authentication information provided");
    }
  }

  // Update the configuration file.
  if let Some(conf) = config {
    let config_str = serde_yaml::to_string(&conf)?;
    if let Some(config_path) = &args.config {
      tokio::fs::write(config_path, config_str).await?;
    }
  }

  // Setup the context with the refreshed token.
  let token = token.expect("Token still missing after authentication succeeded.");
  let mut headers = header::HeaderMap::new();
  headers.insert(
    header::AUTHORIZATION,
    HeaderValue::from_str(token.as_str())?,
  );
  let client = Client::builder().default_headers(headers).build()?;
  let context = Context::new(url, client);

  // Connect to the remote registry, and add entities added locally.
  let mut registry = RemoteCachedRegistry::new(context);
  for include in args.include {
    let include_path = PathBuf::from_str(include.as_str())?;
    if !include_path.exists() {
      eprintln!(
        "include path {} does not exist and is ignored",
        include_path.display()
      );
    }

    let mut folders = Vec::new();
    for_each_uuid_yaml(&include_path.join("folder"), &mut |id, yaml: String| {
      folders.push((id, serde_yaml::from_str::<FolderPublic>(yaml.as_str())?));
      Ok(())
    })
    .await?;
    for (id, folder) in folders {
      registry.add_folder(id, folder).await?;
    }

    let mut enumerations = Vec::new();
    for_each_uuid_yaml(
      &include_path.join("enumeration"),
      &mut |id, yaml: String| {
        enumerations.push((
          id,
          serde_yaml::from_str::<EnumerationPublic>(yaml.as_str())?,
        ));
        Ok(())
      },
    )
    .await?;
    for (id, enumeration) in enumerations {
      registry.add_enumeration(id, enumeration).await?;
    }

    let mut structures = Vec::new();
    for_each_uuid_yaml(&include_path.join("structure"), &mut |id, yaml: String| {
      structures.push((id, serde_yaml::from_str::<StructurePublic>(yaml.as_str())?));
      Ok(())
    })
    .await?;
    for (id, structure) in structures {
      registry.add_structure(id, structure).await?;
    }

    let mut modules = Vec::new();
    for_each_uuid_yaml(&include_path.join("module"), &mut |id, yaml: String| {
      modules.push((id, serde_yaml::from_str::<ModulePublic>(yaml.as_str())?));
      Ok(())
    })
    .await?;
    for (id, module) in modules {
      registry.add_module(id, module).await?;
    }
  }

  // Perform the command.
  match args.command {
    Commands::Generate(cmd) => {
      generate(cmd, &mut registry).await?;
    }
  }

  Ok(())
}

async fn for_each_uuid_yaml<F>(path: &Path, mut f: F) -> anyhow::Result<()>
where
  F: FnMut(Uuid, String) -> anyhow::Result<()>,
{
  let mut dir = match tokio::fs::read_dir(path).await {
    Ok(dir) => dir,
    _ => return Ok(()),
  };
  while let Some(entry) = dir.next_entry().await? {
    let path = entry.path();
    if path.extension() != Some(OsStr::new("yaml")) {
      continue;
    }
    let stem = match path.file_stem().map(OsStr::to_str) {
      Some(Some(stem)) => stem,
      _ => continue,
    };
    let id = match Uuid::from_str(stem) {
      Ok(id) => id,
      _ => continue,
    };
    let yaml = read_to_string(&path).await?;
    f(id, yaml)?;
  }
  Ok(())
}
