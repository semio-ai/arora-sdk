//! Quantifies the per-call overhead of a *dynamic* module boundary, the kind
//! Arora pays on every `arora_dispatch` into a wasm guest, versus a static
//! Rust call. Mirrors the dominant costs of Arora's dispatch path:
//!   1. crossing the host<->guest WebAssembly boundary,
//!   2. reading/writing the guest's linear memory for the argument buffer,
//!   3. serialising args + result (Arora uses arora-buffers; bincode here is a
//!      faithful proxy of that cost class).
//! Run: `cargo run --release --bin wasm_dispatch`
use std::hint::black_box;
use std::time::Instant;
use wasmtime::*;

// A guest that (a) offers a pure exported function and (b) calls back into a
// host import N times, exactly the round-trip shape of arora_dispatch_indirect.
const WAT: &str = r#"
(module
  (import "env" "host_call" (func $host_call (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  ;; pure guest export: doubles its argument, no host crossing
  (func (export "pure") (param $x i32) (result i32)
    local.get $x i32.const 2 i32.mul)
  ;; calls the host import $iters times, accumulating the result
  (func (export "call_host_n") (param $iters i32) (result i32)
    (local $i i32) (local $acc i32)
    (loop $l
      (local.set $acc (i32.add (local.get $acc) (call $host_call (i32.const 0) (i32.const 16))))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br_if $l (i32.lt_s (local.get $i) (local.get $iters))))
    local.get $acc))
"#;

#[derive(bincode::Encode, bincode::Decode, Clone)]
struct Payload { a: f64, b: f64 } // 16 bytes, a representative tiny call arg

fn main() -> anyhow::Result<()> {
    let n: u64 = 2_000_000;

    // (1) Static Rust baseline: a plain function call doing the same trivial work.
    fn rust_work(x: i32) -> i32 { black_box(x).wrapping_mul(2) }
    let t = Instant::now();
    let mut acc = 0i32;
    for i in 0..n { acc = acc.wrapping_add(rust_work(i as i32)); }
    black_box(acc);
    let direct_ns = t.elapsed().as_nanos() as f64 / n as f64;

    // Wasm setup
    let engine = Engine::default();
    let module = Module::new(&engine, WAT)?;
    let mut store = Store::new(&engine, ());
    let mut linker = Linker::new(&engine);
    // host import: reads 16 bytes from guest memory (the "arg buffer") and returns.
    linker.func_wrap("env", "host_call",
        |mut caller: Caller<'_, ()>, ptr: i32, len: i32| -> i32 {
            let mem = caller.get_export("memory").and_then(|e| e.into_memory()).unwrap();
            let data = mem.data(&caller);
            let slice = &data[ptr as usize..(ptr as usize + len as usize)];
            black_box(slice[0] as i32) // touch the buffer like a real reader would
        })?;
    let instance = linker.instantiate(&mut store, &module)?;
    let pure = instance.get_typed_func::<i32, i32>(&mut store, "pure")?;
    let call_host_n = instance.get_typed_func::<i32, i32>(&mut store, "call_host_n")?;

    // (2) host -> guest pure exported call (no host import crossing)
    let t = Instant::now();
    let mut acc = 0i32;
    for i in 0..n { acc = acc.wrapping_add(pure.call(&mut store, i as i32)?); }
    black_box(acc);
    let guest_call_ns = t.elapsed().as_nanos() as f64 / n as f64;

    // (3) host -> guest -> host import round trip (one boundary each way + mem read)
    let iters = 2_000_000i32;
    let t = Instant::now();
    black_box(call_host_n.call(&mut store, iters)?);
    let roundtrip_ns = t.elapsed().as_nanos() as f64 / iters as f64;

    // (4) serialise + deserialise a 16-byte arg payload (Arora pays this per call)
    let cfg = bincode::config::standard();
    let p = Payload { a: 1.5, b: 2.5 };
    let t = Instant::now();
    let m = 2_000_000u64;
    for _ in 0..m {
        let bytes = bincode::encode_to_vec(black_box(&p), cfg).unwrap();
        let (d, _): (Payload, _) = bincode::decode_from_slice(&bytes, cfg).unwrap();
        black_box(d.a);
    }
    let serde_ns = t.elapsed().as_nanos() as f64 / m as f64;

    println!("RESULT direct_rust_ns={:.2} wasm_guest_call_ns={:.2} wasm_host_roundtrip_ns={:.2} bincode_roundtrip_ns={:.2}",
        direct_ns, guest_call_ns, roundtrip_ns, serde_ns);
    println!("MODEL arora_dynamic_call_min_ns ~= roundtrip + 2*bincode = {:.1}", roundtrip_ns + 2.0*serde_ns);
    Ok(())
}
