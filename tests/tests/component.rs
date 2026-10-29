//! Engine-level tests for the component-model executor: loads the
//! test-rust-component guest (a wasm32-wasip2 component implementing the
//! `arora:module` WIT world) and dispatches through it, including a
//! host callback via `host.dispatch-indirect` and the WASI 0.2 interfaces the
//! executor links.

use std::rc::Rc;

use arora_engine::call::{CallBridge, Callable, CallableId};
use arora_engine::engine::EngineBuilder;
use arora_engine::executor::component::ComponentExecutor;
use arora_engine::load::load_module_from_parts;
use arora_types::module::low::Header;
use arora_types::value::Value;
use uuid::Uuid;

/// Function ids implemented by modules/test-rust-component/src/lib.rs.
const ECHO_FN: &str = "dd6a4f61-9f88-4376-9d50-1e92e95a73aa";

/// The parameters a function of the header declares: `echo` takes a `u64`
/// under its own id, so a call to it echoes back as that function's result
/// (the re-entry tests read it so); the others take none.
fn parameters(function: &str) -> &'static str {
    if function == ECHO_FN {
        "[{ id: dd6a4f61-9f88-4376-9d50-1e92e95a73aa, name: value, type: { kind: scalar, id: 00000000-0000-0000-0000-000000000009 } }]"
    } else {
        "[]"
    }
}
const CALL_INDIRECT_FN: &str = "1bd1cf09-e94c-4c14-b653-32bd96157c41";
const WASI_FN: &str = "0455052e-bb89-41ec-a963-2bbe82ea583b";
const TRAP_FN: &str = "42681638-6fe3-4004-92fb-bb048dc3bbab";
const ERROR_BUFFER_FN: &str = "2041d791-4440-4dc9-b54d-1004e314cf99";
const CALL_FN: &str = "473a50b2-f275-4767-989c-69aa2c2def5b";
const SPIN_FN: &str = "5d0c4a3e-7b21-4f6e-9a8d-2c41b6e0f3a1";
const GROW_FN: &str = "9e7b1c52-0d3a-4b8f-8e61-f4a2c9d7b053";

/// What the guest's `wasi` function observes on a host that runs it without
/// arguments or an environment. The browser executor's test
/// (`crates/arora-engine/tests/browser_component.rs`) expects the same.
const WASI_REPORT: &str = "\
monotonic-advances=true
monotonic-resolution-positive=true
wall-after-2020=true
wall-nanoseconds-below-a-second=true
wall-resolution-positive=true
random-bytes=40
insecure-random-bytes=3
environment=0
arguments=0
initial-cwd=None
preopened-directories=0
stdout-writable=true
stdout-flushes=true
stdout-writes-zeroes=true
standard-streams-written=true
poll-ready=[0, 1]
stdout-ready=true
past-instant-waited=true";

const MODULE_ID: &str = "0cd3f0c6-95d4-4ab1-90fa-7fbcca25c2c6";

/// The module whose `spin` the guest's `functions` calls, when one is loaded
/// under this id.
const LOAD_HOOK_ID: &str = "6f3a9c2e-1b84-4d07-a5e2-8c9d0b7f4e16";

fn header() -> Header {
    header_as(MODULE_ID, "test-rust-component")
}

/// The test guest's header under the module id `id` and the name `name`, as
/// a second module served by the same artifact.
fn header_as(id: &str, name: &str) -> Header {
    let exports: String = [
        (ECHO_FN, "echo"),
        (CALL_INDIRECT_FN, "call-indirect"),
        (WASI_FN, "wasi"),
        (TRAP_FN, "trap"),
        (ERROR_BUFFER_FN, "error-buffer"),
        (CALL_FN, "call"),
        (SPIN_FN, "spin"),
        (GROW_FN, "grow"),
    ]
    .iter()
    .map(|(id, name)| {
        format!(
            "  - type: function\n    id: {id}\n    name: {name}\n    parameters: {}\n    \
             ret: {{ kind: scalar, id: 00000000-0000-0000-0000-000000000000 }}\n",
            parameters(id),
        )
    })
    .collect();
    let yaml = format!(
        r#"
id: {id}
name: {name}
author: ""
description: ~
license: ""
version: {{ major: 0, minor: 0, patch: 0 }}
executor: {{ name: wasm-component, min_version: ~, max_version: ~ }}
imports: []
executable_mime: ""
exports:
{exports}"#
    );
    serde_yaml::from_str(&yaml).expect("parse test header")
}

