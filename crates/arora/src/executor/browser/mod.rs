//! Browser-hosted Arora executor.
//!
//! Instantiates guest wasm modules via the browser's native
//! `WebAssembly` runtime (no wasmtime). Mirrors the wasmtime executor's
//! ABI: every guest module is expected to export `arora_buffer_alloc`,
//! `arora_buffer_free`, and `arora_function_<uuid_with_underscores>`
//! for each function declared in its `Header`, and to import
//! `env.arora_dispatch` / `env.arora_dispatch_indirect` for callbacks.
//!
//! Guest modules built for `wasm32-wasip1` also import a subset of
//! `wasi_snapshot_preview1`; we provide minimal stubs sufficient to
//! get past instantiation and Rust's startup.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use js_sys::{Function, Object, Reflect, Uint8Array, WebAssembly};
use uuid::Uuid;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use arora_buffers::serde_uuid::serialize as serialize_value;
use arora_types::module::low::ModuleDefinition;

use super::{Executor, LoadModuleError, UnloadModuleError};
use crate::call::{CallBridge, CallableId};
use crate::engine::{Engine, EngineRef};
use crate::module::{DispatchError, Module};

pub struct BrowserExecutor {
  engine: Option<EngineRef>,
}

impl BrowserExecutor {
  pub fn new() -> Self {
    Self { engine: None }
  }
}

impl Default for BrowserExecutor {
  fn default() -> Self {
    Self::new()
  }
}

impl Executor for BrowserExecutor {
  fn set_engine(&mut self, engine: EngineRef) {
    self.engine = Some(engine);
  }

