use arora::{
  actor::Actor,
  engine::{Engine, EngineBuilder},
  module::Dispatch,
  schema::module::low::{Header, ModuleDefinition},
};

use arora_buffers::{BufferReader, BufferPrinter};
use clap::Parser;

use tokio::{
  fs::{read_to_string, File},
  io::AsyncReadExt,
};
use uuid::Uuid;

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
  #[clap(short, long)]
  pub header: String,

  #[clap(short, long)]
  pub exe: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let args = Args::parse();

  let arora = EngineBuilder::new()
    .add_executor(arora::executor::wasm::WebAssemblyExecutor::new()?)
    .build()
    .spawn();

  let header: Header = serde_yaml::from_str(&read_to_string(args.header).await?)?; 

  let mut executable_file = File::open(args.exe).await?;

  let mut executable = Vec::new();
  executable_file.read_to_end(&mut executable).await?;
  let executable = executable.into_boxed_slice();

  let module = arora
    .load_module(ModuleDefinition {
      schema_version: 0,
      header,
      executable,
    })
    .await.expect("failed to load module");

  let mut writer = arora_buffers::BufferWriter::new();

  let method_id = Uuid::parse_str("07f5740c-ba4a-45af-8ec5-bedde5737e99").unwrap();

  writer.begin_structure(method_id.as_bytes(), 2);
  writer.add_structure_field(Uuid::parse_str("63086e48-804f-403a-8862-3358ddedc08d").unwrap().as_bytes());
  writer.add_s32(10);
  writer.add_structure_field(Uuid::parse_str("b41899c3-66dc-40d4-ab61-d1ccf5231c88").unwrap().as_bytes());
  writer.add_s32(20);
  let arg = writer.finalize().to_vec().into_boxed_slice();

  let start_time = std::time::Instant::now();
  let mut handles = Vec::new();
  let nof_iterations = 20000;
  for _i in 1..nof_iterations {
    let module = module.clone();
    let arg = arg.clone();
    handles.push(tokio::spawn(async move {
      module
        .dispatch(Dispatch {
          method_id,
          arg,
        })
        .await.expect("failed to dispatch")
    }));
  }

  futures::future::join_all(handles).await;

  let end_time = std::time::Instant::now();
  let total_duration = end_time - start_time;
  let duration = total_duration / nof_iterations;

  println!(
    "{:?} for {:?} iterations ({:?} per iteration)",
    total_duration, nof_iterations, duration
  );

  Ok(())
}
