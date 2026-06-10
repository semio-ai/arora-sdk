use std::cell::RefCell;

#[no_mangle]
pub extern "C" fn arora_buffer_alloc(size: u32) -> *mut u8 {
    BUFFER_POOL.with(|pool_cell| {
        let mut pool = pool_cell.borrow_mut();
        pool.allocate(size)
    })
}

#[no_mangle]
pub extern "C" fn arora_buffer_free(buffer: *mut u8) {
    BUFFER_POOL.with(|pool_cell| {
        let mut pool = pool_cell.borrow_mut();
        pool.free(buffer)
    })
}

thread_local! {
  static BUFFER_POOL: RefCell<BufferPool> = RefCell::new(BufferPool::new(8, 256));
}

/// A pool of buffers, that are reused instead of being reallocated.
/// It grows whenever no buffer can be found to satisfy a given allocation.
struct BufferPool {
    buffers: Vec<Buffer>,
}

impl BufferPool {
    fn new(initial_count: u32, default_size: u32) -> Self {
        let buffers = vec![Buffer::new(default_size); initial_count as usize];
        Self { buffers }
    }

    /// Returns the pointer to a buffer of at least `size` bytes.
    /// If none can be found, a new one is created.
    fn allocate(&mut self, size: u32) -> *mut u8 {
        for buffer in &mut self.buffers {
            if !buffer.busy && buffer.data.len() >= size as usize {
                return buffer.data.as_mut_ptr();
            }
        }
        // if no fitting buffer was found, create one.
        self.buffers.push(Buffer::new(size));
        self.buffers.last_mut().unwrap().data.as_mut_ptr()
    }

    /// Looks up the buffer providing the given data pointer and sets it free (`busy = false`).
    fn free(&mut self, data_ptr: *mut u8) {
        for buffer in &mut self.buffers {
            if data_ptr == buffer.data.as_mut_ptr() {
                buffer.busy = false;
                break;
            }
        }
    }
}

/// A buffer of bytes, that can be set to `busy = true` in the context of the `BufferPool`.
#[derive(Clone)]
struct Buffer {
    data: Vec<u8>,
    busy: bool,
}

impl Buffer {
    fn new(size: u32) -> Self {
        Self {
            data: vec![0; size as usize],
            busy: false,
        }
    }
}
