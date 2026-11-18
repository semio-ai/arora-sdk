//! How many bytes a step allocates to take a large reading from the HAL to a
//! bridge: a camera frame is copied once, into the store's cell, and nowhere
//! else on the way. A test binary of its own, for its counting allocator.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arora::Arora;
use arora_bridge::{Bridge, BridgeResult, DeviceInfo, Inbound, InboundStream};
use arora_hal::{Hal, HalDescription, HalResult, UpdatesStream};
use arora_types::data::{Key, State, StateChange};
use arora_types::value::Value;
use async_trait::async_trait;
use futures::channel::mpsc;
use futures::StreamExt;

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
    readings: Mutex<Option<mpsc::UnboundedReceiver<StateChange>>>,
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

/// A bridge that asks for the device's data and keeps each change it is
/// shared, as a bridge handing changes to another task does.
struct Keeper {
    kept: Arc<Mutex<Vec<Arc<StateChange>>>>,
}

#[async_trait]
impl Bridge for Keeper {
    fn take_inbound(&mut self) -> InboundStream {
        futures::stream::iter([Inbound::DataRequested(true)])
            .chain(futures::stream::pending())
            .boxed()
    }
    fn try_send(&mut self, _change: &StateChange) {
        panic!("the runtime shares its change: try_send_shared");
    }
    fn try_send_shared(&mut self, change: &Arc<StateChange>) {
        self.kept.lock().unwrap().push(change.clone());
    }
    async fn get_device_info(&self) -> BridgeResult<Option<DeviceInfo>> {
        Ok(None)
    }
    async fn update_device_info(
        &self,
        info: Option<DeviceInfo>,
    ) -> BridgeResult<Option<DeviceInfo>> {
        Ok(info)
    }
}

const FRAME: usize = 1 << 20;
const STEPS: usize = 20;

/// A step that takes a 1 MiB frame from the HAL to a bridge allocates one copy
/// of it, the store's cell, and no copy per stage: none for the sensor phase,
/// the store's notification, its subscriber, the frame's change or the bridge.
#[test]
fn a_frame_is_copied_once_on_its_way_to_a_bridge() {
    let (readings, feed) = mpsc::unbounded();
    let kept = Arc::new(Mutex::new(Vec::new()));
    let mut device = Arora::builder()
        .with_hal(Box::new(Camera {
            readings: Mutex::new(Some(feed)),
        }))
        .with_bridge(Box::new(Keeper { kept: kept.clone() }))
        .build()
        .expect("build the device");
    // The device takes the bridge's `DataRequested`.
    device.step(Duration::from_millis(10)).unwrap();

    let frames: Vec<StateChange> = (0..=STEPS)
        .map(|i| StateChange::set("camera/frame", Value::ArrayU8(vec![i as u8; FRAME])))
        .collect();
    kept.lock().unwrap().reserve(2 * STEPS);
    let mut measured = 0;
    for (i, frame) in frames.into_iter().enumerate() {
        let start = ALLOCATED.load(Ordering::SeqCst);
        readings.unbounded_send(frame).unwrap();
        device.step(Duration::from_millis(10)).unwrap();
        if i > 0 {
            measured += ALLOCATED.load(Ordering::SeqCst) - start;
        }
    }
    let delivered = kept
        .lock()
        .unwrap()
        .iter()
        .filter(|change| change.set.contains_key(&Key::from("camera/frame")))
        .count();
    assert_eq!(delivered, STEPS + 1, "every frame reached the bridge");
    let per_step = measured as f64 / STEPS as f64 / FRAME as f64;
    println!("{per_step:.2} frames allocated per step");
    assert!(
        per_step < 1.5,
        "{per_step:.2} frames allocated per step: a stage copies the frame"
    );
}