fn guest_bytes() -> Box<[u8]> {
    std::fs::read(env!(
        "CARGO_CDYLIB_FILE_TEST_RUST_COMPONENT_test_rust_component"
    ))
    .expect("read test-rust-component wasm")
    .into_boxed_slice()
}

#[test]
fn component_echo_roundtrip() {
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();

    let loaded = load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();
    assert_eq!(loaded.id, MODULE_ID.parse::<Uuid>().unwrap());

    let payload = b"hello component".to_vec();
    let result = engine
        .dispatch(
            &MODULE_ID.parse().unwrap(),
            &ECHO_FN.parse().unwrap(),
            &payload,
        )
        .unwrap();
    assert_eq!(&*result, payload.as_slice());
}

#[test]
fn component_bad_callable_arg_is_guest_error() {
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();

    // The guest rejects call-indirect args shorter than 8 bytes with a
    // typed guest error (no trap).
    let err = engine
        .dispatch(
            &MODULE_ID.parse().unwrap(),
            &CALL_INDIRECT_FN.parse().unwrap(),
            b"abc",
        )
        .unwrap_err();
    let message = format!("{err}");
    assert!(
        message.contains("8-byte callable id"),
        "unexpected error: {message}"
    );
}

struct FortyTwo;

impl Callable for FortyTwo {
    fn call(&self, _caller: &mut dyn CallBridge) -> Result<Value, arora_engine::call::CallError> {
        Ok(Value::U64(42))
    }
}

#[test]
fn component_calls_back_through_dispatch_indirect() {
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();

    let callable: Rc<dyn Callable> = Rc::new(FortyTwo);
    let CallableId { id } = engine.arora_register_callable(callable);

    let result = engine
        .dispatch(
            &MODULE_ID.parse().unwrap(),
            &CALL_INDIRECT_FN.parse().unwrap(),
            &id.to_le_bytes(),
        )
        .unwrap();

    let expected = arora_buffers::serde_uuid::serialize(&Value::U64(42));
    assert_eq!(&*result, &*expected);
}

#[test]
fn component_reaches_the_wasi_interfaces() {
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();

    let report = engine
        .dispatch(&MODULE_ID.parse().unwrap(), &WASI_FN.parse().unwrap(), &[])
        .unwrap();
    assert_eq!(std::str::from_utf8(&report).unwrap(), WASI_REPORT);
}

/// Calls the guest's `echo` through the engine, from inside a call into the
/// same guest: the echoed argument, a structure under the function's id,
/// reads back as the return value.
struct EchoThroughTheEngine;

impl Callable for EchoThroughTheEngine {
    fn call(&self, caller: &mut dyn CallBridge) -> Result<Value, arora_engine::call::CallError> {
        let echo: Uuid = ECHO_FN.parse().unwrap();
        let echoed = caller.arora_call(arora_types::call::Call {
            module_id: Some(MODULE_ID.parse().unwrap()),
            id: echo,
            args: vec![arora_types::value::StructureField {
                id: echo,
                value: Box::new(Value::U64(42)),
            }],
        })?;
        Ok(echoed.ret)
    }
}

/// A guest that calls back into the engine can be called again before that
/// call returns — a behavior tree's node running a child of the same module
/// does.
#[test]
fn component_is_reentered_through_the_host() {
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();

    let callable: Rc<dyn Callable> = Rc::new(EchoThroughTheEngine);
    let CallableId { id } = engine.arora_register_callable(callable);
    let result = engine
        .dispatch(
            &MODULE_ID.parse().unwrap(),
            &CALL_INDIRECT_FN.parse().unwrap(),
            &id.to_le_bytes(),
        )
        .unwrap();

    let expected = arora_buffers::serde_uuid::serialize(&Value::U64(42));
    assert_eq!(&*result, &*expected);
}

