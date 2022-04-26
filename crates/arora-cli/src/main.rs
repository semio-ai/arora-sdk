use anyhow::bail;
use arora::{
  call::{Call, CallBridge},
  engine::EngineBuilder,
  schema::module::low::{Header, ModuleDefinition},
};
use arora_module_core::header::module_frozen_from_header_file;
use arora_registry::{
  config::check_and_update_config, local::LocalRegistry, local_yaml::load_records_from_yaml_dir,
  remote_cached::RemoteCachedRegistry, EditableRegistry, ReadableRegistry, RegistryError,
};
use clap::{Error, ErrorKind, Parser};
use reqwest::{
  header::{self, HeaderMap, HeaderValue},
  Client,
};
use semio_client::{authentication::Config, context::Context};
use semio_record::module::v0::frozen::ExportKind;
use semio_record::record::Freezer;
use std::{
  borrow::BorrowMut, collections::HashMap, fs::read_to_string, path::PathBuf, str::FromStr,
};
use tokio::{fs::File, io::AsyncReadExt};
use url::Url;

// Command-line arguments.
//=====================================================================
#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
  #[clap(
    long,
    help = "Path to a semio-cli configuration file to reuse and potentially update.
    If absent and no registry URL is provided, a local registry will be used."
  )]
  config: Option<String>,

  #[clap(
    short,
    long,
    help = "URL of the registry to use.
    If absent and no configuration file is provided, a local registry will be used."
  )]
  registry_url: Option<String>,

  #[clap(
    short,
    long,
    name = "user-name",
    help = "User name to authenticate with.
    Overrides and updates the configuration file if provided.
    Ignored if no registry URL is provided."
  )]
  user_name: Option<String>,

  #[clap(
    short,
    long,
    help = "Password to authenticate with.
    Updates the configuration file if provided.
    Ignored if no registry URL is provided."
  )]
  password: Option<String>,

  #[clap(
    short,
    long,
    help = "Include records in the registry.
    It should be the path to a directory of records."
  )]
  include: Vec<String>,

  /// Headers of modules to load. Order must match --exe arguments.
  #[clap(short, long)]
  pub header: Vec<String>,

  /// Binaries of modules to load. Order must match --header arguments.
  #[clap(short, long)]
  pub exe: Vec<String>,

  /// If set, performs a call described in yaml.
  #[clap(short, long)]
  pub call: Option<String>,

  /// Measure time taken to perform the tasks, and print them.
  #[clap(short, long)]
  pub benchmark: bool,

  /// Number of times to perform a call. Still performs the call is set to 0. Ignored if --call is not set.
  #[clap(short = 'n', long, default_value = "1")]
  pub repeat: u32,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let args = Args::parse();

  if args.header.len() != args.exe.len() {
    bail!(Error::raw(
      ErrorKind::WrongNumberOfValues,
      format!(
        "mismatching number of headers ({}) and executables ({}) provided",
        args.header.len(),
        args.exe.len()
      )
    ));
  }

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

async fn main_with_registry<R: ReadableRegistry + EditableRegistry + Freezer>(
  args: Args,
  registry: &mut R,
) -> anyhow::Result<()> {
  // Add records manually included to the registry.
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

  let mut functions_modules = HashMap::new();
  let mut engine = EngineBuilder::new()
    .add_executor(arora::executor::wasm::WebAssemblyExecutor::new()?)
    .build();
  for i in 0..args.header.len() {
    // Read the header.
    let header_path = &args.header[i];
    let header: Header = serde_yaml::from_str(
      &read_to_string(header_path)
        .expect(format!("header file {} could not be read", header_path).as_str()),
    )
    .expect(format!("header file {} contains invalid yaml", header_path).as_str());
    let (module_id, module_version, module_and_imports) =
      module_frozen_from_header_file(header_path, registry.borrow_mut()).await?;

    // Remember the module ID for each function ID.
    for (export_id, export) in &module_and_imports.module.exports {
      match export.kind {
        ExportKind::Function(_) => functions_modules.insert(export_id.clone(), module_id.clone()),
      };
    }

    // Add it to the registry.
    // It might be already brought by the includes, but we don't care.
    match registry
      .add_module(module_id, module_version, module_and_imports.module)
      .await
    {
      Ok(_) | Err(RegistryError::DuplicateSelector { selector: _ }) => {}
      Err(e) => bail!(e),
    }

    // Load the executable in the engine.
    let mut executable_file = File::open(&args.exe[i]).await?;
    let mut executable = Vec::new();
    executable_file.read_to_end(&mut executable).await?;
    let executable = executable.into_boxed_slice();

    let module_name = header.name.clone();
    engine
      .load_module(ModuleDefinition {
        schema_version: 0,
        header,
        executable,
      })
      .expect(format!("failed to load module {}", module_name).as_str());
  }

  if let Some(call_yaml) = &args.call {
    let call: Call = serde_yaml::from_str(&call_yaml)?;
    let function_id = call.id.clone();
    let module_id = if let Some(module_id) = &call.module_id {
      module_id
    } else {
      functions_modules
        .get(&function_id)
        .expect(format!("no such function {}", function_id).as_str())
    };

    let start_time = if args.benchmark {
      Some(std::time::Instant::now())
    } else {
      None
    };

    let nof_iterations = args.repeat;
    for _i in 0..nof_iterations {
      let result = engine.arora_call(&module_id, call.clone())?;
      println!("{}", serde_yaml::to_string(&result)?);
    }

    if args.benchmark {
      let end_time = std::time::Instant::now();
      let total_duration = end_time - start_time.unwrap();
      let duration = total_duration / nof_iterations;

      println!(
        "{:?} for {:?} calls ({:?} per call)",
        total_duration, nof_iterations, duration
      );

      println!(
        "{:?} calls per second",
        nof_iterations as f64 / total_duration.as_secs_f64()
      );
    }
  }

  Ok(())
}
