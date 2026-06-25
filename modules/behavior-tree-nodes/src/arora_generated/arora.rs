#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "env")]
extern "C" {
    pub fn arora_dispatch(module_id: usize, method_id: usize, arg: usize) -> usize;
    pub fn arora_dispatch_indirect(callable_id: u64) -> usize;
}
#[cfg(not(target_arch = "wasm32"))]
pub unsafe extern "C" fn arora_dispatch(
    _module_id: usize,
    _method_id: usize,
    _arg: usize,
) -> usize {
    panic ! ("arora_dispatch called on the host; this module is meant to run as a wasm guest under the arora engine");
}
#[cfg(not(target_arch = "wasm32"))]
pub unsafe extern "C" fn arora_dispatch_indirect(_callable_id: u64) -> usize {
    panic ! ("arora_dispatch_indirect called on the host; this module is meant to run as a wasm guest under the arora engine");
}
