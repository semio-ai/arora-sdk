// use crate::{engine::{Engine, EngineBuilder}, executor::wasm::WebAssemblyExecutor};

// #[no_mangle]
// pub extern "C" fn semio_engine_new() -> *mut Engine {

//   let wasm_executor = Box::new(WebAssemblyExecutor::new()
//     .expect("Failed to initialize WebAssembly executor"));
//   let engine = EngineBuilder::new()
//     .add_executor(wasm_executor)
//     .build();
//   Box::into_raw(Box::new(engine))
// }
