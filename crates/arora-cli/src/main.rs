use std::collections::HashMap;

use anyhow::bail;
use arora::{
  engine::EngineBuilder,
  schema::module::low::{Header, ModuleDefinition},
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use tokio::{
  fs::{read_to_string, File},
  io::AsyncReadExt,
};
use uuid::Uuid;

// Value representation for received parameters.
//=====================================================================
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Value {
  #[serde(rename = "unit")]
  Unit,
  #[serde(rename = "book")]
  Boolean(bool),
  #[serde(rename = "u8")]
  U8(u8),
  #[serde(rename = "u16")]
  U16(u16),
  #[serde(rename = "u32")]
  U32(u32),
  #[serde(rename = "u64")]
  U64(u64),
  #[serde(rename = "s8")]
  S8(i8),
  #[serde(rename = "s16")]
  S16(i16),
  #[serde(rename = "s32")]
  S32(i32),
  #[serde(rename = "s64")]
  S64(i64),
  #[serde(rename = "f32")]
  R32(f32),
  #[serde(rename = "f64")]
  R64(f64),
  #[serde(rename = "str")]
  String(String),
  #[serde(rename = "struct")]
  Structure {
    id: Uuid,
    // #[serde(flatten)]
    fields: Vec<StructureField>,
  },
  #[serde(rename = "enum")]
  Enumeration {
    id: Uuid,
    variant_id: Uuid,
    value: Box<Value>,
  },
  #[serde(rename = "bool[]")]
  ArrayBoolean(Vec<bool>),
  #[serde(rename = "u8[]")]
  ArrayU8(Vec<u8>),
  #[serde(rename = "u16[]")]
  ArrayU16(Vec<u16>),
  #[serde(rename = "u32[]")]
  ArrayU32(Vec<u32>),
  #[serde(rename = "u64[]")]
  ArrayU64(Vec<u64>),
  #[serde(rename = "s8[]")]
  ArrayS8(Vec<i8>),
  #[serde(rename = "s16[]")]
  ArrayS16(Vec<i16>),
  #[serde(rename = "s32[]")]
  ArrayS32(Vec<i32>),
  #[serde(rename = "s64[]")]
  ArrayS64(Vec<i64>),
  #[serde(rename = "f32[]")]
  ArrayR32(Vec<f32>),
  #[serde(rename = "f64[]")]
  ArrayR64(Vec<f64>),
  #[serde(rename = "str[]")]
  ArrayString(Vec<String>),
  #[serde(rename = "struct[]")]
  ArrayStructure {
    id: Uuid,
    elements: Vec<StructureWithoutId>,
  },
  #[serde(rename = "enum[]")]
  ArrayEnumeration {
    id: Uuid,
    elements: Vec<EnumerationWithoutId>,
  },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StructureField {
  pub id: Uuid,
  pub value: Box<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StructureWithoutId {
  // #[serde(flatten)]
  pub fields: Vec<StructureField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnumerationWithoutId {
  pub variant_id: Uuid,
  pub value: Box<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Call {
  pub function: Uuid,
  #[serde(flatten)]
  pub args: HashMap<Uuid, Value>,
}

// Command-line arguments.
//=====================================================================
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
    &read_to_string(args.header)
      .await
      .expect("header file could not be read"),
  )
  .expect("header file contains invalid yaml");
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
    })
    .expect("failed to load module");

  engine
    .load_module(ModuleDefinition {
      schema_version: 0,
      header: serde_yaml::from_str(
        &read_to_string("modules/test-cpp-2/arora/module.yaml")
          .await
          .expect("second header file could not be read"),
      )
      .expect("second header file could not be parsed"),
      executable: {
        let mut file = File::open("modules/test-cpp-2/test-cpp-2").await?;
        let mut executable = Vec::new();
        file.read_to_end(&mut executable).await?;
        executable.into_boxed_slice()
      },
    })
    .expect("failed to load module");

  let method_id = Uuid::parse_str("07f5740c-ba4a-45af-8ec5-bedde5737e99").unwrap();
  let b = Uuid::parse_str("63086e48-804f-403a-8862-3358ddedc08d").unwrap();
  let a = Uuid::parse_str("b41899c3-66dc-40d4-ab61-d1ccf5231c88").unwrap();

  let integer_array = Uuid::parse_str("5ffa9104-1e5c-4026-943f-8db38bd34563").unwrap();
  let status = Uuid::parse_str("7d94a956-e50d-4cc4-9714-f62e1f9b134e").unwrap();

  let status_enumeration = Uuid::parse_str("325a5767-e344-4532-860e-0749bcf2e428").unwrap();

  let success = Uuid::parse_str("766e9e9a-446d-4e46-83e6-14b7ca101169").unwrap();

  let arg = arora_buffers::Value::Structure(arora_buffers::Structure {
    id: method_id.as_bytes().as_slice().into(),
    fields: vec![
      arora_buffers::StructureField {
        id: b.as_bytes().as_slice().into(),
        value: arora_buffers::Value::Structure(arora_buffers::Structure {
          id: a.as_bytes().as_slice().into(),
          fields: vec![
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
              }),
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
        }),
      },
    ],
  });

  let arg = arg.serialize();

  let start_time = std::time::Instant::now();
  let nof_iterations = 20;
  for _i in 1..nof_iterations {
    /*let raw_result =*/ engine.dispatch(&module_id, &method_id, &arg)?;
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

  println!(
    "{:?} calls per second",
    nof_iterations as f64 / total_duration.as_secs_f64()
  );

  Ok(())
}

// Tests.
//=====================================================================
#[cfg(test)]
mod tests {
  use super::*;
  use anyhow::Result;
  use std::str::FromStr;

  #[test]
  pub fn parse_call_test() -> Result<()> {
    let call: Call = serde_yaml::from_str(CALL_TEST)?;
    assert_eq!(call.function, Uuid::from_str("07f5740c-ba4a-45af-8ec5-bedde5737e99")?);
    if let Value::Structure { id, fields } = &call.args[&Uuid::from_str("63086e48-804f-403a-8862-3358ddedc08d")?] {
      assert_eq!(*id, Uuid::from_str("7f9aedf8-dbde-4020-b5f4-c28a6635ae7c")?);
      if let Value::S32(v) = fields[1].value.as_ref() {
        assert_eq!(*v, 113);
      } else {
        bail!("expected s32 value under second field of struct arg");
      }
    } else {
      bail!("expected a string under arg 55dbec70-1c3a-433e-a6e6-27446b7f065e");
    }
    Ok(())
  }

  #[test]
  pub fn parse_call_test_2() -> Result<()> {
    let call: Call = serde_yaml::from_str(CALL_TEST_2)?;
    assert_eq!(call.function, Uuid::from_str("b213a552-77ad-465a-a26d-352e8eccfd63")?);
    assert_eq!(call.args[&Uuid::from_str("55dbec70-1c3a-433e-a6e6-27446b7f065e")?], Value::U32(42));
    assert_eq!(call.args[&Uuid::from_str("abf9ca4e-e03f-431a-a32b-4911f809c399")?], Value::U32(64));
    Ok(())
  }

  pub const CALL_TEST: &'static str = "\
function: 07f5740c-ba4a-45af-8ec5-bedde5737e99
b41899c3-66dc-40d4-ab61-d1ccf5231c88:
  enum:
    id: 325a5767-e344-4532-860e-0749bcf2e428
    variant_id: 766e9e9a-446d-4e46-83e6-14b7ca101169
    value: unit
63086e48-804f-403a-8862-3358ddedc08d:
  struct:
    id: 7f9aedf8-dbde-4020-b5f4-c28a6635ae7c
    fields:
      - id: 7d94a956-e50d-4cc4-9714-f62e1f9b134e
        value:
          enum[]:
            id: 325a5767-e344-4532-860e-0749bcf2e428
            elements:
              - variant_id: 2468f46c-bb60-425c-9a4d-9ad326ccc7e2
                value: unit
      - id: 5ffa9104-1e5c-4026-943f-8db38bd34563
        value:
          s32: 113
";

  pub const CALL_TEST_2: &'static str = "\
function: b213a552-77ad-465a-a26d-352e8eccfd63
55dbec70-1c3a-433e-a6e6-27446b7f065e:
  u32: 42
abf9ca4e-e03f-431a-a32b-4911f809c399:
  u32: 64
";
}
