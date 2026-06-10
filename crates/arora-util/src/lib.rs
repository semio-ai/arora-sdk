// `arora_uuid_compare` is part of the `#[no_mangle] extern "C"` ABI consumed by
// WASM guests; it takes raw pointers by contract. Marking it `unsafe` would
// change the ABI surface for callers without making the FFI boundary safer.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use uuid::Uuid;

#[no_mangle]
pub extern "C" fn arora_uuid_compare(a: *const u8, b: *const u8) -> i32 {
    let a = unsafe { Uuid::from_slice(std::slice::from_raw_parts(a, 16)) }.unwrap();
    let b = unsafe { Uuid::from_slice(std::slice::from_raw_parts(b, 16)) }.unwrap();
    match a.cmp(&b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}
