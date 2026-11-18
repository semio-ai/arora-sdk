//! A step that fails loses no change: what it drained from the store goes out
//! with the next step.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use arora::Arora;
use arora_hal::{Hal, HalDescription, HalResult, UpdatesStream};
use arora_simple_data_store::SimpleDataStore;
use arora_types::data::{
    DataError, DataStore, Key, KeyMeta, Slot, State, StateChange, Subscription,
};
use arora_types::value::Value;
use async_trait::async_trait;
use futures::channel::mpsc;

/// A HAL whose readings the test sends, and which keeps what it is sent.
struct Probe {
    readings: Mutex<Option<mpsc::UnboundedReceiver<StateChange>>>,
    sent: Arc<Mutex<Vec<StateChange>>>,
}

#[async_trait]
impl Hal for Probe {
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
    fn try_send(&self, changes: &StateChange) {
        self.sent.lock().unwrap().push(changes.clone());
    }
    fn updates(&self) -> UpdatesStream {
        Box::pin(self.readings.lock().unwrap().take().expect("one feed"))
    }
}

/// A store that refuses any write to `bad`.
#[derive(Clone)]
struct Refusing(SimpleDataStore);

impl DataStore for Refusing {
    fn read(&self, keys: &[Key]) -> Vec<Option<Value>> {
        self.0.read(keys)
    }
    fn write(&self, changes: StateChange) -> Result<(), DataError> {
        let bad = Key::from("bad");
        if changes.set.contains_key(&bad) || changes.unset.contains(&bad) {
            return Err(DataError::Other("bad is refused".into()));
        }
        self.0.write(changes)
    }
    fn snapshot(&self) -> State {
        self.0.snapshot()
    }
    fn slot(&self, key: &Key) -> Box<dyn Slot> {
        self.0.slot(key)
    }
    fn subscribe(&self) -> Subscription {
        self.0.subscribe()
    }
    fn meta(&self, keys: &[Key]) -> Vec<Option<KeyMeta>> {
        self.0.meta(keys)
    }
    fn set_meta(&self, meta: std::collections::HashMap<Key, KeyMeta>) -> Result<(), DataError> {
        self.0.set_meta(meta)
    }
    fn all_meta(&self) -> std::collections::HashMap<Key, KeyMeta> {
        self.0.all_meta()
    }
    fn set_prefix_meta(
        &self,
        meta: std::collections::HashMap<String, KeyMeta>,
    ) -> Result<(), DataError> {
        self.0.set_prefix_meta(meta)
    }
    fn clone_box(&self) -> Box<dyn DataStore> {
        Box::new(self.clone())
    }
}

/// A host's write drained by a step whose readings the store refused still
/// reaches the HAL, with the next step.
#[test]
fn a_change_drained_by_a_failed_step_goes_out_with_the_next() {
    let (readings, feed) = mpsc::unbounded();
    let sent = Arc::new(Mutex::new(Vec::new()));
    let store = Refusing(SimpleDataStore::new());
    let mut device = Arora::builder()
        .with_hal(Box::new(Probe {
            readings: Mutex::new(Some(feed)),
            sent: sent.clone(),
        }))
        .with_data_store(Box::new(store.clone()))
        .build()
        .expect("build the device");
    let step = |device: &mut Arora| device.step(Duration::from_millis(10));
    step(&mut device).expect("a first step");

    store
        .write(StateChange::set("arm/target", Value::F64(0.5)))
        .expect("the host's write");
    readings
        .unbounded_send(StateChange::set("bad", Value::F64(1.0)))
        .unwrap();
    sent.lock().unwrap().clear();
    assert!(step(&mut device).is_err(), "the store refuses the reading");
    step(&mut device).expect("the next step");

    let target = Key::from("arm/target");
    let reached = sent
        .lock()
        .unwrap()
        .iter()
        .any(|change| change.set.get(&target) == Some(&Some(Value::F64(0.5))));
    assert!(reached, "the host's write reaches the HAL");
}
