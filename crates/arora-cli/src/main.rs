
use arora::{
  engine::EngineBuilder,
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

  let mut engine = EngineBuilder::new()
    .add_executor(arora::executor::wasm::WebAssemblyExecutor::new()?)
    .build();

  let header: Header = serde_yaml::from_str(
    &read_to_string(args.header).await
    .expect("header file could not be read")
  ).expect("header file contains invalid yaml"); 
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

  engine
    .load_module(ModuleDefinition {
      schema_version: 0,
      header: serde_yaml::from_str(&read_to_string("modules/test-cpp-2/arora/module.yaml").await
          .expect("second header file could not be read")
        ).expect("second header file could not be parsed"),
      executable: {
        let mut file = File::open("modules/test-cpp-2/test-cpp-2").await?;
        let mut executable = Vec::new();
        file.read_to_end(&mut executable).await?;
        executable.into_boxed_slice()
      },
    }).expect("failed to load module");


  let method_id = Uuid::parse_str("07f5740c-ba4a-45af-8ec5-bedde5737e99").unwrap();
  let b = Uuid::parse_str("63086e48-804f-403a-8862-3358ddedc08d").unwrap();
  let a = Uuid::parse_str("b41899c3-66dc-40d4-ab61-d1ccf5231c88").unwrap();

  let integer_array = Uuid::parse_str("5ffa9104-1e5c-4026-943f-8db38bd34563").unwrap();
  let status = Uuid::parse_str("7d94a956-e50d-4cc4-9714-f62e1f9b134e").unwrap();

  let status_enumeration = Uuid::parse_str("325a5767-e344-4532-860e-0749bcf2e428").unwrap();

  let success = Uuid::parse_str("766e9e9a-446d-4e46-83e6-14b7ca101169").unwrap();

  let arg = arora_buffers::Value::Structure(arora_buffers::Structure {
    id: method_id.as_bytes().as_slice().into(),
    fields: vec! [
      arora_buffers::StructureField {
        id: b.as_bytes().as_slice().into(),
        value: arora_buffers::Value::Structure(arora_buffers::Structure {
          id: a.as_bytes().as_slice().into(),
          fields: vec! [
            arora_buffers::StructureField {
              id: integer_array.as_bytes().as_slice().into(),
              value: arora_buffers::Value::S32(1),
            },
            arora_buffers::StructureField {
              id: status.as_bytes().as_slice().into(),
              value: arora_buffers::Value::Enumeration(arora_buffers::Enumeration {
                id: status_enumeration.as_bytes().as_slice().into(),
                variant_id: success.as_bytes().as_slice().into(),
                value: arora_buffers::Value::Unit.into(),
              })
            },
          ],
        }),
      },
      arora_buffers::StructureField {
        id: a.as_bytes().as_slice().into(),
        value: arora_buffers::Value::Enumeration(arora_buffers::Enumeration {
          id: status_enumeration.as_bytes().as_slice().into(),
          variant_id: success.as_bytes().as_slice().into(),
          value: arora_buffers::Value::Unit.into(),
        })
      }
    ]
  });

  let arg = arg.serialize();

  let start_time = std::time::Instant::now();
  let nof_iterations = 20;
  for _i in 1..nof_iterations {
    let raw_result = engine.dispatch(&module_id, &method_id, &arg)?;
    // let result = unsafe { Value::deserialize(&raw_result) };
    // println!("{:#?}", result);
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
