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