/// A guest that returns an error buffer, as a core module writes its errors,
/// has it read as its error, as a core module's is.
#[test]
fn component_error_buffer_is_a_guest_error() {
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();

    let error = engine
        .dispatch(
            &MODULE_ID.parse().unwrap(),
            &ERROR_BUFFER_FN.parse().unwrap(),
            b"no such thing",
        )
        .unwrap_err();
    assert!(
        matches!(&error, arora_engine::module::DispatchError::Guest { message } if message == "no such thing"),
        "{error:?}"
    );
}

/// A call that traps fails with its trap; the component, instantiated
/// afresh, serves the next call, without a reload.
#[test]
fn a_component_that_traps_is_instantiated_afresh_for_its_next_call() {
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    let compiled = arora_engine::compiled::CompiledModule::new(header(), &guest_bytes()).unwrap();
    engine.load_compiled_module(&compiled).unwrap();
    let (module, echo) = (MODULE_ID.parse().unwrap(), ECHO_FN.parse().unwrap());

    for _ in 0..2 {
        let trap = engine
            .dispatch(&module, &TRAP_FN.parse().unwrap(), &[])
            .unwrap_err();
        assert!(format!("{trap}").contains("dispatch trapped"), "{trap}");
        assert_eq!(
            &*engine.dispatch(&module, &echo, b"after").unwrap(),
            b"after"
        );
    }
}

/// A component instantiated afresh after a trap is loaded within the call that
/// triggers it: its instantiation, and a guest it calls, end by the earlier of
/// that call's deadline — the step's, in a step — and the load budget, never
/// by the long budgets the modules have. Running out of time leaves the
/// module trapped, not lost: once instantiating it takes no time, its next
/// call is served.
#[test]
fn instantiating_a_trapped_component_afresh_ends_by_its_call_s_deadline() {
    use arora_engine::limits::{GuestLimits, DEFAULT_CALL_BUDGET, EPOCH};
    use std::time::{Duration, Instant};
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    let (module, hook, echo): (Uuid, Uuid, Uuid) = (
        MODULE_ID.parse().unwrap(),
        LOAD_HOOK_ID.parse().unwrap(),
        ECHO_FN.parse().unwrap(),
    );
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();
    // Instantiating the guest afresh now calls the hook's `spin`.
    load_module_from_parts(&mut engine, header_as(LOAD_HOOK_ID, "hook"), guest_bytes()).unwrap();
    for id in [module, hook] {
        engine.set_guest_limits(
            id,
            GuestLimits {
                call_budget: Some(Duration::from_secs(30)),
                ..GuestLimits::default()
            },
        );
    }
    engine
        .dispatch(&module, &TRAP_FN.parse().unwrap(), &[])
        .unwrap_err();

    let step = Duration::from_millis(20);
    for (in_step, bound) in [(None, DEFAULT_CALL_BUDGET), (Some(step), step)] {
        let started = Instant::now();
        engine.set_step_budget(in_step);
        let refused = engine.dispatch(&module, &echo, b"after").unwrap_err();
        engine.set_step_budget(None);
        let took = started.elapsed();
        assert!(
            took >= bound && took < bound + 2 * EPOCH + Duration::from_millis(300),
            "step {in_step:?}: {took:?}"
        );
        assert!(
            format!("{refused}").contains("no time was left"),
            "{refused}"
        );
    }

    engine.unload_module(&hook).unwrap();
    assert_eq!(
        &*engine.dispatch(&module, &echo, b"after").unwrap(),
        b"after"
    );
}

/// A component that traps in a call it made back into itself is instantiated
/// afresh once that call returns: its next call is served.
#[test]
fn a_component_trapped_in_its_own_callback_serves_its_next_call() {
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();
    let (module, echo, trap): (Uuid, Uuid, Uuid) = (
        MODULE_ID.parse().unwrap(),
        ECHO_FN.parse().unwrap(),
        TRAP_FN.parse().unwrap(),
    );
    let mut arg = module.as_bytes().to_vec();
    arg.extend_from_slice(trap.as_bytes());
    let failed = engine
        .dispatch(&module, &CALL_FN.parse().unwrap(), &arg)
        .unwrap_err();
    assert!(format!("{failed}").contains("trapped"), "{failed}");
    assert_eq!(
        &*engine.dispatch(&module, &echo, b"after").unwrap(),
        b"after"
    );
}

