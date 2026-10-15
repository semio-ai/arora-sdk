//! Loading, reloading and unloading guest modules on a running device.
//!
//! The guests are small WebAssembly modules written in the text format, each
//! answering every function it exports with a fixed value: two versions of one
//! module id that answer differently tell which version a call reached.

use super::*;
use arora_bridge::client;
use arora_types::call::{Call, CallError};
use arora_types::value::{Structure, StructureField, Value};
use std::time::Duration;

/// `succeed() -> bool`, as the test-rust-wasm declaration pins it.
const SUCCEED: Uuid = Uuid::from_u128(0x00cd31a8_2cf4_48e6_a957_69a55de90424);
/// `ping() -> ()`, as the test-rust-wasm declaration pins it.
const PING: Uuid = Uuid::from_u128(0x5f423ba9_d5f9_46d7_a9b5_fb7d28f99ea6);

/// A header under the test-rust-wasm module id exporting `functions` of its
/// declaration, at `minor`.
fn header(functions: &[Uuid], minor: u32) -> Header {
    let mut header = test_rust_wasm::test_rust_wasm::header(low::Executor {
        name: "wasm".to_string(),
        min_version: None,
        max_version: None,
    });
    header
        .exports
        .retain(|export| functions.contains(export.id()));
    header.version.minor = minor;
    header
}

/// A guest answering each function of `answers` with its value, whatever the
/// argument: a bump allocator that every free resets, and each function's
/// result buffer laid out ahead in a data segment.
fn guest(answers: &[(Uuid, Value)]) -> Vec<u8> {
    let mut data = String::new();
    let mut functions = String::new();
    let mut offset = 16usize;
    for (id, value) in answers {
        let result = arora_buffers::serde_uuid::serialize(&Value::Structure(Structure {
            id: *id,
            fields: vec![StructureField {
                id: *id,
                value: Box::new(value.clone()),
            }],
        }));
        let bytes: String = result.iter().map(|b| format!("\\{b:02x}")).collect();
        data += &format!("(data (i32.const {offset}) \"{bytes}\")\n");
        functions += &format!(
            "(func (export \"arora_function_{}\") (param i32) (result i32) i32.const {offset})\n",
            id.to_string().replace('-', "_")
        );
        offset += result.len().next_multiple_of(8);
    }
    format!(
        r#"(module
             (memory (export "memory") 1)
             (global $next (mut i32) (i32.const 4096))
             (func (export "arora_buffer_alloc") (param $size i32) (result i32)
               (local $at i32)
               global.get $next
               local.set $at
               global.get $next
               local.get $size
               i32.add
               global.set $next
               local.get $at)
             (func (export "arora_buffer_free") (param i32)
               i32.const 4096
               global.set $next)
             {data}
             {functions})"#
    )
    .into_bytes()
}

fn compiled(functions: &[(Uuid, Value)], minor: u32) -> CompiledModule {
    let ids: Vec<Uuid> = functions.iter().map(|(id, _)| *id).collect();
    CompiledModule::new(header(&ids, minor), &guest(functions)).expect("the guest compiles")
}

fn v1() -> CompiledModule {
    compiled(&[(SUCCEED, Value::Boolean(true))], 1)
}

fn v2() -> CompiledModule {
    compiled(&[(SUCCEED, Value::Boolean(false)), (PING, Value::Unit)], 2)
}

fn module_id() -> Uuid {
    test_rust_wasm::test_rust_wasm::ids::MODULE
}

fn call(function: Uuid) -> Call {
    Call {
        module_id: Some(module_id()),
        id: function,
        args: Vec::new(),
    }
}

fn indexed(arora: &Arora) -> Vec<String> {
    let mut names: Vec<String> = arora
        .function_index
        .borrow()
        .values()
        .filter(|function| function.module_id == module_id())
        .map(|function| function.function_name.clone())
        .collect();
    names.sort();
    names
}

fn device_with(module: &CompiledModule) -> Arora {
    Arora::builder()
        .with_compiled_module(module)
        .build()
        .expect("build a device with the guest")
}

/// A reload swaps the code a call reaches and the method index's entries for
/// the module, in one step.
#[test]
fn a_reload_serves_the_new_version_and_describes_it() {
    let mut arora = device_with(&v1());
    assert_eq!(arora.call(call(SUCCEED)).unwrap().ret, Value::Boolean(true));
    assert_eq!(indexed(&arora), ["succeed"]);
    assert!(matches!(
        arora.call(call(PING)),
        Err(CallError::FunctionNotFound { .. })
    ));

    arora.reload_module(&v2()).expect("the new version loads");

    assert_eq!(
        arora.call(call(SUCCEED)).unwrap().ret,
        Value::Boolean(false)
    );
    assert_eq!(arora.call(call(PING)).unwrap().ret, Value::Unit);
    assert_eq!(indexed(&arora), ["ping", "succeed"]);

    // And back: the function v1 does not export leaves the index and fails
    // as a function the module does not have.
    arora
        .reload_module(&v1())
        .expect("the old version loads again");
    assert_eq!(indexed(&arora), ["succeed"]);
    assert!(matches!(
        arora.call(call(PING)),
        Err(CallError::FunctionNotFound { .. })
    ));
}