  fn name(&self) -> &'static str {
    "wasm"
  }

  fn load_module(
    &mut self,
    module_definition: ModuleDefinition,
  ) -> Result<Box<dyn Module>, LoadModuleError> {
    let engine_ptr = self
      .engine
      .ok_or_else(|| LoadModuleError::Internal("BrowserExecutor: set_engine not called".into()))?;

    // Compile the module from bytes.
    let bytes_view = Uint8Array::from(module_definition.executable.as_ref());
    let module = WebAssembly::Module::new(&bytes_view.into())
      .map_err(|e| LoadModuleError::Internal(format!("WebAssembly.Module: {:?}", e)))?;

    // Late-bound view of the instance's memory + malloc, shared with
    // the dispatch closures (the instance does not exist yet at the
    // moment we have to declare them as imports).
    let late: Rc<RefCell<Option<LateBound>>> = Rc::new(RefCell::new(None));

    // env.arora_dispatch
    let late_d = late.clone();
    let dispatch_cb =
      Closure::<dyn FnMut(u32, u32, u32) -> u32>::new(move |module_id_ptr, method_id_ptr, arg_ptr| {
        let late = late_d.borrow();
        let late = late.as_ref().expect("late-bound state not set");
        // SAFETY: matches the wasmtime executor's engine-pointer trick.
        let engine = unsafe { &mut *(engine_ptr as *mut Engine) };
        let module_id = read_uuid(&late.memory, module_id_ptr);
        let method_id = read_uuid(&late.memory, method_id_ptr);
        let arg = read_arora_buffer(&late.memory, arg_ptr);
        let result = engine
          .dispatch(&module_id, &method_id, arg.as_ref())
          .expect("arora_dispatch: engine.dispatch failed");
        let result_addr = call_u32_u32(&late.malloc, result.len() as u32)
          .expect("arora_dispatch: malloc failed");
        write_bytes(&late.memory, result_addr, &result);
        result_addr
      });

    // env.arora_dispatch_indirect
    let late_di = late.clone();
    let dispatch_indirect_cb =
      Closure::<dyn Fn(i64) -> u32>::new(move |callable_id: i64| {
        let late = late_di.borrow();
        let late = late.as_ref().expect("late-bound state not set");
        let engine = unsafe { &mut *(engine_ptr as *mut Engine) };
        let value = engine
          .arora_call_indirect(&CallableId { id: callable_id as u64 })
          .expect("arora_dispatch_indirect: engine call failed");
        let buf = serialize_value(&value);
        let addr = call_u32_u32(&late.malloc, buf.len() as u32)
          .expect("arora_dispatch_indirect: malloc failed");
        write_bytes(&late.memory, addr, &buf);
        addr
      });

    // Build the import object.
    let imports = Object::new();
    let env = Object::new();
    Reflect::set(&env, &"arora_dispatch".into(), dispatch_cb.as_ref().unchecked_ref())
      .map_err(js_to_load_err)?;
    Reflect::set(
      &env,
      &"arora_dispatch_indirect".into(),
      dispatch_indirect_cb.as_ref().unchecked_ref(),
    )
    .map_err(js_to_load_err)?;
    Reflect::set(&imports, &"env".into(), &env).map_err(js_to_load_err)?;

    let (wasi_obj, wasi_keepalive) = build_wasi_stubs();
    Reflect::set(&imports, &"wasi_snapshot_preview1".into(), &wasi_obj)
      .map_err(js_to_load_err)?;

    // Instantiate.
    let instance = WebAssembly::Instance::new(&module, &imports)
      .map_err(|e| LoadModuleError::Internal(format!("WebAssembly.Instance: {:?}", e)))?;

    // Pull exports.
    let exports = instance.exports();
    let memory: WebAssembly::Memory = Reflect::get(&exports, &"memory".into())
      .map_err(js_to_load_err)?
      .dyn_into()
      .map_err(|_| LoadModuleError::Internal("guest does not export 'memory'".into()))?;
    let malloc: Function = Reflect::get(&exports, &"arora_buffer_alloc".into())
      .map_err(js_to_load_err)?
      .dyn_into()
      .map_err(|_| LoadModuleError::Internal("guest does not export 'arora_buffer_alloc'".into()))?;
    let free: Function = Reflect::get(&exports, &"arora_buffer_free".into())
      .map_err(js_to_load_err)?
      .dyn_into()
      .map_err(|_| LoadModuleError::Internal("guest does not export 'arora_buffer_free'".into()))?;

    let mut arora_functions = HashMap::new();
    for export in &module_definition.header.exports {
      let id = *export.id();
      let symbol = format!("arora_function_{}", id.to_string().replace('-', "_"));
      let f: Function = Reflect::get(&exports, &symbol.clone().into())
        .map_err(js_to_load_err)?
        .dyn_into()
        .map_err(|_| LoadModuleError::Internal(format!("guest missing export '{}'", symbol)))?;
      arora_functions.insert(id, f);
    }

    *late.borrow_mut() = Some(LateBound {
      memory: memory.clone(),
      malloc: malloc.clone(),
    });

    Ok(Box::new(BrowserModule {
      _instance: instance,
      memory,
      malloc,
      free,
      arora_functions,
      _dispatch_cb: dispatch_cb,
      _dispatch_indirect_cb: dispatch_indirect_cb,
      _wasi_keepalive: wasi_keepalive,
      _late: late,
    }))
  }

  fn unload_module(&mut self, _module_id: Uuid) -> Result<(), UnloadModuleError> {
    // The instance is dropped along with the BrowserModule held by the
    // engine; nothing else to do.
    Ok(())
  }
}

struct LateBound {
  memory: WebAssembly::Memory,
  malloc: Function,
}

struct BrowserModule {
  _instance: WebAssembly::Instance,
  memory: WebAssembly::Memory,
  malloc: Function,
  free: Function,
  arora_functions: HashMap<Uuid, Function>,
  // Closures must outlive the instance.
  _dispatch_cb: Closure<dyn FnMut(u32, u32, u32) -> u32>,
  _dispatch_indirect_cb: Closure<dyn Fn(i64) -> u32>,
  _wasi_keepalive: Vec<JsValue>,
  _late: Rc<RefCell<Option<LateBound>>>,
}

