//! A guest's calls through the host hold one result at a time: each call's
//! buffer is read through the address it returns, and freed by the next call.
//! The test is alone in its binary, so the allocations it counts are its own.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

use arora_buffers::{BufferWriter, BUFFER_SIZE_SIZE, TYPE_ERROR};
use arora_module::__rt::keep_result;

/// The system allocator, counting the bytes allocated and not yet freed.
struct Counting;

static LIVE: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size() as isize, Ordering::SeqCst);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as isize, Ordering::SeqCst);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The size-prefixed buffer at `address`.
fn buffer_at<'a>(address: usize) -> &'a [u8] {
    unsafe {
        let size = u32::from_le_bytes(*(address as *const [u8; 4])) as usize;
        std::slice::from_raw_parts(address as *const u8, size)
    }
}

/// The message of the error buffer `buffer`: its type, then the message's
/// length and bytes.
fn error_message(buffer: &[u8]) -> Option<String> {
    let rest = &buffer[BUFFER_SIZE_SIZE..];
    let (&kind, rest) = rest.split_first()?;
    let length = u32::from_le_bytes(rest.get(..4)?.try_into().unwrap()) as usize;
    (kind == TYPE_ERROR).then(|| String::from_utf8(rest[4..4 + length].to_vec()).unwrap())
}

/// A finalized buffer of `count` u32s.
fn result(count: usize) -> Vec<u8> {
    let mut writer = BufferWriter::new();
    for _ in 0..count {
        writer.add_u32(7);
    }
    writer.finalize().into_vec()
}

#[test]
fn a_guest_holds_one_host_call_result_at_a_time() {
    // The result, and the host's error, read back through the address.
    let ok = result(3);
    assert_eq!(buffer_at(keep_result(Ok(ok.clone()))), &ok[..]);
    let error = keep_result(Err("refused".to_string()));
    assert_eq!(error_message(buffer_at(error)).as_deref(), Some("refused"));

    // A result that is not the Arora Buffer it starts with, read back as an
    // error: too short for its prefix, a prefix counting more than it holds
    // or less than itself, and a value cut short under a prefix counting it.
    let mut counts_more = result(3);
    counts_more.truncate(counts_more.len() - 1);
    let mut cut_short = result(3);
    cut_short.truncate(BUFFER_SIZE_SIZE + 3);
    let len = cut_short.len() as u32;
    cut_short[..4].copy_from_slice(&len.to_le_bytes());
    for malformed in [
        vec![],
        vec![8, 0, 0],
        vec![3, 0, 0, 0],
        counts_more,
        cut_short,
    ] {
        let address = keep_result(Ok(malformed.clone()));
        assert_eq!(
            error_message(buffer_at(address)).as_deref(),
            Some("the result is not an Arora Buffer"),
            "{malformed:?}"
        );
    }

    // However many calls the guest makes, it holds the latest result alone.
    let calls = 1000;
    let size = result(256).len() as isize;
    keep_result(Ok(result(256)));
    let before = LIVE.load(Ordering::SeqCst);
    for _ in 0..calls {
        keep_result(Ok(result(256)));
    }
    let grown = LIVE.load(Ordering::SeqCst) - before;
    assert!(
        grown < size,
        "{calls} calls holding {size}-byte results grew the live allocations by {grown} bytes"
    );
}
