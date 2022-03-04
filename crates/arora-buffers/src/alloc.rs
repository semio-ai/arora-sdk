#[no_mangle]
pub extern "C" fn arora_buffer_alloc(size: u32) -> *mut u8 {
  let vec = vec![0u8; size as usize];
  vec.leak().as_mut_ptr()
}

#[no_mangle]
pub extern "C" fn arora_buffer_free(buffer: *mut u8) {
  unsafe {
    let _ = Box::from_raw(buffer);
  }
}
