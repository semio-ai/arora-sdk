use std::{cell::Cell, cell::RefCell, collections::HashMap, convert::TryInto, ptr};

use super::{Executor, LoadModuleError, UnloadModuleError};
use crate::{
  call::{CallBridge, CallableId},
  engine::EngineRef,
  module::{DispatchError, Module},
};
use arora_buffers::serde_uuid::serialize;
use arora_buffers::{BufferWriter, BUFFER_SIZE_SIZE, TYPE_ERROR};
use tempfile::{tempdir, TempDir};
use uuid::Uuid;

pub struct NativeExecutor {
  engine: Option<EngineRef>,
  modules: HashMap<Uuid, Box<NativeModule>>,
}

impl NativeExecutor {
  pub fn new() -> Self {
    Self {
      engine: None,
      modules: HashMap::new(),
    }
  }
}

impl Executor for NativeExecutor {
  fn set_engine(&mut self, engine: EngineRef) {
    self.engine = Some(engine);
  }

  fn name(&self) -> &'static str {
    "native"
  }

  fn load_module(
    &mut self,
    module_definition: arora_types::module::low::ModuleDefinition,
  ) -> Result<Box<dyn Module>, LoadModuleError> {
    let engine = self.engine.ok_or_else(|| {
      LoadModuleError::Internal("native executor has no engine reference".to_string())
    })?;
    let module_name = module_definition.header.name.clone();
    let module_imports_host_functions = !module_definition.header.imports.is_empty();
    let tmp_dir = tempdir().map_err(|err| {
      LoadModuleError::Internal(format!(
        "failed put module in a temporary directory: {}",
        err
      ))
    })?;

    let tmp_file_path = tmp_dir.path().join(format!(
      "lib{}.{}",
      module_definition.header.name,
      native_library_extension()
    ));

    std::fs::write(&tmp_file_path, module_definition.executable).map_err(|err| {
      LoadModuleError::Internal(format!("failed to write module to file: {}", err))
    })?;

    let lib = unsafe {
      libloading::Library::new(tmp_file_path)
        .map_err(|err| LoadModuleError::Internal(format!("failed to load module: {}", err)))?
    };

    unsafe {
      match lib.get::<unsafe extern "C" fn(usize)>(b"arora_set_host_dispatcher") {
        Ok(set_dispatcher) => set_dispatcher(native_host_dispatch as *const () as usize),
        Err(err) if module_imports_host_functions => {
          return Err(LoadModuleError::Internal(format!(
            "native module {module_name} declares imports but does not export arora_set_host_dispatcher: {err}"
          )));
        }
        Err(_) => {}
      }

      if let Ok(set_dispatch_indirect) =
        lib.get::<unsafe extern "C" fn(usize)>(b"arora_set_host_dispatch_indirect")
      {
        set_dispatch_indirect(native_host_dispatch_indirect as *const () as usize);
      }
    }

    Ok(Box::new(NativeModule {
      lib,
      engine,
      _tmp_dir: tmp_dir,
    }))
  }

  fn unload_module(&mut self, module_id: uuid::Uuid) -> Result<(), super::UnloadModuleError> {
    if let Some(module) = self.modules.remove(&module_id) {
      module
        .lib
        .close()
        .map_err(|err| UnloadModuleError::Internal(format!("failed to unload module: {}", err)))
    } else {
      Err(UnloadModuleError::ModuleNotFound)
    }
  }
}

struct NativeModule {
  lib: libloading::Library,
  engine: EngineRef,
  _tmp_dir: TempDir,
}

thread_local! {
  static CURRENT_ENGINE: Cell<EngineRef> = Cell::new(ptr::null_mut());
  static IMPORT_RESULT_BUFFERS: RefCell<Vec<Box<[u8]>>> = RefCell::new(Vec::new());
}

struct NativeDispatchScope {
  previous_engine: EngineRef,
}

impl NativeDispatchScope {
  fn enter(engine: EngineRef) -> Self {
    let previous_engine = CURRENT_ENGINE.with(|current| current.replace(engine));
    IMPORT_RESULT_BUFFERS.with(|buffers| buffers.borrow_mut().clear());
    Self { previous_engine }
  }
}

impl Drop for NativeDispatchScope {
  fn drop(&mut self) {
    CURRENT_ENGINE.with(|current| current.set(self.previous_engine));
    IMPORT_RESULT_BUFFERS.with(|buffers| buffers.borrow_mut().clear());
  }
}

fn native_library_extension() -> &'static str {
  if cfg!(target_os = "macos") {
    "dylib"
  } else if cfg!(target_os = "windows") {
    "dll"
  } else {
    "so"
  }
}

unsafe extern "C" fn native_host_dispatch(
  module_id_ptr: usize,
  method_id_ptr: usize,
  arg_ptr: usize,
) -> usize {
  let buffer = match native_host_dispatch_inner(module_id_ptr, method_id_ptr, arg_ptr) {
    Ok(buffer) => buffer,
    Err(message) => native_error_buffer(message),
  };
  store_import_result_for_generated_native_reader(buffer)
}