impl Module for BrowserModule {
  fn dispatch(&mut self, function_id: &Uuid, arg: &[u8]) -> Result<Box<[u8]>, DispatchError> {
    let arg_size = arg.len() as u32;

    let arg_addr = call_u32_u32(&self.malloc, arg_size).map_err(|e| DispatchError::Trap {
      message: format!("malloc({arg_size}) failed: {e:?}"),
    })?;
    write_bytes(&self.memory, arg_addr, arg);

    let func = self
      .arora_functions
      .get(function_id)
      .ok_or_else(|| DispatchError::Internal {
        message: format!("no exported function {}", function_id),
      })?;
    let result_addr = func
      .call1(&JsValue::NULL, &JsValue::from(arg_addr))
      .map_err(|e| DispatchError::Trap {
        message: format!("function call failed: {e:?}"),
      })?
      .as_f64()
      .ok_or_else(|| DispatchError::Internal {
        message: "function did not return a number".into(),
      })? as u32;

    // Free the input.
    self
      .free
      .call1(&JsValue::NULL, &JsValue::from(arg_addr))
      .map_err(|e| DispatchError::Trap {
        message: format!("free(arg) failed: {e:?}"),
      })?;

    // Read the size of the result (LE u32 at offset 0).
    let mut size_buf = [0u8; 4];
    read_bytes(&self.memory, result_addr, &mut size_buf);
    let size = u32::from_le_bytes(size_buf);

    // Read `size` bytes total (matching wasmtime executor's behavior).
    let mut result_buf = vec![0u8; size as usize];
    read_bytes(&self.memory, result_addr, &mut result_buf);

    self
      .free
      .call1(&JsValue::NULL, &JsValue::from(result_addr))
      .map_err(|e| DispatchError::Trap {
        message: format!("free(result) failed: {e:?}"),
      })?;

    Ok(result_buf.into_boxed_slice())
  }
}

// --- helpers -------------------------------------------------------------

fn js_to_load_err(e: JsValue) -> LoadModuleError {
  LoadModuleError::Internal(format!("js error: {e:?}"))
}

fn call_u32_u32(f: &Function, arg: u32) -> Result<u32, JsValue> {
  let res = f.call1(&JsValue::NULL, &JsValue::from(arg))?;
  Ok(res.as_f64().ok_or_else(|| JsValue::from_str("non-numeric return"))? as u32)
}

fn write_bytes(memory: &WebAssembly::Memory, offset: u32, bytes: &[u8]) {
  let view = Uint8Array::new(&memory.buffer());
  let src = Uint8Array::from(bytes);
  view.set(&src, offset);
}

fn read_bytes(memory: &WebAssembly::Memory, offset: u32, out: &mut [u8]) {
  let view = Uint8Array::new(&memory.buffer());
  let slice = view.subarray(offset, offset + out.len() as u32);
  slice.copy_to(out);
}

fn read_uuid(memory: &WebAssembly::Memory, offset: u32) -> Uuid {
  let mut buf = [0u8; 16];
  read_bytes(memory, offset, &mut buf);
  Uuid::from_slice(&buf).expect("16 bytes")
}

/// Reads an Arora buffer (u32 LE size header + payload). Matches
/// `AroraBuffer::read_wasm_memory` in the wasmtime path — the returned
/// Vec contains the size header followed by `size` payload bytes
/// (total `size + 4` bytes).
fn read_arora_buffer(memory: &WebAssembly::Memory, offset: u32) -> Vec<u8> {
  let mut size_buf = [0u8; 4];
  read_bytes(memory, offset, &mut size_buf);
  let size = u32::from_le_bytes(size_buf);
  let mut buf = vec![0u8; size as usize + 4];
  read_bytes(memory, offset, &mut buf);
  buf
}

