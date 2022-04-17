mod generate;
use arora_registry::{
  config::check_and_update_config, local_yaml::load_entities_from_yaml_dir,
  remote_cached::RemoteCachedRegistry,
};
use clap::{Parser, Subcommand};
use generate::generate;
use reqwest::{
  header::{self, HeaderValue},
  Client, Url,
};
use semio_client::context::Context;
use std::{path::PathBuf, str::FromStr};

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
  let registry_url = Url::parse(args.registry_url.as_str())?;
  let token =
    check_and_update_config(&registry_url, args.config, args.user_name, args.password).await?;

  // Setup the context with the refreshed token.
  let mut headers = header::HeaderMap::new();
  headers.insert(
    header::AUTHORIZATION,
    HeaderValue::from_str(token.as_str())?,
  );
  let client = Client::builder().default_headers(headers).build()?;
  let context = Context::new(registry_url, client);

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
    load_entities_from_yaml_dir(include_path, &mut registry).await?;
  }

  // Perform the command.
  match args.command {
    Commands::Generate(cmd) => {
      generate(cmd, &mut registry).await?;
    }
  }

  Ok(())
}