/// A new version that does not load leaves the old one loaded, serving and
/// described.
#[test]
fn a_reload_that_fails_keeps_the_old_version() {
    let mut arora = device_with(&v1());
    // Declares `ping`, which the guest does not export: instantiating fails.
    let broken = CompiledModule::new(
        header(&[SUCCEED, PING], 3),
        &guest(&[(SUCCEED, Value::Boolean(false))]),
    )
    .expect("the guest compiles");

    let error = arora.reload_module(&broken).unwrap_err().to_string();
    assert!(error.contains("failed to reload"), "{error}");

    assert_eq!(arora.call(call(SUCCEED)).unwrap().ret, Value::Boolean(true));
    assert_eq!(indexed(&arora), ["succeed"]);
}

/// Reloading needs a guest module under the id; a host module is not one.
#[test]
fn only_a_loaded_guest_module_reloads() {
    let mut arora = Arora::builder().build().expect("build the default device");
    let error = arora.reload_module(&v1()).unwrap_err().to_string();
    assert!(error.contains("no guest module"), "{error}");

    let error = arora
        .unload_module(arora_behavior::interpreter_module::ID)
        .unwrap_err()
        .to_string();
    assert!(error.contains("no guest module"), "{error}");
}

/// Unload, then load a new version under the same id: one module id, one
/// module, at every point.
#[test]
fn unload_then_load_round_trips() {
    let mut arora = device_with(&v1());

    let error = arora.load_module(&v2()).unwrap_err().to_string();
    assert!(error.contains("already loaded"), "{error}");

    arora.unload_module(module_id()).expect("nothing calls it");
    assert!(indexed(&arora).is_empty());
    assert!(matches!(
        arora.call(call(SUCCEED)),
        Err(CallError::ModuleNotFound { .. })
    ));

    arora.load_module(&v2()).expect("the id is free again");
    assert_eq!(
        arora.call(call(SUCCEED)).unwrap().ret,
        Value::Boolean(false)
    );
    assert_eq!(indexed(&arora), ["ping", "succeed"]);
}

/// A running behavior calling the module keeps running across a reload, and
/// its next tick reaches the new version.
#[test]
fn a_behavior_calls_the_new_version_after_a_reload() {
    let store = SimpleDataStore::new();
    let mut arora = Arora::builder()
        .with_data_store(Box::new(store.clone()))
        .with_compiled_module(&v1())
        .build()
        .expect("build");
    arora
        .load_groot(
            r#"<root main_tree_to_execute="MainTree"><BehaviorTree ID="MainTree">
                 <succeed res="{ok}"/>
               </BehaviorTree></root>"#,
        )
        .expect("the tree names succeed");
    let ok = || store.read(&[Key::from("ok")]).remove(0);

    arora.step(Duration::from_millis(10)).expect("step");
    assert_eq!(ok(), Some(Value::Boolean(true)));

    arora
        .reload_module(&v2())
        .expect("v2 still exports succeed");
    arora.step(Duration::from_millis(10)).expect("step");
    assert_eq!(ok(), Some(Value::Boolean(false)));
    assert_eq!(*arora.behavior_error().borrow(), None);
}

/// What the running behavior calls is neither unloaded nor dropped by a
/// reload; once nothing calls it, it goes.
#[test]
fn a_function_the_behavior_calls_stays() {
    let mut arora = device_with(&v2());
    arora
        .load_groot(
            r#"<root main_tree_to_execute="MainTree"><BehaviorTree ID="MainTree">
                 <ping/>
               </BehaviorTree></root>"#,
        )
        .expect("the tree names ping");

    let error = arora.unload_module(module_id()).unwrap_err().to_string();
    assert!(error.contains("'ping'"), "{error}");
    let error = arora.reload_module(&v1()).unwrap_err().to_string();
    assert!(error.contains("'ping'"), "{error}");
    // Refused: v2 still serves and is still described.
    assert_eq!(indexed(&arora), ["ping", "succeed"]);
    assert_eq!(arora.call(call(PING)).unwrap().ret, Value::Unit);

    // A behavior that calls only `succeed` lets `ping` go.
    arora
        .load_groot(
            r#"<root main_tree_to_execute="MainTree"><BehaviorTree ID="MainTree">
                 <succeed/>
               </BehaviorTree></root>"#,
        )
        .expect("the tree names succeed");
    arora.reload_module(&v1()).expect("succeed stays");
    let error = arora.unload_module(module_id()).unwrap_err().to_string();
    assert!(error.contains("'succeed'"), "{error}");
}