/// Build a minimal `wasi_snapshot_preview1` stub object. Returns the
/// object plus a vector of `JsValue`s the caller must keep alive (the
/// underlying `Closure`s, retained as `JsValue` because their concrete
/// signatures differ).
fn build_wasi_stubs() -> (Object, Vec<JsValue>) {
  let obj = Object::new();
  let mut keepalive: Vec<JsValue> = Vec::new();

  // proc_exit(code) -> never. Throw to unwind.
  let proc_exit = Closure::<dyn FnMut(i32)>::new(|code: i32| {
    panic!("guest called proc_exit({code})");
  });
  Reflect::set(&obj, &"proc_exit".into(), proc_exit.as_ref().unchecked_ref()).unwrap();
  keepalive.push(proc_exit.into_js_value());

  // fd_write(fd, iovs, iovs_len, nwritten) -> errno. Stub returns 0
  // and writes 0 to `nwritten` so the guest believes the write
  // succeeded. We have no easy access to memory from here without
  // capturing the instance, which doesn't exist yet at import time;
  // for Phase 4 we accept silent stdout.
  let fd_write = Closure::<dyn FnMut(i32, i32, i32, i32) -> i32>::new(|_, _, _, _| 0);
  Reflect::set(&obj, &"fd_write".into(), fd_write.as_ref().unchecked_ref()).unwrap();
  keepalive.push(fd_write.into_js_value());

  let fd_close = Closure::<dyn FnMut(i32) -> i32>::new(|_| 0);
  Reflect::set(&obj, &"fd_close".into(), fd_close.as_ref().unchecked_ref()).unwrap();
  keepalive.push(fd_close.into_js_value());

  let fd_seek = Closure::<dyn FnMut(i32, i64, i32, i32) -> i32>::new(|_, _, _, _| 0);
  Reflect::set(&obj, &"fd_seek".into(), fd_seek.as_ref().unchecked_ref()).unwrap();
  keepalive.push(fd_seek.into_js_value());

  let fd_fdstat_get = Closure::<dyn FnMut(i32, i32) -> i32>::new(|_, _| 0);
  Reflect::set(&obj, &"fd_fdstat_get".into(), fd_fdstat_get.as_ref().unchecked_ref()).unwrap();
  keepalive.push(fd_fdstat_get.into_js_value());

  // 8 = WASI errno BADF — pretend no preopened dirs.
  let fd_prestat_get = Closure::<dyn FnMut(i32, i32) -> i32>::new(|_, _| 8);
  Reflect::set(&obj, &"fd_prestat_get".into(), fd_prestat_get.as_ref().unchecked_ref()).unwrap();
  keepalive.push(fd_prestat_get.into_js_value());

  let fd_prestat_dir_name = Closure::<dyn FnMut(i32, i32, i32) -> i32>::new(|_, _, _| 0);
  Reflect::set(
    &obj,
    &"fd_prestat_dir_name".into(),
    fd_prestat_dir_name.as_ref().unchecked_ref(),
  )
  .unwrap();
  keepalive.push(fd_prestat_dir_name.into_js_value());

  let environ_get = Closure::<dyn FnMut(i32, i32) -> i32>::new(|_, _| 0);
  Reflect::set(&obj, &"environ_get".into(), environ_get.as_ref().unchecked_ref()).unwrap();
  keepalive.push(environ_get.into_js_value());

  let environ_sizes_get = Closure::<dyn FnMut(i32, i32) -> i32>::new(|_, _| 0);
  Reflect::set(
    &obj,
    &"environ_sizes_get".into(),
    environ_sizes_get.as_ref().unchecked_ref(),
  )
  .unwrap();
  keepalive.push(environ_sizes_get.into_js_value());

  let args_get = Closure::<dyn FnMut(i32, i32) -> i32>::new(|_, _| 0);
  Reflect::set(&obj, &"args_get".into(), args_get.as_ref().unchecked_ref()).unwrap();
  keepalive.push(args_get.into_js_value());

  let args_sizes_get = Closure::<dyn FnMut(i32, i32) -> i32>::new(|_, _| 0);
  Reflect::set(
    &obj,
    &"args_sizes_get".into(),
    args_sizes_get.as_ref().unchecked_ref(),
  )
  .unwrap();
  keepalive.push(args_sizes_get.into_js_value());

  let clock_time_get = Closure::<dyn FnMut(i32, i64, i32) -> i32>::new(|_, _, _| 0);
  Reflect::set(
    &obj,
    &"clock_time_get".into(),
    clock_time_get.as_ref().unchecked_ref(),
  )
  .unwrap();
  keepalive.push(clock_time_get.into_js_value());

  let random_get = Closure::<dyn FnMut(i32, i32) -> i32>::new(|_, _| 0);
  Reflect::set(&obj, &"random_get".into(), random_get.as_ref().unchecked_ref()).unwrap();
  keepalive.push(random_get.into_js_value());

  (obj, keepalive)
}
