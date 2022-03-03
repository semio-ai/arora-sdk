mod arora_generated;

use arora_buffers::BufferReader;
use arora_generated::{status::Status, tick_id::TickId};

use crate::arora_generated::status;

fn succeed() -> Status {
  Status::Success
}

fn fail() -> Status {
  Status::Failure
}

fn run() -> Status {
  Status::Running
}

fn seq(children: Vec<TickId>) -> Status {
  for child in children {
    match call_tick_function(&child) {
      Status::Success => continue,
      Status::Failure => return Status::Failure,
      Status::Running => return Status::Running,
    }
  }
  Status::Success
}

fn fallback(children: Vec<TickId>) -> Status {
  for child in children {
    match call_tick_function(&child) {
      Status::Success => return Status::Success,
      Status::Failure => continue,
      Status::Running => return Status::Running,
    }
  }
  Status::Success
}

fn parallel(children: Vec<TickId>) -> Status {
  let mut status = Status::Success;
  for child in children {
    match call_tick_function(&child) {
      Status::Success => continue,
      Status::Failure => status = Status::Failure,
      Status::Running => status = Status::Running,
    }
  }
  status
}

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

#[link(wasm_import_module = "env")]
extern "C" {
  fn arora_dispatch_indirect(callable_id: u64) -> i32;
}