/// A component loaded compiled ahead, as a device loads one, or reloaded, has
/// its calls' arguments checked against its header as one loaded from its
/// bytes does.
#[test]
fn a_compiled_or_reloaded_component_s_arguments_are_checked() {
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    let compiled = arora_engine::compiled::CompiledModule::new(header(), &guest_bytes()).unwrap();
    let echo: Uuid = ECHO_FN.parse().unwrap();
    let call = || arora_types::call::Call {
        module_id: Some(MODULE_ID.parse().unwrap()),
        id: echo,
        args: vec![arora_types::value::StructureField {
            id: echo,
            value: Box::new(Value::String("not a u64".into())),
        }],
    };
    let refused = |engine: &mut std::pin::Pin<Box<arora_engine::engine::Engine>>| {
        let error = engine.arora_call(call()).unwrap_err();
        assert!(
            matches!(&error, arora_engine::call::CallError::Generic { message }
                if message.contains("of 'echo'")),
            "{error:?}"
        );
    };
    engine.load_compiled_module(&compiled).unwrap();
    refused(&mut engine);
    engine.reload_module(&compiled).unwrap();
    refused(&mut engine);
}

/// A guest calls a function of a module — here its own — through
/// `host.dispatch`.
#[test]
fn component_calls_through_host_dispatch() {
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();

    let module: Uuid = MODULE_ID.parse().unwrap();
    let echo: Uuid = ECHO_FN.parse().unwrap();
    let mut arg = module.as_bytes().to_vec();
    arg.extend_from_slice(echo.as_bytes());
    arg.extend_from_slice(b"through the host");
    let result = engine
        .dispatch(&module, &CALL_FN.parse().unwrap(), &arg)
        .unwrap();
    assert_eq!(&*result, b"through the host");
}

/// One artifact serving two modules, loaded under one's header, serves that
/// module's functions only.
#[test]
fn component_serves_only_the_functions_its_header_declares() {
    let (mut engine, module) = test_rust_wasm_engine();

    // compass's `bearing`, which the same artifact serves under compass's id.
    let bearing: Uuid = "c4396a6b-600e-480e-adcd-7dd091a41a99".parse().unwrap();
    assert!(matches!(
        engine.dispatch(&module, &bearing, &[]),
        Err(arora_engine::module::DispatchError::FunctionNotFound { id }) if id == bearing
    ));
}

/// An engine with test-rust-wasm — two modules declared with `#[module]`, one
/// artifact — loaded under its `test_rust_wasm` module's header, and that id.
fn test_rust_wasm_engine() -> (std::pin::Pin<Box<arora_engine::engine::Engine>>, Uuid) {
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    let header = test_rust_wasm::test_rust_wasm::header(arora_types::module::low::Executor {
        name: "wasm-component".to_string(),
        min_version: None,
        max_version: None,
    });
    let module = header.id;
    let artifact = std::fs::read(env!("CARGO_CDYLIB_FILE_TEST_RUST_WASM_test_rust_wasm")).unwrap();
    load_module_from_parts(&mut engine, header, artifact.into()).unwrap();
    (engine, module)
}

/// An argument that is not an Arora Buffer — short, or a value cut short — is
/// a `#[module]`-declared guest's error, not a trap: it answers the next call.
#[test]
fn a_declared_component_refuses_an_argument_that_is_not_an_arora_buffer() {
    use arora_types::value::{Structure, StructureField};
    let (mut engine, module) = test_rust_wasm_engine();
    let cos: Uuid = "c13757cb-2311-4c93-abcc-cb12d6cbb859".parse().unwrap();
    let angle = StructureField {
        id: "6c2a157c-4235-47b0-bff3-1eeef3e5747d".parse().unwrap(),
        value: Box::new(Value::F32(0.0)),
    };
    let whole = arora_buffers::serde_uuid::serialize(&Value::Structure(Structure {
        id: cos,
        fields: vec![angle.clone()],
    }));
    let mut cut = whole[..whole.len() - 2].to_vec();
    let len = cut.len() as u32;
    cut[..4].copy_from_slice(&len.to_le_bytes());
    for arg in [&[][..], &[9, 0, 0, 0], &cut] {
        let error = engine.dispatch(&module, &cos, arg).unwrap_err();
        assert!(
            matches!(&error, arora_engine::module::DispatchError::Guest { message }
                if message.contains("not an Arora Buffer")),
            "{arg:?}: {error:?}"
        );
    }
    let result = engine
        .arora_call(arora_types::call::Call {
            module_id: Some(module),
            id: cos,
            args: vec![angle],
        })
        .expect("a call after the refusals");
    assert_eq!(result.ret, Value::F32(1.0));
}

