mod arora_generated;

use arora_buffers::BufferReader;
use arora_generated::{status::Status, tick_id::TickId};

use crate::arora_generated::status;

// To simulate statuses
//===============================================================
fn succeed() -> Status {
  Status::Success
}

fn fail() -> Status {
  Status::Failure
}

fn run() -> Status {
  Status::Running
}

fn status_identity(value: Option<Status>) -> Status {
  value.unwrap()
}

// Basic data-oriented action nodes
//==============================================================
fn store(storage: &mut Option<f32>, value: Option<f32>) -> Status {
  *storage = value;
  Status::Success
}

fn increase(storage: &mut Option<f32>, delta: Option<f32>) -> Status {
  let new_value = storage.unwrap() + delta.unwrap();
  *storage = Some(new_value);
  Status::Success
}

// Basic control nodes
//==============================================================
fn seq(children: Option<Vec<TickId>>) -> Status {
  for child in children.unwrap() {
    match call_tick_function(&child) {
      Status::Success => continue,
      Status::Failure => return Status::Failure,
      Status::Running => return Status::Running,
    }
  }
  Status::Success
}

fn seq_star(children_arg: Option<Vec<TickId>>, current_index_arg: &mut Option<u16>) -> Status {
  let mut current_index = current_index_arg.unwrap();
  let children = children_arg.unwrap();
  let mut status = Status::Success;
  for i in (current_index as usize)..children.len() {
    let child = &children[i];
    match call_tick_function(&child) {
      Status::Success => current_index += 1,
      Status::Failure => {
        status = Status::Failure;
        break;
      }
      Status::Running => {
        status = Status::Running;
        break;
      }
    }
  }

  if status != Status::Running {
    current_index = 0;
  }
  *current_index_arg = Some(current_index);
  status
}

fn fallback(children: Option<Vec<TickId>>) -> Status {
  for child in children.unwrap() {
    match call_tick_function(&child) {
      Status::Success => return Status::Success,
      Status::Failure => continue,
      Status::Running => return Status::Running,
    }
  }
  Status::Success
}

fn parallel(children: Option<Vec<TickId>>) -> Status {
  let mut success = true;
  let mut failure = false;
  for child in children.unwrap() {
    match call_tick_function(&child) {
      Status::Success => continue,
      Status::Failure => {
        success = false;
        failure = true;
      }
      Status::Running => {
        success = false;
      }
    }
  }
  if success {
    Status::Success
  } else if failure {
    Status::Failure
  } else {
    Status::Running
  }
}

// Calling tick functions through arora_call_indirect.
//========================================================================
fn call_tick_function(tick_id: &TickId) -> Status {
  let result_buffer_addr = unsafe { arora_dispatch_indirect(tick_id.callable_id) };
  let result_buffer_ptr = result_buffer_addr as *const u8;
  const BUFFER_SIZE_SIZE: usize = std::mem::size_of::<u32>();
  let input_size_bytes: &[u8; 4] =
    unsafe { std::slice::from_raw_parts(result_buffer_ptr, BUFFER_SIZE_SIZE) }
      .try_into()
      .expect("input is too small");
  let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
  let input =
    unsafe { std::slice::from_raw_parts(result_buffer_ptr, BUFFER_SIZE_SIZE + input_size) };
  let mut reader = BufferReader::new(&input);
  status::deserialize_from_reader(&mut reader, true).unwrap()
}

// Other functions
//================================================================
fn cos(angle: Option<f32>, res: &mut Option<f32>) -> Status {
  *res = Some(test_rust_wasm::cos(angle.unwrap()));
  Status::Success
}

// Imported functions
//================================================================
mod test_rust_wasm {
  use arora_buffers::{BufferReader, BufferWriter, TYPE_STRUCTURE, TYPE_F32};

  use crate::arora_dispatch;

  pub fn cos(angle: f32) -> f32 {
    let mut writer = BufferWriter::new();
    writer.begin_structure(TEST_RUST_WASM_COS_FUNCTION_ID.as_slice(), 1);
    writer.add_structure_field(TEST_RUST_WASM_COS_ANGLE_PARAM_ID.as_slice());
    writer.add_f32(angle);
    let arg = writer.finalize();

    let result_buffer_addr = unsafe {
      arora_dispatch(
        TEST_RUST_WASM_MODULE_ID.as_ptr() as i32,
        TEST_RUST_WASM_COS_FUNCTION_ID.as_ptr() as i32,
        arg.as_ptr() as i32,
      )
    };

    let result_buffer_ptr = result_buffer_addr as *const u8;
    const BUFFER_SIZE_SIZE: usize = std::mem::size_of::<u32>();
    let input_size_bytes: &[u8; 4] =
      unsafe { std::slice::from_raw_parts(result_buffer_ptr, BUFFER_SIZE_SIZE) }
        .try_into()
        .expect("input is too small");
    let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
    let input =
      unsafe { std::slice::from_raw_parts(result_buffer_ptr, BUFFER_SIZE_SIZE + input_size) };
    let mut reader = BufferReader::new(&input);
    let type_raw_id_opt = reader.next_type();
    assert!(!type_raw_id_opt.is_none());
    assert_eq!(type_raw_id_opt.unwrap(), TYPE_STRUCTURE);
    let (result_struct_id, result_field_count) = reader.get_structure();
    assert_eq!(result_struct_id, TEST_RUST_WASM_COS_FUNCTION_ID);
    assert_eq!(result_field_count, 1);
    let result_field_id = reader.get_structure_field();
    assert_eq!(result_field_id, TEST_RUST_WASM_COS_FUNCTION_ID);
    let type_raw_id_opt = reader.next_type();
    assert!(!type_raw_id_opt.is_none());
    assert_eq!(type_raw_id_opt.unwrap(), TYPE_F32);
    reader.get_f32()
  }

  const TEST_RUST_WASM_MODULE_ID: [u8; 16] = [
    0x66, 0x5d, 0x6e, 0xc9, 0x3f, 0xc9, 0x4c, 0xfa, 0x91, 0x00, 0x5c, 0x89, 0x64, 0xe9, 0x5a, 0xec,
  ];
  const TEST_RUST_WASM_COS_FUNCTION_ID: [u8; 16] = [
    0xc1, 0x37, 0x57, 0xcb, 0x23, 0x11, 0x4c, 0x93, 0xab, 0xcc, 0xcb, 0x12, 0xd6, 0xcb, 0xb8, 0x59,
  ];
  const TEST_RUST_WASM_COS_ANGLE_PARAM_ID: [u8; 16] = [
    0x6c, 0x2a, 0x15, 0x7c, 0x42, 0x35, 0x47, 0xb0, 0xbf, 0xf3, 0x1e, 0xee, 0xf3, 0xe5, 0x74, 0x7d,
  ];
}

#[link(wasm_import_module = "env")]
extern "C" {
  fn arora_dispatch(module_id: i32, method_id: i32, arg: i32) -> i32;
  fn arora_dispatch_indirect(callable_id: u64) -> i32;
}
