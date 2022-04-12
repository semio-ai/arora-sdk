mod generate;
use arora_registry::remote::RemoteRegistry;
use clap::{Parser, Subcommand};
use generate::generate;
use reqwest::Client;
use semio_client::context::Context;

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
  #[clap(short, long)]
  registry_uri: Option<String>,

  #[clap(subcommand)]
  command: Commands,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  env_logger::builder().init();

  let args = Args::parse();

  let url = args
    .registry_uri
    .unwrap_or("http://localhost:8080".to_string());
  let context = Context {
    url: url.to_string(),
    client: Client::builder().build()?,
  };
  let mut registry = RemoteRegistry::new(context);

  match args.command {
    Commands::Generate(cmd) => {
      generate(cmd, &mut registry).await?;
    }
  }

  Ok(())
}