/// A call to a component that loops forever is interrupted within its budget
/// and one epoch; the component, instantiated afresh, serves the next call.
#[test]
fn a_component_call_that_overruns_its_budget_is_interrupted() {
    use arora_engine::limits::{GuestLimits, EPOCH};
    use std::time::{Duration, Instant};
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    let compiled = arora_engine::compiled::CompiledModule::new(header(), &guest_bytes()).unwrap();
    engine.load_compiled_module(&compiled).unwrap();
    let (module, spin, echo): (Uuid, Uuid, Uuid) = (
        MODULE_ID.parse().unwrap(),
        SPIN_FN.parse().unwrap(),
        ECHO_FN.parse().unwrap(),
    );
    let budget = Duration::from_millis(50);
    engine.set_guest_limits(
        module,
        GuestLimits {
            call_budget: Some(budget),
            ..GuestLimits::default()
        },
    );

    let started = Instant::now();
    let overran = engine.dispatch(&module, &spin, &[]).unwrap_err();
    let took = started.elapsed();
    // Slack for a loaded machine: the deadline itself is the budget plus at
    // most one epoch.
    assert!(
        took >= budget && took < budget + EPOCH + Duration::from_millis(200),
        "{took:?}"
    );
    assert!(
        format!("{overran}").contains("of 'test-rust-component' overran its budget of 50 ms"),
        "{overran}"
    );
    assert_eq!(
        &*engine.dispatch(&module, &echo, b"after").unwrap(),
        b"after"
    );
}

/// A component whose allocator grows its memory past the module's cap sees
/// the allocation fail; under the cap, it succeeds.
#[test]
fn a_component_growing_its_memory_past_its_cap_sees_the_allocation_fail() {
    use arora_engine::limits::GuestLimits;
    let (module, grow): (Uuid, Uuid) = (MODULE_ID.parse().unwrap(), GROW_FN.parse().unwrap());
    for (memory, reserved) in [(32 << 20, 0), (128 << 20, 1)] {
        let mut engine = EngineBuilder::new()
            .with_guest_limits(GuestLimits {
                memory,
                ..GuestLimits::default()
            })
            .add_executor(ComponentExecutor::new().unwrap())
            .build();
        load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();
        let result = engine.dispatch(&module, &grow, &[]).unwrap();
        assert_eq!(&*result, &[reserved], "a cap of {memory} bytes");
    }
}

/// A component calling another through `host.dispatch` gives its callee no
/// more than its own deadline: the callee is interrupted at the caller's
/// budget, and serves its next call instantiated afresh.
#[test]
fn a_component_s_callee_ends_by_its_caller_s_deadline() {
    use arora_engine::limits::{GuestLimits, EPOCH};
    use std::time::{Duration, Instant};
    const CALLEE_ID: &str = "7e0b9c4d-2a61-4f38-b5d7-0c9e8a1f6b24";
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();
    load_module_from_parts(&mut engine, header_as(CALLEE_ID, "callee"), guest_bytes()).unwrap();
    let (caller, callee, spin): (Uuid, Uuid, Uuid) = (
        MODULE_ID.parse().unwrap(),
        CALLEE_ID.parse().unwrap(),
        SPIN_FN.parse().unwrap(),
    );
    // Only the caller has a budget: the callee's call ends by it.
    let budget = Duration::from_millis(30);
    engine.set_guest_limits(
        caller,
        GuestLimits {
            call_budget: Some(budget),
            ..GuestLimits::default()
        },
    );
    let mut arg = callee.as_bytes().to_vec();
    arg.extend_from_slice(spin.as_bytes());
    let started = Instant::now();
    let failed = engine
        .dispatch(&caller, &CALL_FN.parse().unwrap(), &arg)
        .unwrap_err();
    let took = started.elapsed();
    assert!(
        took >= budget && took < budget + 2 * EPOCH + Duration::from_millis(200),
        "{took:?}"
    );
    // The caller, past its deadline once the callee returns, is interrupted
    // too: its call fails with its own overrun.
    assert!(
        format!("{failed}").contains("overran its budget"),
        "{failed}"
    );
    assert_eq!(
        &*engine
            .dispatch(&callee, &ECHO_FN.parse().unwrap(), b"after")
            .unwrap(),
        b"after"
    );
}

