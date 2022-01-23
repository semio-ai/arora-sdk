// use crate::{arora::{arora, aroraBuilder}, executor::wasm::WebAssemblyExecutor};

// #[no_mangle]
// pub extern "C" fn semio_arora_new() -> *mut arora {

//   let wasm_executor = Box::new(WebAssemblyExecutor::new()
//     .expect("Failed to initialize WebAssembly executor"));
//   let arora = aroraBuilder::new()
//     .add_executor(wasm_executor)
//     .build();
//   Box::into_raw(Box::new(arora))
// }
