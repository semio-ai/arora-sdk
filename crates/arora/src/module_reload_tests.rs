//! Loading, reloading and unloading guest modules on a running device.
//!
//! The guests are small components written in the text format, each
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
        name: "wasm-component".to_string(),
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
/// argument.
fn guest(answers: &[(Uuid, Value)]) -> Vec<u8> {
    use crate::test_components::{component, Answer};
    let functions: Vec<(Uuid, Answer)> = answers
        .iter()
        .map(|(id, value)| {
            let result = arora_buffers::serde_uuid::serialize(&Value::Structure(Structure {
                id: *id,
                fields: vec![StructureField {
                    id: *id,
                    value: Box::new(value.clone()),
                }],
            }));
            (*id, Answer::Bytes(result.to_vec()))
        })
        .collect();
    component(&functions)
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

/// A guest under module id `id` whose one function, `function`, takes
/// `succeed`'s name and signature and answers `answer`.
fn other_module(id: Uuid, function: Uuid, answer: bool) -> CompiledModule {
    let mut header = header(&[SUCCEED], 1);
    header.id = id;
    header.name = "other".to_string();
    let low::ExportSymbol::Function(export) = &mut header.exports[0];
    export.id = function;
    export.name = "other_succeed".to_string();
    CompiledModule::new(header, &guest(&[(function, Value::Boolean(answer))]))
        .expect("the guest compiles")
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

    assert!(matches!(
        arora.reload_module(&broken),
        Err(ModuleError::Load { id, .. }) if id == module_id()
    ));
    assert_eq!(arora.call(call(SUCCEED)).unwrap().ret, Value::Boolean(true));
    assert_eq!(indexed(&arora), ["succeed"]);
}

/// Reloading and unloading need a guest module under the id; a host module
/// is not one.
#[test]
fn only_a_loaded_guest_module_reloads_or_unloads() {
    let mut arora = Arora::builder().build().expect("build the default device");
    assert!(matches!(
        arora.reload_module(&v1()),
        Err(ModuleError::NotLoaded { .. })
    ));
    assert!(matches!(
        arora.unload_module(arora_behavior::interpreter_module::ID),
        Err(ModuleError::NotLoaded { .. })
    ));
}

/// Unload, then load a new version under the same id: one module id, one
/// module, at every point.
#[test]
fn unload_then_load_round_trips() {
    let mut arora = device_with(&v1());

    assert!(matches!(
        arora.load_module(&v2()),
        Err(ModuleError::AlreadyLoaded { .. })
    ));

    arora.unload_module(module_id()).expect("unloads");
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

    arora.reload_module(&v2()).expect("reloads");
    arora.step(Duration::from_millis(10)).expect("step");
    assert_eq!(ok(), Some(Value::Boolean(false)));
    assert_eq!(*arora.behavior_error().borrow(), None);
}

/// Nothing checks a reload or an unload against what the behavior calls: a
/// tree calling a function the module no longer has keeps running, and the
/// node calling it fails each tick with the error a missing function gives.
#[test]
fn a_behavior_calling_what_a_reload_dropped_fails_its_node() {
    let mut arora = device_with(&v2());
    arora
        .load_groot(
            r#"<root main_tree_to_execute="MainTree"><BehaviorTree ID="MainTree">
                 <ping/>
               </BehaviorTree></root>"#,
        )
        .expect("the tree names ping");
    arora.step(Duration::from_millis(10)).expect("step");
    assert_eq!(*arora.behavior_error().borrow(), None);

    arora.reload_module(&v1()).expect("v1 does not export ping");
    arora
        .step(Duration::from_millis(10))
        .expect("the device keeps stepping");
    let error = arora
        .behavior_error()
        .borrow()
        .clone()
        .expect("the node fails");
    assert!(error.contains("FunctionNotFound"), "{error}");

    arora.unload_module(module_id()).expect("unloads");
    arora
        .step(Duration::from_millis(10))
        .expect("the device keeps stepping");
    let error = arora
        .behavior_error()
        .borrow()
        .clone()
        .expect("the node fails");
    assert!(error.contains("ModuleNotFound"), "{error}");
}

/// A task run calling the module is interrupted by a reload: halted, its
/// status key reports it ended. A run calling another module goes on.
#[test]
fn a_reload_interrupts_the_task_runs_calling_the_module() {
    let other = Uuid::from_u128(0xabc);
    let other_succeed = Uuid::from_u128(0xabd);
    let store = SimpleDataStore::new();
    let mut arora = Arora::builder()
        .with_data_store(Box::new(store.clone()))
        .with_compiled_module(&v2())
        .with_compiled_module(&other_module(other, other_succeed, true))
        .build()
        .expect("build");
    let spawn = |arora: &mut Arora, call: Call| {
        let spawned = arora.call(client::spawn(&call)).expect("the run starts");
        interpreter_module::decode_spawn_result(&spawned.ret).expect("a handle")
    };
    // `ping` returns no status: its run would end at its first tick. A run
    // ticks its call until it says it is done, so each of these is still
    // running when the module reloads, before any step.
    let reloaded = spawn(&mut arora, call(PING));
    let elsewhere = spawn(
        &mut arora,
        Call {
            module_id: Some(other),
            id: other_succeed,
            args: Vec::new(),
        },
    );

    arora.reload_module(&v2()).expect("reloads");
    {
        let runs = arora.task_runs.borrow();
        let run = |id| runs.iter().find(|run| run.id == id).expect("followed");
        assert!(run(reloaded.id).interrupted.is_some());
        assert!(run(elsewhere.id).interrupted.is_none());
    }

    arora.step(Duration::from_millis(10)).expect("step");
    let failure: Value = arora_behavior::Status::Failure.into();
    assert_eq!(
        store.read(std::slice::from_ref(&reloaded.status)),
        vec![Some(failure)],
        "the interrupted run ended"
    );
    let success: Value = arora_behavior::Status::Success.into();
    assert_eq!(
        store.read(std::slice::from_ref(&elsewhere.status)),
        vec![Some(success)],
        "the other run ran"
    );
}

/// A graph run taken over by a spawn of another graph with its root is
/// followed as calling what the new graph calls: a reload of a module only
/// the new graph calls interrupts it.
#[test]
fn a_reload_interrupts_a_run_taken_over_to_call_the_module() {
    use arora_behavior::graph::{Graph, Node};
    use arora_behavior_tree::nodes::PARALLEL_FUNCTION_ID;

    let other = Uuid::from_u128(0xabc);
    let other_succeed = Uuid::from_u128(0xabd);
    let mut arora = Arora::builder()
        .with_compiled_module(&v2())
        .with_compiled_module(&other_module(other, other_succeed, true))
        .build()
        .expect("build");
    // A parallel over one node calling `function`.
    let calling = |function: Uuid| {
        let [root, leaf] = [0x1, 0x2].map(Uuid::from_u128);
        let mut graph = Graph::empty();
        graph.root = Some(root);
        graph.nodes.insert(
            root,
            Node {
                id: root,
                function: PARALLEL_FUNCTION_ID,
                children: Some(vec![leaf]),
                ..Node::default()
            },
        );
        graph.nodes.insert(
            leaf,
            Node {
                id: leaf,
                function,
                ..Node::default()
            },
        );
        graph
    };
    let spawn = |arora: &mut Arora, graph: Graph| {
        let spawned = arora
            .call(interpreter_module::encode_spawn_graph(
                &arora_behavior_tree::behavior::graph_type(),
                &graph,
                arora_behavior::RunPolicy::Concurrent,
            ))
            .expect("the spawn is answered");
        interpreter_module::decode_spawn_result(&spawned.ret).expect("a handle")
    };
    let run = spawn(&mut arora, calling(other_succeed));
    let taken_over = spawn(&mut arora, calling(PING));
    assert_eq!(taken_over.id, run.id, "the spawn answered the live run");

    arora.reload_module(&v2()).expect("reloads");
    let runs = arora.task_runs.borrow();
    assert_eq!(runs.len(), 1);
    assert!(
        runs[0].interrupted.is_some(),
        "the run now calls the reloaded module"
    );
}

/// A call to the module the device received and has not applied yet fails
/// with the reload's reason; a call elsewhere is applied at the next step.
#[test]
fn a_reload_fails_the_pending_calls_to_the_module() {
    let mut arora = device_with(&v1());
    let caller = arora.caller();
    let to_module = caller.call(call(SUCCEED));
    let elsewhere = caller.call(interpreter_module::encode_load(
        &arora_behavior::Graph::empty(),
    ));

    arora.reload_module(&v2()).expect("reloads");
    arora.step(Duration::from_millis(10)).expect("step");

    let error = futures::executor::block_on(to_module).expect_err("interrupted");
    assert!(error.to_string().contains("was reloaded"), "{error}");
    futures::executor::block_on(elsewhere).expect("applied");
}

/// Two modules cannot share a function id: a call names its function by id.
#[test]
fn a_module_exporting_another_module_s_function_id_is_refused() {
    let mut arora = device_with(&v1());
    let mut other = header(&[SUCCEED], 1);
    other.id = Uuid::from_u128(0xabc);
    let impostor =
        CompiledModule::new(other, &guest(&[(SUCCEED, Value::Boolean(false))])).expect("compiles");
    assert!(matches!(
        arora.load_module(&impostor),
        Err(ModuleError::FunctionTaken { function, .. }) if function == SUCCEED
    ));
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
        name: "wasm-component".to_string(),
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