/// A component calling back into itself through `host.dispatch` keeps its
/// call's deadline and budget: the re-entered call is interrupted at the
/// budget of the call under way.
#[test]
fn a_component_called_back_keeps_its_call_s_deadline() {
    use arora_engine::limits::{GuestLimits, EPOCH};
    use std::time::{Duration, Instant};
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();
    let (module, spin): (Uuid, Uuid) = (MODULE_ID.parse().unwrap(), SPIN_FN.parse().unwrap());
    let budget = Duration::from_millis(50);
    engine.set_guest_limits(
        module,
        GuestLimits {
            call_budget: Some(budget),
            ..GuestLimits::default()
        },
    );
    let mut arg = module.as_bytes().to_vec();
    arg.extend_from_slice(spin.as_bytes());
    let started = Instant::now();
    let failed = engine
        .dispatch(&module, &CALL_FN.parse().unwrap(), &arg)
        .unwrap_err();
    let took = started.elapsed();
    assert!(
        took >= budget && took < budget + 2 * EPOCH + Duration::from_millis(200),
        "{took:?}"
    );
    assert!(
        format!("{failed}").contains("overran its budget of 50 ms"),
        "{failed}"
    );
}

/// During a step, a module's own budget and the time left in the step bound
/// a component's call together: whichever ends first.
#[test]
fn a_component_call_ends_by_the_earlier_of_the_step_and_its_module_s_budget() {
    use arora_engine::limits::{GuestLimits, EPOCH};
    use std::time::{Duration, Instant};
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    let compiled = arora_engine::compiled::CompiledModule::new(header(), &guest_bytes()).unwrap();
    engine.load_compiled_module(&compiled).unwrap();
    let (module, spin): (Uuid, Uuid) = (MODULE_ID.parse().unwrap(), SPIN_FN.parse().unwrap());
    let (short, long) = (Duration::from_millis(30), Duration::from_millis(400));
    for (step, own) in [(long, short), (short, long)] {
        engine.reload_module(&compiled).unwrap();
        engine.set_guest_limits(
            module,
            GuestLimits {
                call_budget: Some(own),
                ..GuestLimits::default()
            },
        );
        let started = Instant::now();
        engine.set_step_budget(Some(step));
        let overran = engine.dispatch(&module, &spin, &[]).unwrap_err();
        let took = started.elapsed();
        engine.set_step_budget(None);
        assert!(
            took >= short && took < short + 2 * EPOCH + Duration::from_millis(200),
            "step {step:?}, own {own:?}: {took:?}"
        );
        assert!(format!("{overran}").contains("overran"), "{overran}");
    }
}

/// The epoch counter follows the wall clock: a long budget is not stretched
/// by the ticker's sleeps overrunning.
#[test]
fn a_long_component_budget_is_kept_to_the_wall_clock() {
    use arora_engine::limits::GuestLimits;
    use std::time::{Duration, Instant};
    let budget = Duration::from_millis(800);
    let mut engine = EngineBuilder::new()
        .with_guest_limits(GuestLimits {
            call_budget: Some(budget),
            ..GuestLimits::default()
        })
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();
    let started = Instant::now();
    engine
        .dispatch(&MODULE_ID.parse().unwrap(), &SPIN_FN.parse().unwrap(), &[])
        .unwrap_err();
    let took = started.elapsed();
    // Counting sleeps instead overruns by several percent: 50 ms and more
    // here.
    assert!(
        took >= budget && took < budget + Duration::from_millis(40),
        "{took:?}"
    );
}