fn native_host_dispatch_inner(
  module_id_ptr: usize,
  method_id_ptr: usize,
  arg_ptr: usize,
) -> Result<Box<[u8]>, String> {
  let module_id = uuid_from_raw_ptr(module_id_ptr)?;
  let method_id = uuid_from_raw_ptr(method_id_ptr)?;
  let arg = buffer_from_raw_ptr(arg_ptr)?;
  let engine = CURRENT_ENGINE.with(|current| current.get());
  if engine.is_null() {
    return Err("native host dispatch called outside native module dispatch".to_string());
  }
  unsafe { (&mut *engine).dispatch(&module_id, &method_id, arg) }
    .map_err(|error| format!("{error}"))
}

unsafe extern "C" fn native_host_dispatch_indirect(callable_id: u64) -> usize {
  let buffer = match native_host_dispatch_indirect_inner(callable_id) {
    Ok(buffer) => buffer,
    Err(message) => native_error_buffer(message),
  };
  store_import_result_for_generated_native_reader(buffer)
}

fn store_import_result_for_generated_native_reader(buffer: Box<[u8]>) -> usize {
  let buffer = pad_buffer_for_generated_native_reader(buffer.as_ref()).into_boxed_slice();
  IMPORT_RESULT_BUFFERS.with(|buffers| {
    let ptr = buffer.as_ptr() as usize;
    buffers.borrow_mut().push(buffer);
    ptr
  })
}

fn native_host_dispatch_indirect_inner(callable_id: u64) -> Result<Box<[u8]>, String> {
  let engine = CURRENT_ENGINE.with(|current| current.get());
  if engine.is_null() {
    return Err("native host indirect dispatch called outside native module dispatch".to_string());
  }
  let value = unsafe { (&mut *engine).arora_call_indirect(&CallableId { id: callable_id }) }
    .map_err(|error| format!("{error}"))?;
  Ok(serialize(&value))
}

fn uuid_from_raw_ptr(ptr: usize) -> Result<Uuid, String> {
  let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, 16) };
  Uuid::from_slice(bytes).map_err(|error| format!("invalid uuid pointer: {error}"))
}

fn buffer_from_raw_ptr(ptr: usize) -> Result<&'static [u8], String> {
  let ptr = ptr as *const u8;
  let size = native_buffer_size(ptr)?;
  Ok(unsafe { std::slice::from_raw_parts(ptr, size) })
}

fn native_buffer_size(ptr: *const u8) -> Result<usize, String> {
  let size_buf = unsafe { std::slice::from_raw_parts(ptr, BUFFER_SIZE_SIZE) };
  let size = u32::from_le_bytes(
    size_buf
      .try_into()
      .map_err(|_| "native buffer is too small for a size header".to_string())?,
  ) as usize;
  if size < BUFFER_SIZE_SIZE {
    return Err(format!(
      "native buffer size {size} is smaller than its header"
    ));
  }
  Ok(size)
}

fn native_error_buffer(message: String) -> Box<[u8]> {
  let mut writer = BufferWriter::new();
  writer.add_error(&message);
  writer.finalize()
}

fn pad_arg_for_generated_native_reader(arg: &[u8]) -> Vec<u8> {
  pad_buffer_for_generated_native_reader(arg)
}

fn pad_buffer_for_generated_native_reader(buffer: &[u8]) -> Vec<u8> {
  let mut padded = Vec::with_capacity(buffer.len() + BUFFER_SIZE_SIZE);
  padded.extend_from_slice(buffer);
  padded.resize(buffer.len() + BUFFER_SIZE_SIZE, 0);
  padded
}

impl Module for NativeModule {
  fn dispatch(&mut self, function_id: &uuid::Uuid, arg: &[u8]) -> Result<Box<[u8]>, DispatchError> {
    unsafe {
      let func: libloading::Symbol<unsafe extern "C" fn(usize) -> usize> = self
        .lib
        .get(
          format!(
            "arora_function_{}",
            function_id.to_string().replace('-', "_")
          )
          .as_bytes(),
        )
        .map_err(|err| DispatchError::Internal {
          message: format!("failed to get function {}: {}", function_id, err),
        })?;

      let arg = pad_arg_for_generated_native_reader(arg);
      let arg_address = arg.as_ptr() as usize;
      let _scope = NativeDispatchScope::enter(self.engine);
      let res_address = func(arg_address);
      let res_ptr = res_address as *mut u8;
      let size_buf = std::slice::from_raw_parts(res_ptr, 4);
      let size = u32::from_le_bytes(size_buf.try_into().unwrap());
      let res_buf: *const [u8] = std::slice::from_raw_parts(res_ptr, size as usize);
      let result = Box::from_raw(res_buf as *mut [u8]);
      if result.get(BUFFER_SIZE_SIZE) == Some(&TYPE_ERROR) {
        let msg_start = BUFFER_SIZE_SIZE + 1;
        let message = if result.len() >= msg_start + 4 {
          let len =
            u32::from_le_bytes(result[msg_start..msg_start + 4].try_into().unwrap()) as usize;
          let str_start = msg_start + 4;
          std::str::from_utf8(&result[str_start..str_start + len])
            .unwrap_or("<invalid utf-8>")
            .to_string()
        } else {
          "guest returned error (no message)".to_string()
        };
        return Err(DispatchError::Guest { message });
      }
      Ok(result)
    }
  }
}
