use engine::{
  actor::Actor,
  engine::{Engine, EngineBuilder},
  module::Dispatch,
  schema::module::low::{Header, ModuleDefinition},
};

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

  let engine = EngineBuilder::new()
    .add_executor(engine::executor::wasm::WebAssemblyExecutor::new()?)
    .build()
    .spawn();

  let header: Header = serde_yaml::from_str(&read_to_string(args.header).await?)?;

  let mut executable_file = File::open(args.exe).await?;

  let mut executable = Vec::new();
  executable_file.read_to_end(&mut executable).await?;
  let executable = executable.into_boxed_slice();

  let module = engine
    .load_module(ModuleDefinition {
      schema_version: 0,
      header,
      executable,
    })
    .await?;

  module
    .dispatch(Dispatch {
      method_id: Uuid::new_v4(),
    })
    .await?;

  Ok(())
}
