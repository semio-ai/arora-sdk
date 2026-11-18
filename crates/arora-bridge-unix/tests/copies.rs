//! How many bytes a device allocates to take a large reading from its HAL to
//! a subscribed client: a camera frame is copied into the store's cell once,
//! when its size changes, and encoded once for the wire, and nowhere else on
//! the way. A test binary of its own, for its counting allocator.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use arora::Arora;
use arora_hal::{Hal, HalDescription, HalResult, UpdatesStream};
use arora_types::data::{Key, State, StateChange};
use arora_types::value::Value;
use async_trait::async_trait;
use futures::channel::mpsc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Counts every byte allocated, by any thread of the test.
struct Counting;

static ALLOCATED: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    /// A growing reallocation may copy, so it counts its new size; a
    /// shrinking one is done in place, and counts nothing.
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size > layout.size() {
            ALLOCATED.fetch_add(new_size, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// A HAL that reports the readings the test hands it, and takes no command.
struct Camera {
    readings: std::sync::Mutex<Option<mpsc::UnboundedReceiver<StateChange>>>,
}

#[async_trait]
impl Hal for Camera {
    async fn describe(&self) -> HalDescription {
        HalDescription::default()
    }
    async fn read(&self, keys: &[Key]) -> HalResult<Vec<Option<Value>>> {
        Ok(vec![None; keys.len()])
    }
    async fn read_all(&self) -> HalResult<State> {
        Ok(State::default())
    }
    async fn write(&self, _changes: StateChange) -> HalResult<()> {
        Ok(())
    }
    fn try_send(&self, _changes: &StateChange) {}
    fn updates(&self) -> UpdatesStream {
        Box::pin(self.readings.lock().unwrap().take().expect("one feed"))
    }
}

const FRAME: usize = 1 << 20;
const STEPS: usize = 20;

/// A step that takes a 1 MiB frame from the HAL to a client allocates two
/// copies of it, the store's cell and the encoded message, and no copy per
/// stage: none for the sensor phase, the store's notification, its subscriber
/// or the bridge.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_frame_is_copied_only_to_be_encoded() {
    let dir = std::env::temp_dir().join(format!("arora-bridge-unix-copies-{}", std::process::id()));
    let path = dir.join("device.sock");
    let (readings, feed) = mpsc::unbounded();
    let mut device = Arora::builder()
        .with_hal(Box::new(Camera {
            readings: std::sync::Mutex::new(Some(feed)),
        }))
        .with_bridge(Box::new(
            arora_bridge_unix::UnixBridge::bind(&path).expect("bind the bridge"),
        ))
        .build()
        .expect("build the device");

    // A client that reads into one buffer, so it allocates nothing per frame.
    let mut stream = tokio::net::UnixStream::connect(&path)
        .await
        .expect("connect");
    let subscribe =
        arora_bridge_unix::wire::encode_request(&arora_bridge_unix::wire::Request::Subscribe {
            id: 1,
            prefixes: vec!["camera".into()],
            latest: vec![],
        })
        .unwrap();
    stream.write_all(&subscribe).await.unwrap();
    let received = Arc::new(AtomicUsize::new(0));
    let reader = tokio::spawn({
        let received = received.clone();
        async move {
            let mut buffer = vec![0u8; 2 * FRAME];
            loop {
                let mut len = [0; 4];
                if stream.read_exact(&mut len).await.is_err() {
                    return;
                }
                let len = u32::from_le_bytes(len) as usize - 4;
                stream.read_exact(&mut buffer[..len]).await.unwrap();
                received.fetch_add(1, Ordering::SeqCst);
            }
        }
    });
    // The subscription's answer, then the device takes its `DataRequested`.
    while received.load(Ordering::SeqCst) < 1 {
        device.step(Duration::from_millis(10)).unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    let frames: Vec<StateChange> = (0..=STEPS)
        .map(|i| StateChange::set("camera/frame", Value::ArrayU8(vec![i as u8; FRAME])))
        .collect();
    let mut measured = 0;
    for (i, frame) in frames.into_iter().enumerate() {
        let start = ALLOCATED.load(Ordering::SeqCst);
        readings.unbounded_send(frame).unwrap();
        device.step(Duration::from_millis(10)).unwrap();
        while received.load(Ordering::SeqCst) < i + 2 {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        // The first frame sizes the store's cell; the others reuse it.
        if i > 0 {
            measured += ALLOCATED.load(Ordering::SeqCst) - start;
        }
    }
    reader.abort();
    let per_step = measured as f64 / STEPS as f64 / FRAME as f64;
    println!("{per_step:.2} frames allocated per step");
    assert!(
        per_step < 2.5,
        "{per_step:.2} frames allocated per step: a stage copies the frame"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