/// A component called back by its callee ends by the callee's deadline when
/// it is the earlier: `a` (one second) calls `b` (30 ms), which calls `a`'s
/// `spin`; the spin ends at `b`'s budget.
#[test]
fn a_component_called_back_ends_by_its_caller_s_earlier_deadline() {
    use arora_engine::limits::{GuestLimits, EPOCH};
    use std::time::{Duration, Instant};
    const B_ID: &str = "3c5e7a91-8d24-4b6f-a0e3-5f19c2d84b70";
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();
    load_module_from_parts(&mut engine, header_as(B_ID, "b"), guest_bytes()).unwrap();
    let (a, b, call, spin): (Uuid, Uuid, Uuid, Uuid) = (
        MODULE_ID.parse().unwrap(),
        B_ID.parse().unwrap(),
        CALL_FN.parse().unwrap(),
        SPIN_FN.parse().unwrap(),
    );
    let budget = |budget| GuestLimits {
        call_budget: Some(budget),
        ..GuestLimits::default()
    };
    engine.set_guest_limits(a, budget(Duration::from_secs(1)));
    let short = Duration::from_millis(30);
    engine.set_guest_limits(b, budget(short));
    // `a` calls `b`'s `call`, which calls `a`'s `spin`.
    let mut arg = b.as_bytes().to_vec();
    arg.extend_from_slice(call.as_bytes());
    arg.extend_from_slice(a.as_bytes());
    arg.extend_from_slice(spin.as_bytes());
    let started = Instant::now();
    engine.dispatch(&a, &call, &arg).unwrap_err();
    let took = started.elapsed();
    assert!(
        took >= short && took < short + 2 * EPOCH + Duration::from_millis(200),
        "{took:?}"
    );
}
/// An executor whose load calls a guest function through the engine, as a
/// guest's initializer would, then refuses the module.
struct CallsWhileLoading {
    engine: Option<arora_engine::engine::EngineRef>,
    module: Uuid,
    function: Uuid,
}

impl arora_engine::executor::Executor for CallsWhileLoading {
    fn set_engine(&mut self, engine: arora_engine::engine::EngineRef) {
        self.engine = Some(engine);
    }

    fn name(&self) -> &'static str {
        "calls-while-loading"
    }

    fn load_module(
        &mut self,
        _: arora_types::module::low::ModuleDefinition,
    ) -> Result<Box<dyn arora_engine::module::Module>, arora_engine::executor::LoadModuleError>
    {
        // SAFETY: the engine outlives its executors, and calls nothing else
        // while it loads.
        let engine = unsafe { &mut *self.engine.expect("set_engine") };
        let _ = engine.dispatch(&self.module, &self.function, &[]);
        Err(arora_engine::executor::LoadModuleError::Internal(
            "loaded nothing".into(),
        ))
    }

    fn unload_module(&mut self, _: Uuid) -> Result<(), arora_engine::executor::UnloadModuleError> {
        Ok(())
    }
}

/// A guest loading gives the guests it calls no more than the load's own
/// budget: a component allowed 30 s, called while another module loads, is
/// interrupted when the load's second is out, and serves its next call
/// instantiated afresh.
#[test]
fn a_guest_called_while_another_loads_ends_by_the_load_s_budget() {
    use arora_engine::limits::{GuestLimits, DEFAULT_CALL_BUDGET};
    use std::time::{Duration, Instant};
    let (spinner, spin): (Uuid, Uuid) = (MODULE_ID.parse().unwrap(), SPIN_FN.parse().unwrap());
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .add_executor(CallsWhileLoading {
            engine: None,
            module: spinner,
            function: spin,
        })
        .build();
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();
    engine.set_guest_limits(
        spinner,
        GuestLimits {
            call_budget: Some(Duration::from_secs(30)),
            ..GuestLimits::default()
        },
    );
    let mut loading = header_as("5b2d8e17-4c90-4a3f-9e61-07d4c8a2f5b3", "loading");
    loading.executor.name = "calls-while-loading".to_string();
    let started = Instant::now();
    load_module_from_parts(&mut engine, loading, Box::new([])).unwrap_err();
    let took = started.elapsed();
    assert!(
        took < DEFAULT_CALL_BUDGET + Duration::from_millis(500),
        "{took:?}"
    );
    assert_eq!(
        &*engine
            .dispatch(&spinner, &ECHO_FN.parse().unwrap(), b"after")
            .unwrap(),
        b"after"
    );
}

