//! Can a "slice of the shared data storage" be made directly accessible across
//! the Arora module boundary, zero-copy, in a cross-language setup? This probes
//! the three executors Arora actually has:
//!   1. NATIVE (cdylib via libloading): one address space -> a raw pointer to a
//!      host struct can be shared and mutated in place (this is Copper's case).
//!   2. WASM (wasmtime) / BROWSER (js WebAssembly): isolated linear memory, BUT
//!      one linear memory can be IMPORTED into several module instances, so they
//!      address the same bytes -> zero-copy *across modules* (single-threaded).
//!   3. The fallback copy: even without sharing, a raw memcpy of a slice into a
//!      guest's linear memory beats serialising it (no serde framing).
//! Run: `cargo run --release --bin shared_data`
use std::hint::black_box;
use std::time::Instant;
use wasmtime::*;

// ---------- (1) NATIVE: zero-copy shared slice via a stable C ABI ----------
#[repr(C)]
struct DataSlice { a: f64, b: f64, n: u32 } // the layout arora-buffers would pin

// Mimics a native module receiving a *pointer* into the host data store and
// mutating it in place. No copy, no serialisation. (C++ would see the same
// struct through an arora-buffers-generated header.)
extern "C" fn native_module_process(p: *mut DataSlice) {
    unsafe { (*p).a += 1.0; (*p).n = (*p).n.wrapping_add(1); }
}

// ---------- (3) serialise cost for comparison ----------
#[derive(bincode::Encode, bincode::Decode, Clone)]
struct Payload64 { v: [f64; 8] } // 64 bytes

const WAT: &str = r#"
(module
  (import "env" "memory" (memory 1))
  (func (export "write") (param $off i32) (param $val i32)
    (i32.store (local.get $off) (local.get $val)))
  (func (export "read") (param $off i32) (result i32)
    (i32.load (local.get $off))))
"#;

fn main() -> anyhow::Result<()> {
    let n: u64 = 5_000_000;

    // (1) native zero-copy pointer share
    let mut slice = DataSlice { a: 0.0, b: 1.0, n: 0 };
    let p: *mut DataSlice = &mut slice;
    let t = Instant::now();
    for _ in 0..n { native_module_process(black_box(p)); }
    let native_ns = t.elapsed().as_nanos() as f64 / n as f64;
    assert_eq!(slice.n, n as u32);

    // (2) WASM: one linear memory imported into TWO instances -> shared, zero-copy
    let engine = Engine::default();
    let module = Module::new(&engine, WAT)?;
    let mut store = Store::new(&engine, ());
    let memory = Memory::new(&mut store, MemoryType::new(1, None))?;
    let mut linker = Linker::new(&engine);
    linker.define(&mut store, "env", "memory", memory)?;
    let inst_a = linker.instantiate(&mut store, &module)?;
    let inst_b = linker.instantiate(&mut store, &module)?;
    let write_a = inst_a.get_typed_func::<(i32, i32), ()>(&mut store, "write")?;
    let read_b = inst_b.get_typed_func::<i32, i32>(&mut store, "read")?;
    // module A writes at offset 64; module B reads the SAME bytes
    write_a.call(&mut store, (64, 0xC0FFEE))?;
    let shared = read_b.call(&mut store, 64)?;
    let cross_module_shared_ok = shared == 0xC0FFEE;

    // host reads a slice straight out of guest linear memory (no serde)
    let view = &memory.data(&store)[64..68];
    black_box(view[0]);

    // (3) raw memcpy of 64 bytes into linear memory + read-back vs bincode roundtrip
    let bytes = [7u8; 64];
    let t = Instant::now();
    for _ in 0..n {
        memory.write(&mut store, 256, black_box(&bytes))?;
        let mut out = [0u8; 64];
        memory.read(&store, 256, &mut out)?;
        black_box(out[0]);
    }
    let memcpy_ns = t.elapsed().as_nanos() as f64 / n as f64;

    let cfg = bincode::config::standard();
    let pay = Payload64 { v: [1.0; 8] };
    let t = Instant::now();
    for _ in 0..n {
        let b = bincode::encode_to_vec(black_box(&pay), cfg).unwrap();
        let (d, _): (Payload64, _) = bincode::decode_from_slice(&b, cfg).unwrap();
        black_box(d.v[0]);
    }
    let serde_ns = t.elapsed().as_nanos() as f64 / n as f64;

    println!("RESULT native_zero_copy_ns={:.2} cross_module_shared_ok={} \
wasm_memcpy64_ns={:.2} bincode64_ns={:.2}",
        native_ns, cross_module_shared_ok, memcpy_ns, serde_ns);
    Ok(())
}
