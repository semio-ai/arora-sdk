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

fn status_identity(value: Option<Status>) -> Status {
  value.unwrap()
}

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

#[link(wasm_import_module = "env")]
extern "C" {
  fn arora_dispatch_indirect(callable_id: u64) -> i32;
}

// Implementation of `scoped`.
//========================================================================
struct CallOnDrop<F: FnMut() -> ()> {
  function: F,
}

impl<F: FnMut() -> ()> Drop for CallOnDrop<F> {
  fn drop(&mut self) {
    (self.function)()
  }
}

fn scoped<F: FnMut() -> ()>(function: F) -> CallOnDrop<F> {
  CallOnDrop { function }
}