/// A module reloading, whose new instance's initializer calls the old one,
/// gives that call no more than the load's own budget: the old instance is
/// not a call under way, whose deadline it would keep. Its last call, allowed
/// 30 s, left its store's deadline 30 s out.
#[test]
fn a_reload_s_initializer_calling_the_old_instance_ends_by_the_load_s_budget() {
    use arora_engine::compiled::CompiledModule;
    use arora_engine::limits::{GuestLimits, DEFAULT_CALL_BUDGET};
    use std::time::{Duration, Instant};
    let (module, spin, echo): (Uuid, Uuid, Uuid) = (
        MODULE_ID.parse().unwrap(),
        SPIN_FN.parse().unwrap(),
        ECHO_FN.parse().unwrap(),
    );
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .add_executor(CallsWhileLoading {
            engine: None,
            module,
            function: spin,
        })
        .build();
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();
    engine.set_guest_limits(
        module,
        GuestLimits {
            call_budget: Some(Duration::from_secs(30)),
            ..GuestLimits::default()
        },
    );
    engine.dispatch(&module, &echo, &[]).unwrap();
    let mut reloading = header();
    reloading.executor.name = "calls-while-loading".to_string();
    let reloading = CompiledModule::new(reloading, &[]).unwrap();
    let started = Instant::now();
    engine.reload_module(&reloading).unwrap_err();
    let took = started.elapsed();
    assert!(
        took < DEFAULT_CALL_BUDGET + Duration::from_millis(500),
        "{took:?}"
    );
}

/// A call made once the step's time is used up is not made: the module is not
/// entered, and serves the calls of the next step. `a` uses up the step;
/// `b`, called after it in the same step, is refused and stays usable.
#[test]
fn a_call_with_no_time_left_in_the_step_is_not_made() {
    use std::time::Duration;
    const B_ID: &str = "8a14c6e0-5f3b-4d92-b7e8-2c06d9f1a3e5";
    let mut engine = EngineBuilder::new()
        .add_executor(ComponentExecutor::new().unwrap())
        .build();
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();
    load_module_from_parts(&mut engine, header_as(B_ID, "b"), guest_bytes()).unwrap();
    let (a, b, spin, echo): (Uuid, Uuid, Uuid, Uuid) = (
        MODULE_ID.parse().unwrap(),
        B_ID.parse().unwrap(),
        SPIN_FN.parse().unwrap(),
        ECHO_FN.parse().unwrap(),
    );
    engine.set_step_budget(Some(Duration::from_millis(30)));
    engine.dispatch(&a, &spin, &[]).unwrap_err();
    let refused = engine.dispatch(&b, &spin, &[]).unwrap_err();
    engine.set_step_budget(None);
    assert!(
        format!("{refused}").contains("was not made: no time was left"),
        "{refused}"
    );
    assert!(engine.dispatch(&b, &echo, &[]).is_ok());
}

/// An executor of its own under the component executor's name, which loads a
/// module from its bytes and serves `echo`.
struct LoadsBytes;

struct Echoes;

impl arora_engine::module::Module for Echoes {
    fn dispatch(
        &mut self,
        _: &Uuid,
        arg: &[u8],
    ) -> Result<Box<[u8]>, arora_engine::module::DispatchError> {
        Ok(arg.into())
    }
}

impl arora_engine::executor::Executor for LoadsBytes {
    fn set_engine(&mut self, _: arora_engine::engine::EngineRef) {}

    fn name(&self) -> &'static str {
        "wasm-component"
    }

    fn load_module(
        &mut self,
        _: arora_types::module::low::ModuleDefinition,
    ) -> Result<Box<dyn arora_engine::module::Module>, arora_engine::executor::LoadModuleError>
    {
        Ok(Box::new(Echoes))
    }

    fn unload_module(&mut self, _: Uuid) -> Result<(), arora_engine::executor::UnloadModuleError> {
        Ok(())
    }
}

/// An executor that loads modules from their bytes, registered under a name
/// whose modules the engine could compile, still loads them from their bytes.
#[test]
fn an_executor_loading_bytes_under_the_component_executor_s_name_loads_them() {
    let mut engine = EngineBuilder::new().add_executor(LoadsBytes).build();
    load_module_from_parts(&mut engine, header(), guest_bytes()).unwrap();
    let echoed = engine
        .dispatch(&MODULE_ID.parse().unwrap(), &ECHO_FN.parse().unwrap(), b"")
        .unwrap();
    assert!(echoed.is_empty());
}
