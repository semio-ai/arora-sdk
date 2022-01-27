use arora::{
  actor::Actor,
  engine::{Engine, EngineBuilder},
  schema::module::low::{Header, ModuleDefinition},
};

use arora_buffers::{BufferReader};
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

  let mut engine = EngineBuilder::new()
    .add_executor(arora::executor::wasm::WebAssemblyExecutor::new()?)
    .build();

  let header: Header = serde_yaml::from_str(&read_to_string(args.header).await?)?; 
  let module_id = header.id.clone();

  let mut executable_file = File::open(args.exe).await?;

  let mut executable = Vec::new();
  executable_file.read_to_end(&mut executable).await?;
  let executable = executable.into_boxed_slice();

  engine
    .load_module(ModuleDefinition {
      schema_version: 0,
      header,
      executable,
    }).expect("failed to load module");

  let mut writer = arora_buffers::BufferWriter::new();

  let method_id = Uuid::parse_str("07f5740c-ba4a-45af-8ec5-bedde5737e99").unwrap();
  let a = 20;
  let b = 10;

  writer.begin_structure(method_id.as_bytes(), 2);
  // We can set parameters in the order we like. Here we put b first.
  writer.add_structure_field(Uuid::parse_str("63086e48-804f-403a-8862-3358ddedc08d").unwrap().as_bytes());
  writer.add_s32(b);
  writer.add_structure_field(Uuid::parse_str("b41899c3-66dc-40d4-ab61-d1ccf5231c88").unwrap().as_bytes());
  writer.add_s32(a);
  let arg = writer.finalize().to_vec().into_boxed_slice();

  let start_time = std::time::Instant::now();
  let nof_iterations = 20000000;
  for _i in 1..nof_iterations {
    let raw_result = engine.dispatch(&module_id, &method_id, &arg)?;
    let mut reader = arora_buffers::BufferReader::new(&raw_result);
    let result = reader.get_s32();
    if result != a + b {
      panic!("bad result");
    }
  }


  let end_time = std::time::Instant::now();
  let total_duration = end_time - start_time;
  let duration = total_duration / nof_iterations;

  println!(
    "{:?} for {:?} iterations ({:?} per iteration)",
    total_duration, nof_iterations, duration
  );

  println!("{:?} calls per second", nof_iterations as f64 / total_duration.as_secs_f64());

  Ok(())
}
