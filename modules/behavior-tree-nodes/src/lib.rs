mod arora_generated;

use crate::arora_generated::{
  arora::arora_dispatch_indirect,
  behavior_tree::{status, status::Status, tick_id::TickId},
  test_rust_wasm,
};
use arora_buffers::BufferReader;
use regex::Regex;

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
fn set_str(variable: &mut Option<String>, value: Option<String>) -> Status {
  *variable = value;
  Status::Success
}

fn unset_str(variable: &mut Option<String>) -> Status {
  *variable = None;
  Status::Success
}

fn is_str_set(value: Option<String>) -> Status {
  if value.is_some() && !value.unwrap().is_empty() {
    Status::Success
  } else {
    Status::Failure
  }
}

fn wait_str_set(value: Option<String>) -> Status {
  if value.is_some() && !value.unwrap().is_empty() {
    Status::Success
  } else {
    Status::Running
  }
}

fn regex_match(
  value: Option<String>,
  matcher: Option<String>,
  first_match: &mut Option<String>,
) -> Status {
  let value = match value {
    Some(value) => value,
    None => return Status::Failure,
  };
  let matcher = match matcher {
    Some(matcher) => matcher,
    None => return Status::Failure,
  };
  let re = match Regex::new(matcher.as_str()) {
    Ok(re) => re,
    Err(_) => return Status::Failure,
  };
  match re.captures(value.as_str()) {
    Some(captures) => {
      if captures.len() == 0 {
        *first_match = Some(String::new());
      } else {
        *first_match = Some(captures[0].to_string());
      }
      Status::Success
    }
    None => Status::Failure,
  }
}

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
  let children = children.unwrap();
  if children.len() == 0 {
    return Status::Success;
  }
  for child in children {
    match call_tick_function(&child) {
      Status::Success => return Status::Success,
      Status::Failure => continue,
      Status::Running => return Status::Running,
    }
  }
  Status::Failure
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
