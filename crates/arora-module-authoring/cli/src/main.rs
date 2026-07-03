mod generate;
use arora_registry::{
    local::LocalRegistry, local_yaml::load_records_from_yaml_dir, EditableRegistry,
    ReadableRegistry,
};
use arora_registry_remote::{config::check_and_update_config, remote_cached::RemoteCachedRegistry};
use arora_types::record::Resolver;
use clap::{Parser, Subcommand};
use generate::generate;
use reqwest::{
    header::{self, HeaderMap, HeaderValue},
    Client, Url,
};
use semio_client::{authentication::Config, context::Context};
use std::{fs::read_to_string, path::PathBuf, str::FromStr};

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
        help = "URL of the registry to use. Overrides and updates the configuration file if provided."
    )]
    registry_url: Option<String>,

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
        help = "Include records in the registry. It should be the path to a directory of records."
    )]
    include: Vec<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder().init();

    let args = Args::parse();

    let registry_url = if args.registry_url.is_some() {
        args.registry_url.to_owned()
    } else if let Some(config_path) = &args.config {
        let config = read_to_string(config_path)?;
        let config = serde_yaml::from_str::<Config>(&config)?;
        config.url
    } else {
        None
    };

    if let Some(registry_url) = registry_url {
        // Check config and args and update config if necessary,
        // while getting the updated token.
        let registry_url = Url::parse(registry_url.as_str())?;
        let token = check_and_update_config(
            &registry_url,
            args.config.to_owned(),
            args.user_name.to_owned(),
            args.password.to_owned(),
        )
        .await?;

        // Setup the context with the refreshed token.
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(token.as_str())?,
        );
        let client = Client::builder().default_headers(headers).build()?;
        let context = Context::new(registry_url, client);

        // Connect to the remote registry, and add records added locally.
        let mut registry = RemoteCachedRegistry::new(context);
        main_with_registry(args, &mut registry).await
    } else {
        let mut registry = LocalRegistry::new();
        main_with_registry(args, &mut registry).await
    }
}

async fn main_with_registry<R: ReadableRegistry + EditableRegistry + Resolver>(
    args: Args,
    registry: &mut R,
) -> anyhow::Result<()> {
    for include in &args.include {
        let include_path = PathBuf::from_str(include.as_str())?;
        if !include_path.exists() {
            eprintln!(
                "include path {} does not exist and is ignored",
                include_path.display()
            );
        }
        load_records_from_yaml_dir(include_path, registry).await?;
    }

    // Perform the command.
    match args.command {
        Commands::Generate(cmd) => {
            generate(cmd, registry).await?;
        }
    }

    Ok(())
}
