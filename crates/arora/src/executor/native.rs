use std::{collections::HashMap, convert::TryInto};

use super::{Executor, LoadModuleError, UnloadModuleError};
use crate::{
  engine::EngineRef,
  module::{DispatchError, Module},
};
use tempfile::tempdir;
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
    let tmp_dir = tempdir().map_err(|err| {
      LoadModuleError::Internal(format!(
        "failed put module in a temporary directory: {}",
        err
      ))
    })?;

    let tmp_file_path = tmp_dir
      .path()
      .join(format!("{}.bin", module_definition.header.name));

    std::fs::write(&tmp_file_path, module_definition.executable).map_err(|err| {
      LoadModuleError::Internal(format!("failed to write module to file: {}", err))
    })?;

    let lib = unsafe {
      libloading::Library::new(tmp_file_path)
        .map_err(|err| LoadModuleError::Internal(format!("failed to load module: {}", err)))?
    };

    Ok(Box::new(NativeModule { lib }))
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

      let arg_address = arg.as_ptr() as usize;
      let res_address = func(arg_address);
      let res_ptr = res_address as *mut u8;
      let size_buf = std::slice::from_raw_parts(res_ptr, 4);
      let size = u32::from_be_bytes(size_buf.try_into().unwrap());
      let res_buf: *const [u8] = std::slice::from_raw_parts(res_ptr, size as usize);
      Ok(Box::from_raw(res_buf as *mut [u8]))
    }
  }
}