/// A task run calling the module holds it like the main behavior does, until
/// it is halted.
#[test]
fn a_task_run_holds_the_function_it_calls() {
    let mut arora = device_with(&v2());
    let spawned = arora
        .call(client::spawn(&call(PING)))
        .expect("the run starts");
    let handle = interpreter_module::decode_spawn_result(&spawned.ret).expect("a handle");

    let error = arora.reload_module(&v1()).unwrap_err().to_string();
    assert!(error.contains("'ping'"), "{error}");

    arora
        .call(interpreter_module::encode_halt(handle.id))
        .expect("halted");
    // The halt lands at the next tick; the finished run leaves at the next edit.
    arora.step(Duration::from_millis(10)).expect("step");
    arora
        .call(interpreter_module::encode_load(
            &arora_behavior::Graph::empty(),
        ))
        .expect("an edit prunes the finished run");
    arora
        .reload_module(&v1())
        .expect("nothing calls ping any more");
}

/// Two modules cannot share a function id: a call names its function by id.
#[test]
fn a_module_exporting_another_module_s_function_id_is_refused() {
    let mut arora = device_with(&v1());
    let mut other = header(&[SUCCEED], 1);
    other.id = Uuid::from_u128(0xabc);
    let impostor =
        CompiledModule::new(other, &guest(&[(SUCCEED, Value::Boolean(false))])).expect("compiles");
    let error = arora.load_module(&impostor).unwrap_err().to_string();
    assert!(error.contains("which module"), "{error}");
    assert_eq!(arora.call(call(SUCCEED)).unwrap().ret, Value::Boolean(true));
}

/// Instructions retired and memory held by this process, from the kernel.
#[cfg(target_os = "macos")]
fn usage() -> (u64, u64, u64) {
    let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    let status = unsafe {
        libc::proc_pid_rusage(
            std::process::id() as i32,
            libc::RUSAGE_INFO_V4,
            &mut info as *mut _ as *mut libc::rusage_info_t,
        )
    };
    assert_eq!(status, 0, "proc_pid_rusage");
    (
        info.ri_instructions,
        info.ri_resident_size,
        info.ri_phys_footprint,
    )
}

/// Cost and leaks of `cycles` reloads (and unload/load pairs) of the
/// test-rust-wasm guest — a real Rust guest with its allocator and data —
/// measured in instructions retired and memory held. Run alone, with output:
/// `cargo test -p arora --lib module_reload_tests::measure -- --ignored --nocapture --test-threads=1`.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "a measurement, not a check: run it alone"]
fn measure() {
    const WASM: &[u8] = include_bytes!(env!("CARGO_CDYLIB_FILE_TEST_RUST_WASM_test_rust_wasm"));
    let header = test_rust_wasm::test_rust_wasm::header(low::Executor {
        name: "wasm".to_string(),
        min_version: None,
        max_version: None,
    });
    let module = CompiledModule::new(header.clone(), WASM).expect("compiles");
    let mut arora = device_with(&module);
    let text = |arora: &mut Arora| {
        // Touch the guest's allocator, so each instance's memory is used.
        let call = Call {
            module_id: Some(header.id),
            id: Uuid::from_u128(0x431fb111_d750_4470_b1c8_94b0ad8c6e6b),
            args: vec![StructureField {
                id: Uuid::from_u128(0x64351cbc_d84b_4df3_bd52_fd23edd943cf),
                value: Box::new(Value::U32(4096)),
            }],
        };
        arora.call(call).expect("text");
    };

    let mib = |bytes: u64| bytes as f64 / (1024.0 * 1024.0);
    let report = |label: &str, cycles: u64, before: (u64, u64, u64), after: (u64, u64, u64)| {
        println!(
            "{label:<40} {cycles:>6} cycles: {:>12} instructions/cycle, resident {:>8.1} -> {:>8.1} MiB, footprint {:>8.1} -> {:>8.1} MiB",
            (after.0 - before.0) / cycles.max(1),
            mib(before.1),
            mib(after.1),
            mib(before.2),
            mib(after.2),
        );
    };

    // Baseline: the call alone.
    let before = usage();
    for _ in 0..1000 {
        text(&mut arora);
    }
    report("call (baseline)", 1000, before, usage());

    for round in 0..3 {
        let before = usage();
        for _ in 0..1000 {
            arora.reload_module(&module).expect("reload");
            text(&mut arora);
        }
        report(
            &format!("reload compiled + call, round {round}"),
            1000,
            before,
            usage(),
        );
    }

    let before = usage();
    for _ in 0..1000 {
        arora.unload_module(header.id).expect("unload");
        arora.load_module(&module).expect("load");
        text(&mut arora);
    }
    report("unload + load compiled + call", 1000, before, usage());

    for round in 0..2 {
        let before = usage();
        for _ in 0..100 {
            let fresh = CompiledModule::new(header.clone(), WASM).expect("compiles");
            arora.reload_module(&fresh).expect("reload");
            text(&mut arora);
        }
        report(
            &format!("compile + reload + call, round {round}"),
            100,
            before,
            usage(),
        );
    }
}
