# arora-simple-data-store

The reference implementation of Arora's data store
([`arora_types::data::DataStore`](../arora-types/src/data/store.rs)): values
under keys, in memory, observable through change feeds. It is the store an
Arora device uses unless its builder is given another.

## SimpleDataStore

A `SimpleDataStore` maps keys — `/`-separated paths — to values. Its clones
share their storage, so a host keeps one handle and gives another to the
device:

```rust,ignore
use arora_simple_data_store::SimpleDataStore;
use arora_types::data::{DataStore, Key, StateChange};
use arora_types::value::Value;

let store = SimpleDataStore::new();
store.write(StateChange::set("face/mouth", Value::F64(0.5)))?;
assert_eq!(store.read(&[Key::new("face/mouth")]), vec![Some(Value::F64(0.5))]);

let handle = store.clone(); // the same storage
let feed = handle.subscribe();
let opening = feed.try_recv().unwrap(); // the current state: face/mouth = 0.5
assert!(opening.set.contains_key(&Key::new("face/mouth")));
```

It implements the whole trait:

- **Change feeds.** A subscription's first change is the current state of
  the keys it covers; every change written after it follows. A subscriber is
  sent only what changed: writing the value a key already holds notifies
  nothing. `subscribe_prefix` covers one subtree, and the store sends each
  such subscriber the keys under its prefix alone.
- **Key meta.** `set_meta` states a key's `KeyMeta` — its type, range, unit,
  rest value, description and whether a remote may write it — and
  `set_prefix_meta` states it for a subtree; `meta` answers the most specific
  statement. A key is closed to remote writers unless its meta opens it.
- **Slots.** `slot` resolves a key once into a direct handle to its value
  (`Slot`), which reads and writes what `read` and `write` do for that key.

## NamespacedStore

A `NamespacedStore` is a view of a shared store confined to one namespace: a
key `joint1.position` goes in as `<namespace>/joint1.position`, and the
view's snapshots, meta listings and change feeds hold the keys under its
namespace alone, stripped of it. Several devices built over views of one
store each see their own keys and none of their neighbours':

```rust,ignore
use std::sync::Arc;
use arora_simple_data_store::{NamespacedStore, SimpleDataStore};
use arora_types::data::DataStore;

let shared: Arc<dyn DataStore> = Arc::new(SimpleDataStore::new());
let bench_1 = NamespacedStore::new(shared.clone(), "bench-1");
let bench_2 = NamespacedStore::new(shared, "bench-2");
// arora::Arora::builder().with_data_store(Box::new(bench_1)) …
```

Part of the Arora device runtime, with [`arora`](../arora/readme.md).
