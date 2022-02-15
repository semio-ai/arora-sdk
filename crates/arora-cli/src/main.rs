use anyhow::bail;
use arora::{
  call::{Call, Caller},
  engine::EngineBuilder,
  schema::module::low::{Header, ModuleDefinition},
};
use arora_index::Index;
use arora_registry::Registry;
use clap::{Error, ErrorKind, Parser};
use tokio::{
  fs::{read_to_string, File},
  io::AsyncReadExt
};
use url::Url;

// Command-line arguments.
//=====================================================================
#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
  /// URI to the registry of existing types.
  #[clap(short, long)]
  pub registry_uri: Option<String>,
  
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

  let mut engine = EngineBuilder::new()
    .add_executor(arora::executor::wasm::WebAssemblyExecutor::new()?)
    .build();

  let mut index = Index::new();

  if args.header.len() != args.exe.len() {
    bail!(Error::raw(
      ErrorKind::WrongNumberOfValues,
      format!(
        "mismatching number of headers ({}) and executables ({}) provided",
        args.header.len(),
        args.exe.len())
      )
    );
  }

  let registry = args.registry_uri.map(|uri| {
    Registry::new_with_base_uri(Url::parse(&uri)
      .expect(format!("malformed registry URI: {}", uri).as_str()))
  });

  for i in 0..args.header.len() {
    let header_path = &args.header[i];
    let header: Header = serde_yaml::from_str(
      &read_to_string(header_path)
        .await
        .expect(format!("header file {} could not be read", header_path).as_str()),
    ).expect(format!("header file {} contains invalid yaml", header_path).as_str());

    for type_id in header.type_dependencies() {
      if index.find_type(&type_id).is_ok() {
        continue;
      }
      if let Some(ref registry) = registry {
        let ty = registry.get_type(&type_id).await?;
        index.add_type(ty);
      } else {
        bail!("header provided in {} depends on type {} which is unknown", header_path, type_id);
      }
    }

    let mut executable_file = File::open(&args.exe[i]).await?;
    let mut executable = Vec::new();
    executable_file.read_to_end(&mut executable).await?;
    let executable = executable.into_boxed_slice();
    
    index.add_module(&header)?;

    let module_name = header.name.clone();
    engine
    .load_module(ModuleDefinition {
      schema_version: 0,
      header,
      executable,
    })
    .expect(format!("failed to load module {}", module_name).as_str());
  }

  if let Some(call_yaml) = args.call {
    let call: Call = serde_yaml::from_str(&call_yaml)?;
    let function_id = call.id.clone();
    let function = index.find_function(&function_id)?;

    let start_time = if args.benchmark {
      Some(std::time::Instant::now())
    } else {
      None
    };

    let nof_iterations = args.repeat;
    for _i in 0..nof_iterations {
      let result = engine.arora_call(&function.module, call.clone())?;
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
