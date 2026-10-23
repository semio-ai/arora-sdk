//! Browser integration tests. One loads the test-rust-wasm guest module — a
//! component, built for `wasm32-wasip2` as a cdylib artifact dependency —
//! through arora-web's JS-facing `Engine`, then calls its `ping`
//! function; the others drive a device's client operations and drain its
//! store's changes through `AroraRuntime`, on a device nobody steps, one the
//! page steps, and one `run()` owns.

#![cfg(target_arch = "wasm32")]

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use arora::{Arora, BehaviorTreeInterpreter, ModuleBuilder};
use arora_behavior::{
    built_in, BehaviorContext, BehaviorError, BehaviorInterpreter, BehaviorStatus, Graph,
    GraphDiff, GraphType, RunPolicy, Status, TaskHandle, TaskId,
};
use arora_hal::{FakeHal, Hal};
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::{Call, CallError, CallResult};
use arora_types::data::{
    DataError, DataStore, Key, KeyMeta, Slot, State, StateChange, Subscription,
};
use arora_types::module::low::Header;
use arora_types::record::module::frozen::{Export, Function, Parameter};
use arora_types::record::ty::{FrozenScalar, FrozenTy, PrimitiveKind};
use arora_types::record::{FrozenReference, Version};
use arora_types::value::Value;
use arora_web::{AroraWeb, AroraWebBuilder, CompiledModule, Engine};
use futures::FutureExt;
use uuid::Uuid;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

// test-rust-wasm is a cdylib artifact dependency; cargo exposes its built wasm
// path as CARGO_CDYLIB_FILE_<DEP>_<lib>.
const WASM_BYTES: &[u8] = include_bytes!(env!("CARGO_CDYLIB_FILE_TEST_RUST_WASM_test_rust_wasm"));

// `ping`, as the guest's Rust declaration pins it.
const PING_FN_ID: &str = "5f423ba9-d5f9-46d7-a9b5-fb7d28f99ea6";

/// The guest's header, from its declaration — what an export step would write
/// as a `module.yaml`, here handed straight to the engine.
fn header() -> Header {
    test_rust_wasm::test_rust_wasm::header(arora_types::module::low::Executor {
        name: "wasm-component".to_string(),
        min_version: None,
        max_version: None,
    })
}

fn header_json() -> String {
    serde_json::to_string(&header()).expect("the declared header serializes to json")
}

#[wasm_bindgen_test]
fn load_and_ping_test_rust_wasm() {
    let header_json = header_json();
    let mut engine = Engine::new();

    let module_id: String = engine
        .load_module(&header_json, WASM_BYTES)
        .map_err(jsval_to_string)
        .expect("loadModule succeeded");

    // Sanity: returned ID matches the header's id.
    assert_eq!(module_id, header().id.to_string());

    let call_json = format!(r#"{{"id":"{PING_FN_ID}","args":[]}}"#);
    let result = engine
        .call(&call_json)
        .map_err(jsval_to_string)
        .expect("call(ping) succeeded");

    // Just assert we got some non-empty JSON back; full result shape is
    // an `arora_types::call::CallResult` (return value + mutated).
    assert!(!result.is_empty(), "result was empty");
}

/// Same flow through the prepared-module path used for executables larger
/// than Chrome's 8 MB main-thread compile/instantiate limit.
#[wasm_bindgen_test]
async fn prepare_load_and_ping_test_rust_wasm() {
    let header_json = header_json();
    let mut engine = Engine::new();

    wasm_bindgen_futures::JsFuture::from(engine.prepare_module(&header_json, WASM_BYTES.to_vec()))
        .await
        .expect("prepareModule succeeded");

    let module_id: String = engine
        .load_prepared_module(&header_json)
        .map_err(jsval_to_string)
        .expect("loadPreparedModule succeeded");

    assert_eq!(module_id, header().id.to_string());

    let call_json = format!(r#"{{"id":"{PING_FN_ID}","args":[]}}"#);
    let result = engine
        .call(&call_json)
        .map_err(jsval_to_string)
        .expect("call(ping) succeeded");
    assert!(!result.is_empty(), "result was empty");
}

/// Devices built with `withCompiledModule` from one `CompiledModule` each run
/// the module: a call reaches it on every device.
#[wasm_bindgen_test]
async fn devices_share_one_compiled_module() {
    let compiled = CompiledModule::new(&header_json(), WASM_BYTES)
        .map_err(jsval_to_string)
        .expect("the test guest compiles");
    let call_json = format!(
        r#"{{"module_id":"{}","id":"{PING_FN_ID}","args":[]}}"#,
        header().id
    );
    for _ in 0..2 {
        let mut builder = AroraWebBuilder::new();
        builder.with_compiled_module(&compiled);
        let web = builder.build().map_err(jsval_to_string).expect("build");
        let pinged = web.call(&call_json);
        web.step(10.0).expect("step");
        JsFuture::from(pinged)
            .await
            .map_err(jsval_to_string)
            .expect("ping answers");
    }
}

/// The records a guest's package ships, as `withRecords` takes them: each
/// record file's path under `records/` mapped to its YAML.
fn records() -> JsValue {
    use futures::FutureExt;
    let mut registry = arora::LocalRegistry::new();
    registry
        .add_declared_module::<test_rust_wasm::test_rust_wasm::Module>(
            arora_types::record::Version::from(header().version).0,
        )
        .now_or_never()
        .expect("a local registry adds without waiting")
        .expect("the guest's records are added");
    let files = js_sys::Object::new();
    for (kind, stem, yaml) in
        arora_registry::local_yaml::records_as_yaml(&registry).expect("the records serialize")
    {
        js_sys::Reflect::set(
            &files,
            &JsValue::from_str(&format!("{}/{stem}.yaml", kind.directory())),
            &JsValue::from_str(&yaml),
        )
        .expect("an object takes a property");
    }
    files.into()
}

/// A guest export returning a structure is described from the module's
/// records: without them `describeMethods` does not list `reading`; with
/// `withRecords`, it lists it, returning the structure.
#[wasm_bindgen_test]
async fn with_records_describes_a_record_typed_export() {
    for with_records in [false, true] {
        let mut builder = AroraWebBuilder::new();
        builder
            .with_module(&header_json(), WASM_BYTES)
            .map_err(jsval_to_string)
            .expect("withModule");
        if with_records {
            builder
                .with_records(records())
                .map_err(jsval_to_string)
                .expect("withRecords");
        }
        let web = builder.build().map_err(jsval_to_string).expect("build");
        let methods = web.describe_methods(Some("reading".to_string()));
        web.step(10.0).expect("step");
        let methods = json(
            JsFuture::from(methods)
                .await
                .expect("describeMethods resolves"),
        );
        let paths: Vec<&str> = methods
            .as_array()
            .expect("an array")
            .iter()
            .filter_map(|method| method["path"].as_str())
            .collect();
        if with_records {
            assert_eq!(paths, vec!["reading", "readings"], "{methods}");
            assert_eq!(methods[0]["return_type"], "struct", "{methods}");
        } else {
            assert!(paths.is_empty(), "{methods}");
        }
    }
}

/// A path outside the records layout is refused, naming it.
#[wasm_bindgen_test]
fn with_records_refuses_a_path_outside_the_layout() {
    let files = js_sys::Object::new();
    js_sys::Reflect::set(
        &files,
        &JsValue::from_str("records.json"),
        &JsValue::from_str("[]"),
    )
    .expect("an object takes a property");
    let error = AroraWebBuilder::new()
        .with_records(files.into())
        .map_err(jsval_to_string)
        .expect_err("records.json is not a record");
    assert!(error.contains("'records.json'"), "{error}");
}

// =============================================================================
// The device's client operations
// =============================================================================

const TOOLS: Uuid = Uuid::from_u128(0x7001);
const DOUBLE: Uuid = Uuid::from_u128(0x7002);
const DOUBLE_X: Uuid = Uuid::from_u128(0x7003);
const WAVE: Uuid = Uuid::from_u128(0x7004);
const TOOLS_STOP: Uuid = Uuid::from_u128(0x7005);
const PLAYER: Uuid = Uuid::from_u128(0x7101);
const PLAYER_STOP: Uuid = Uuid::from_u128(0x7102);

/// The key `double` writes its answer to: a FakeHal position setpoint, so the
/// HAL holds what a step writes it.
const ARM: &str = "arm/target.position";

fn answer(ret: Value) -> Result<CallResult, CallError> {
    Ok(CallResult {
        ret,
        mutated: Vec::new(),
    })
}

/// One tick of the device's behavior, as the bench saw it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Tick {
    /// The step's `arora/dt`, in nanoseconds.
    dt_ns: u64,
    /// Whether `run()` owned the device during the tick.
    running: bool,
}

type Hook = Box<dyn FnMut(&AroraWeb)>;

/// The device's behavior-tree interpreter, recording each tick and calling the
/// bench's hook from inside it, as a module calling a page's callback does.
struct Recording {
    inner: BehaviorTreeInterpreter,
    ticks: Rc<RefCell<Vec<Tick>>>,
    web: Rc<OnceCell<Weak<AroraWeb>>>,
    hook: Rc<RefCell<Option<Hook>>>,
}

impl BehaviorInterpreter for Recording {
    fn tick(&mut self, ctx: &mut BehaviorContext) -> Result<BehaviorStatus, BehaviorError> {
        let dt_ns = match ctx.store.read(&[Key::new(built_in::DT)]).pop().flatten() {
            Some(Value::U64(dt)) => dt,
            other => panic!("a step publishes its dt: {other:?}"),
        };
        let web = self.web.get().and_then(Weak::upgrade);
        let running = web.as_ref().is_some_and(|web| web.running());
        self.ticks.borrow_mut().push(Tick { dt_ns, running });
        if let (Some(web), Some(hook)) = (web, self.hook.borrow_mut().as_mut()) {
            hook(&web);
        }
        self.inner.tick(ctx)
    }

    fn apply(&mut self, diff: GraphDiff) -> Result<(), BehaviorError> {
        self.inner.apply(diff)
    }

    fn load(&mut self, graph: Graph) -> Result<(), BehaviorError> {
        self.inner.load(graph)
    }

    fn spawn(&mut self, call: Call, policy: RunPolicy) -> Result<TaskHandle, BehaviorError> {
        self.inner.spawn(call, policy)
    }

    fn spawn_graph(
        &mut self,
        graph_type: &GraphType,
        graph: Graph,
        policy: RunPolicy,
    ) -> Result<TaskHandle, BehaviorError> {
        self.inner.spawn_graph(graph_type, graph, policy)
    }

    fn halt(&mut self, task: TaskId) -> Result<(), BehaviorError> {
        self.inner.halt(task)
    }

    fn described_methods(&self) -> HashMap<Uuid, Export> {
        self.inner.described_methods()
    }
}

/// A store whose writes fail while `failing` is set, so a step fails at its
/// first write, the clock's.
#[derive(Clone)]
struct FailingStore {
    inner: SimpleDataStore,
    failing: Arc<AtomicBool>,
}

impl DataStore for FailingStore {
    fn read(&self, keys: &[Key]) -> Vec<Option<Value>> {
        self.inner.read(keys)
    }

    fn write(&self, changes: StateChange) -> Result<(), DataError> {
        if self.failing.load(Ordering::Relaxed) {
            return Err(DataError::Other("the store refuses writes".to_string()));
        }
        self.inner.write(changes)
    }

    fn snapshot(&self) -> State {
        self.inner.snapshot()
    }

    fn slot(&self, key: &Key) -> Box<dyn Slot> {
        self.inner.slot(key)
    }

    fn subscribe(&self) -> Subscription {
        self.inner.subscribe()
    }

    fn meta(&self, keys: &[Key]) -> Vec<Option<KeyMeta>> {
        self.inner.meta(keys)
    }

    fn all_meta(&self) -> HashMap<Key, KeyMeta> {
        self.inner.all_meta()
    }

    fn set_meta(&self, meta: HashMap<Key, KeyMeta>) -> Result<(), DataError> {
        self.inner.set_meta(meta)
    }

    fn set_prefix_meta(&self, meta: HashMap<String, KeyMeta>) -> Result<(), DataError> {
        self.inner.set_prefix_meta(meta)
    }

    fn clone_box(&self) -> Box<dyn DataStore> {
        Box::new(self.clone())
    }
}

/// A device nobody steps unless a test says so, with what a test observes of
/// it: the HAL it writes, every tick of its behavior, and the order `double`
/// was called in.
struct Bench {
    web: Rc<AroraWeb>,
    hal: FakeHal,
    ticks: Rc<RefCell<Vec<Tick>>>,
    doubled: Rc<RefCell<Vec<f64>>>,
    hook: Rc<RefCell<Option<Hook>>>,
    /// Set, the device's store refuses writes, so its steps fail.
    failing: Arc<AtomicBool>,
}

/// A device whose `tools` module exports `double(x: f64) -> f64`, which also
/// writes its answer to the HAL's `arm` setpoint, the task-shaped
/// `wave() -> Status` (running until halted) and `stop()`, and whose `player`
/// module exports a `stop()` of its own.
fn bench() -> Bench {
    let signature = |parameters: &[(Uuid, &str)], return_ty: FrozenTy| Function {
        parameters: parameters
            .iter()
            .map(|(id, name)| {
                (
                    *id,
                    Parameter {
                        name: name.to_string(),
                        ty: FrozenTy::from(PrimitiveKind::F64),
                        mutable: false,
                    },
                )
            })
            .collect(),
        parameter_ordering: parameters.iter().map(|(id, _)| *id).collect(),
        return_ty,
    };
    let status = FrozenTy::FrozenScalar(FrozenScalar {
        reference: FrozenReference {
            id: arora_behavior::STATUS_ENUMERATION_ID,
            version: Version::parse("1.0.0").expect("a valid version"),
        },
    });
    let unit = || FrozenTy::from(PrimitiveKind::Unit);
    let store: Rc<OnceCell<Box<dyn DataStore>>> = Rc::default();
    let doubled: Rc<RefCell<Vec<f64>>> = Rc::default();
    let tools = ModuleBuilder::new(TOOLS)
        .described_function(
            DOUBLE,
            "double",
            signature(&[(DOUBLE_X, "x")], FrozenTy::from(PrimitiveKind::F64)),
            {
                let store = store.clone();
                let doubled = doubled.clone();
                move |call| match call.args.first().map(|field| field.value.as_ref()) {
                    Some(Value::F64(x)) => {
                        doubled.borrow_mut().push(*x);
                        store
                            .get()
                            .expect("the device is built")
                            .write(StateChange::set(ARM, Value::F64(2.0 * x)))
                            .expect("write the arm setpoint");
                        answer(Value::F64(2.0 * x))
                    }
                    _ => Err(CallError::Generic {
                        message: "double takes an f64".to_string(),
                    }),
                }
            },
        )
        .described_function(WAVE, "wave", signature(&[], status), |_call| {
            answer(Status::Running.into())
        })
        .described_function(TOOLS_STOP, "stop", signature(&[], unit()), |_call| {
            answer(Value::Unit)
        })
        .build();
    let player = ModuleBuilder::new(PLAYER)
        .described_function(PLAYER_STOP, "stop", signature(&[], unit()), |_call| {
            answer(Value::String("player stopped".to_string()))
        })
        .build();
    let hal = FakeHal::new();
    let ticks: Rc<RefCell<Vec<Tick>>> = Rc::default();
    let web_cell: Rc<OnceCell<Weak<AroraWeb>>> = Rc::default();
    let hook: Rc<RefCell<Option<Hook>>> = Rc::default();
    let failing = Arc::new(AtomicBool::new(false));
    let arora = Arora::builder()
        .with_data_store(Box::new(FailingStore {
            inner: SimpleDataStore::new(),
            failing: failing.clone(),
        }))
        .with_hal(Box::new(hal.clone()))
        .with_behavior_interpreter(Box::new(Recording {
            inner: BehaviorTreeInterpreter::new(),
            ticks: ticks.clone(),
            web: web_cell.clone(),
            hook: hook.clone(),
        }))
        .with_host_module(tools)
        .with_host_module(player)
        .build()
        .expect("build the device");
    let _ = store.set(arora.store().clone_box());
    let web = Rc::new(AroraWeb::from(arora));
    let _ = web_cell.set(Rc::downgrade(&web));
    Bench {
        web,
        hal,
        ticks,
        doubled,
        hook,
        failing,
    }
}

impl Bench {
    /// What the HAL holds for the `arm` setpoint.
    fn arm(&self) -> Option<Value> {
        self.hal
            .read(&[Key::new(ARM)])
            .now_or_never()
            .expect("the fake HAL reads at once")
            .expect("the fake HAL reads")
            .pop()
            .flatten()
    }

    fn ticks(&self) -> Vec<Tick> {
        self.ticks.borrow().clone()
    }

    fn read(&self, path: &str) -> serde_json::Value {
        let paths = serde_wasm_bindgen::to_value(&vec![path.to_string()]).expect("paths");
        json(self.web.read_values(paths).expect("read a key"))[path].clone()
    }
}

/// A tick of a step at a zero dt the page's operations took.
const ZERO_DT: Tick = Tick {
    dt_ns: 0,
    running: false,
};

/// The global `setTimeout`, read off the global object: arora-web binds the
/// same import, and a second binding of it collides at link time.
fn set_timeout(callback: &js_sys::Function, ms: i32) {
    let set_timeout: js_sys::Function =
        js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("setTimeout"))
            .expect("a global setTimeout")
            .unchecked_into();
    set_timeout
        .call2(&JsValue::NULL, callback, &JsValue::from(ms))
        .expect("setTimeout schedules");
}

/// Await `promise`, failing the test if it has not settled within two
/// seconds: an operation that waits for a step nobody takes never does.
async fn settles(promise: js_sys::Promise, what: &str) -> Result<JsValue, JsValue> {
    const PATIENCE_MS: i32 = 2_000;
    let late = js_sys::Promise::new(&mut |resolve, _reject| {
        set_timeout(&resolve, PATIENCE_MS);
    });
    match futures::future::select(JsFuture::from(promise), JsFuture::from(late)).await {
        futures::future::Either::Left((settled, _)) => settled,
        futures::future::Either::Right(_) => {
            panic!("{what} did not settle within {PATIENCE_MS} ms")
        }
    }
}

/// Let the microtasks queued so far run, and those they queue.
async fn yield_to_tasks() {
    for _ in 0..4 {
        JsFuture::from(js_sys::Promise::resolve(&JsValue::NULL))
            .await
            .expect("a resolved promise");
    }
}

/// Wait `ms` of wall time, through a timer, so a `run()` loop takes steps.
async fn sleep(ms: i32) {
    JsFuture::from(js_sys::Promise::new(&mut |resolve, _reject| {
        set_timeout(&resolve, ms);
    }))
    .await
    .expect("a timer resolves");
}

fn json(value: JsValue) -> serde_json::Value {
    serde_wasm_bindgen::from_value(value).expect("a JSON-shaped answer")
}

fn status_json(status: Status) -> serde_json::Value {
    serde_json::to_value(Value::from(status)).expect("a status serializes")
}

fn double_call(x: f64) -> String {
    format!(
        r#"{{"module_id":"{TOOLS}","id":"{DOUBLE}","args":[{{"id":"{DOUBLE_X}","value":{{"f64":{x}}}}}]}}"#
    )
}

fn wave_call() -> String {
    format!(r#"{{"module_id":"{TOOLS}","id":"{WAVE}","args":[]}}"#)
}

/// Every operation that changes the device settles on a device nobody steps,
/// its effect applied and the HAL written by a step at a zero dt.
#[wasm_bindgen_test]
async fn each_operation_settles_on_a_device_nobody_steps() {
    let bench = bench();
    let web = &bench.web;

    let called = settles(web.call(&double_call(0.25)), "call").await;
    let called: serde_json::Value =
        serde_json::from_str(&called.expect("call answers").as_string().expect("JSON"))
            .expect("a CallResult");
    assert_eq!(called["ret"]["f64"].as_f64(), Some(0.5), "{called}");
    assert_eq!(
        bench.arm(),
        Some(Value::F64(0.5)),
        "the HAL holds the call's write"
    );

    let invoked = settles(
        web.invoke("double", r#"{"x": {"f64": 1.5}}"#, None),
        "invoke",
    )
    .await
    .expect("invoke answers");
    assert_eq!(json(invoked)["f64"].as_f64(), Some(3.0));
    assert_eq!(
        bench.arm(),
        Some(Value::F64(3.0)),
        "the HAL holds the invoke's write"
    );

    let spawned = settles(web.spawn(&wave_call()), "spawn")
        .await
        .expect("spawn answers");
    let handle = json(spawned);
    let run = handle["run"]
        .as_str()
        .expect("the handle names its run")
        .to_string();
    let status = handle["status"]
        .as_str()
        .expect("its status key")
        .to_string();
    assert_eq!(
        bench.read(&status),
        status_json(Status::Running),
        "the run started"
    );

    let started = settles(web.invoke("wave", "{}", None), "invoke of a task")
        .await
        .expect("invoke starts the run");
    assert!(
        json(started)["run"].is_string(),
        "an invoke of a task answers its handle"
    );

    settles(web.halt(&run), "halt")
        .await
        .expect("the halt is applied");
    web.step(10.0).expect("step");
    assert_eq!(
        bench.read(&status),
        status_json(Status::Failure),
        "a halted run ends Failure"
    );

    web.set_value(ARM, r#"{"f64": 0.75}"#)
        .expect("write the arm");
    assert_eq!(
        bench.read(ARM),
        serde_json::json!({"f64": 0.75}),
        "the store holds it at once"
    );
    yield_to_tasks().await;
    assert_eq!(
        bench.arm(),
        Some(Value::F64(0.75)),
        "a write reaches the HAL unstepped"
    );
    web.write_values(&format!(r#"{{"{ARM}": {{"f64": 1.25}}}}"#))
        .expect("write the arm");
    yield_to_tasks().await;
    assert_eq!(
        bench.arm(),
        Some(Value::F64(1.25)),
        "a writeValues reaches the HAL too"
    );

    let ticks = bench.ticks();
    assert_eq!(
        ticks.iter().filter(|tick| **tick != ZERO_DT).count(),
        1,
        "every step but the page's own is at a zero dt: {ticks:?}"
    );
}

/// The operations and writes of one turn share one step at a zero dt, which
/// applies the operations in the order they were issued.
#[wasm_bindgen_test]
async fn a_turn_shares_one_step_applied_in_issue_order() {
    let bench = bench();
    let web = &bench.web;

    let first = web.call(&double_call(1.0));
    web.set_value("face/mouth", r#"{"f64": 0.5}"#)
        .expect("write");
    let second = web.call(&double_call(2.0));
    let third = web.invoke("double", r#"{"x": {"f64": 3.0}}"#, None);
    web.write_values(r#"{"face/brow": {"f64": 1.0}}"#)
        .expect("write");
    for (promise, what) in [(first, "first"), (second, "second"), (third, "third")] {
        settles(promise, what).await.expect("each answers");
    }

    assert_eq!(
        *bench.doubled.borrow(),
        [1.0, 2.0, 3.0],
        "applied in issue order"
    );
    assert_eq!(
        bench.arm(),
        Some(Value::F64(6.0)),
        "the last call's write wins"
    );
    assert_eq!(bench.ticks(), [ZERO_DT], "one step for the whole turn");
}

/// The operations that only read answer from the device's state with no step;
/// `invoke` reads its method's signature that way, so its call takes one step.
#[wasm_bindgen_test]
async fn reads_answer_without_a_step() {
    let bench = bench();
    let web = &bench.web;
    let keys = settles(web.list_keys(Some("arora/".to_string())), "listKeys")
        .await
        .expect("listKeys answers");
    assert!(json(keys).is_array());
    let methods = json(
        settles(
            web.describe_methods(Some("dou".to_string())),
            "describeMethods",
        )
        .await
        .expect("describeMethods answers"),
    );
    assert_eq!(methods[0]["path"], "double", "{methods}");
    assert_eq!(methods[0]["module"], TOOLS.to_string(), "{methods}");
    assert_eq!(methods[0]["params"][0]["name"], "x", "{methods}");
    assert_eq!(methods[0]["task"], false, "{methods}");
    assert_eq!(methods.as_array().map(Vec::len), Some(1), "{methods}");
    yield_to_tasks().await;
    assert_eq!(bench.ticks(), [], "a read takes no step");

    // A key the page wrote, listed with its meta.
    web.set_value("face/mouth", r#"{"f64": 0.5}"#)
        .expect("write");
    let keys = json(
        settles(web.list_keys(Some("face/".to_string())), "listKeys")
            .await
            .expect("listKeys answers"),
    );
    assert_eq!(keys[0]["path"], "face/mouth", "{keys}");
    assert!(keys[0]["__meta"].is_object(), "{keys}");

    yield_to_tasks().await;
    bench.ticks.borrow_mut().clear();
    settles(
        web.invoke("double", r#"{"x": {"f64": 2.0}}"#, None),
        "invoke",
    )
    .await
    .expect("invoke answers");
    assert_eq!(bench.ticks(), [ZERO_DT], "an invoke takes one step");
}

/// A name two modules export rejects naming both, until the module is named.
#[wasm_bindgen_test]
async fn invoke_of_a_shared_name_needs_its_module() {
    let bench = bench();
    let web = &bench.web;

    let error = jsval_to_string(
        settles(web.invoke("stop", "{}", None), "an ambiguous invoke")
            .await
            .expect_err("stop is ambiguous"),
    );
    assert!(error.contains(&TOOLS.to_string()), "{error}");
    assert!(error.contains(&PLAYER.to_string()), "{error}");

    let stopped = json(
        settles(web.invoke("stop", "{}", Some(PLAYER.to_string())), "invoke")
            .await
            .expect("the module disambiguates"),
    );
    assert_eq!(stopped, serde_json::json!({"str": "player stopped"}));
}

/// A page that steps in the same turn applies the operation with its own step:
/// no step at a zero dt follows.
#[wasm_bindgen_test]
async fn a_page_s_own_step_takes_the_zero_dt_step_s_place() {
    let bench = bench();
    let web = &bench.web;

    let called = web.call(&double_call(1.0));
    web.set_value("face/mouth", r#"{"f64": 0.5}"#)
        .expect("write");
    web.step(10.0).expect("step");
    settles(called, "call").await.expect("the call answers");
    yield_to_tasks().await;

    let ticks = bench.ticks();
    assert_eq!(ticks.len(), 1, "the page's step alone: {ticks:?}");
    assert_eq!(ticks[0].dt_ns, 10_000_000, "{ticks:?}");
}

/// Under `run()`, the run's next step applies what the page asks: no step at a
/// zero dt is added, and stopping takes none either.
#[wasm_bindgen_test]
async fn under_run_operations_apply_at_the_run_s_next_step() {
    let bench = bench();
    let web = &bench.web;

    let running = web.run(Some(20.0));
    sleep(50).await;
    let called = settles(web.call(&double_call(1.0)), "call under run").await;
    called.expect("the call answers");
    assert_eq!(
        bench.arm(),
        Some(Value::F64(2.0)),
        "the run's step wrote the HAL"
    );
    web.set_value(ARM, r#"{"f64": 0.75}"#).expect("write");
    let keys = settles(web.list_keys(None), "listKeys under run").await;
    assert!(json(keys.expect("listKeys answers")).is_array());
    sleep(50).await;
    assert_eq!(
        bench.arm(),
        Some(Value::F64(0.75)),
        "the run's step carried the write"
    );

    web.stop();
    settles(running, "the run").await.expect("the run stops");
    yield_to_tasks().await;
    let ticks = bench.ticks();
    assert!(ticks.len() >= 3, "the run stepped: {ticks:?}");
    assert!(
        ticks.iter().all(|tick| tick.running),
        "every step was the run's: {ticks:?}"
    );
}

/// An operation the run leaves unapplied as `stop()` ends it takes a step at a
/// zero dt once the device is back.
#[wasm_bindgen_test]
async fn an_operation_left_by_a_stopping_run_takes_a_zero_dt_step() {
    let bench = bench();
    let web = &bench.web;

    let running = web.run(Some(1_000.0));
    sleep(20).await;
    // Issued while the run sleeps out its period, the call waits for the run's
    // next step, which the stop then forestalls: only the run handing the
    // device back leaves the call a step.
    let called = web.call(&double_call(1.0));
    sleep(20).await;
    assert!(
        bench.doubled.borrow().is_empty(),
        "no run step applied it yet"
    );
    web.stop();
    settles(called, "a call the stopping run left")
        .await
        .expect("the call answers");
    settles(running, "the run").await.expect("the run stops");
    assert_eq!(*bench.doubled.borrow(), [1.0]);
}

/// A call into the surface from inside a step is queued, not nested: the step
/// at a zero dt after the current one applies it. A page's `step()` from
/// there is refused.
#[wasm_bindgen_test]
async fn a_call_from_inside_a_step_applies_at_the_step_after_it() {
    let bench = bench();
    let web = &bench.web;

    let issued: Rc<RefCell<Vec<js_sys::Promise>>> = Rc::default();
    let refused: Rc<RefCell<Option<String>>> = Rc::default();
    {
        let issued = issued.clone();
        let refused = refused.clone();
        let mut fired = false;
        *bench.hook.borrow_mut() = Some(Box::new(move |web: &AroraWeb| {
            if std::mem::replace(&mut fired, true) {
                return;
            }
            issued.borrow_mut().push(web.call(&double_call(4.0)));
            web.set_value("face/mouth", r#"{"f64": 0.5}"#)
                .expect("write");
            *refused.borrow_mut() = web.step(1.0).err().map(jsval_to_string);
            assert!(!web.running(), "a page's step is not a run");
        }));
    }

    web.step(10.0).expect("step");
    assert!(
        bench.doubled.borrow().is_empty(),
        "nothing nested in the step"
    );
    let called = issued.borrow_mut().pop().expect("the hook called");
    settles(called, "a call from inside a step")
        .await
        .expect("the call answers");
    assert_eq!(*bench.doubled.borrow(), [4.0]);
    let refused = refused
        .borrow()
        .clone()
        .expect("a step from inside a step is refused");
    assert!(refused.contains("mid-step"), "{refused}");
    let ticks = bench.ticks();
    assert_eq!(
        ticks.len(),
        2,
        "the page's step, then one at a zero dt: {ticks:?}"
    );
    assert_eq!(ticks[1], ZERO_DT, "{ticks:?}");
}

/// A write from inside a step is carried by that step, which asks for no
/// other: a callback writing on every tick adds no step.
#[wasm_bindgen_test]
async fn a_write_from_inside_a_step_is_carried_by_that_step() {
    let bench = bench();
    let web = &bench.web;
    *bench.hook.borrow_mut() = Some(Box::new(|web: &AroraWeb| {
        web.set_value(ARM, r#"{"f64": 0.875}"#).expect("write");
    }));

    web.step(10.0).expect("step");
    assert_eq!(
        bench.arm(),
        Some(Value::F64(0.875)),
        "the step wrote the HAL"
    );
    sleep(20).await;
    assert_eq!(
        bench.ticks().len(),
        1,
        "no step followed: {:?}",
        bench.ticks()
    );
}

/// A callback that issues an operation on every tick keeps the device stepping
/// at a zero dt, one step per task, and the page's timers still run.
#[wasm_bindgen_test]
async fn a_callback_asking_on_every_tick_does_not_starve_the_page() {
    let bench = bench();
    let web = &bench.web;
    *bench.hook.borrow_mut() = Some(Box::new(|web: &AroraWeb| {
        // Each step's call is applied by the next; its promise is left alone.
        let _ = web.call(&double_call(1.0));
    }));

    web.step(10.0).expect("step");
    sleep(30).await;
    *bench.hook.borrow_mut() = None;
    sleep(30).await;
    let ticks = bench.ticks();
    assert!(
        ticks.len() > 2,
        "the calls kept the device stepping: {ticks:?}"
    );
    assert!(
        ticks[1..].iter().all(|tick| *tick == ZERO_DT),
        "each at a zero dt: {ticks:?}"
    );
    let after = ticks.len();
    sleep(30).await;
    assert_eq!(bench.ticks().len(), after, "the chain ends with the hook");
}

/// A step that fails rejects the operations it left unanswered with its
/// error; an operation it did not reach is still applied by a later step.
#[wasm_bindgen_test]
async fn a_failed_step_rejects_what_it_left_unanswered() {
    let bench = bench();
    let web = &bench.web;
    bench.failing.store(true, Ordering::Relaxed);

    let unstepped = settles(web.call(&double_call(1.0)), "a call on a failing device").await;
    let error = jsval_to_string(unstepped.expect_err("the zero-dt step fails"));
    assert!(error.contains("step failed"), "{error}");
    assert!(error.contains("the store refuses writes"), "{error}");

    let stepped = web.call(&double_call(2.0));
    let error = jsval_to_string(web.step(10.0).expect_err("the page's step fails"));
    assert!(error.contains("step failed"), "{error}");
    let error = jsval_to_string(
        settles(stepped, "a call the page's failing step left")
            .await
            .expect_err("it rejects with the step's error"),
    );
    assert!(error.contains("step failed"), "{error}");

    bench.failing.store(false, Ordering::Relaxed);
    web.step(10.0).expect("step");
    assert_eq!(
        *bench.doubled.borrow(),
        [1.0, 2.0],
        "a rejected operation the failed steps did not reach is applied later"
    );
}

/// Under `run()`, a call from inside a run step applies at the run's next
/// step.
#[wasm_bindgen_test]
async fn a_call_from_inside_a_run_step_applies_at_the_next_one() {
    let bench = bench();
    let web = &bench.web;

    let issued: Rc<RefCell<Vec<js_sys::Promise>>> = Rc::default();
    {
        let issued = issued.clone();
        let mut fired = false;
        *bench.hook.borrow_mut() = Some(Box::new(move |web: &AroraWeb| {
            if std::mem::replace(&mut fired, true) {
                return;
            }
            issued.borrow_mut().push(web.call(&double_call(4.0)));
        }));
    }

    let running = web.run(Some(20.0));
    sleep(10).await;
    let called = issued.borrow_mut().pop().expect("the hook called");
    settles(called, "a call from inside a run step")
        .await
        .expect("the call answers");
    web.stop();
    settles(running, "the run").await.expect("the run stops");
    assert_eq!(*bench.doubled.borrow(), [4.0]);
    let ticks = bench.ticks();
    assert!(
        ticks.iter().all(|tick| tick.running),
        "no step but the run's: {ticks:?}"
    );
}

/// `request_step` on a device nobody steps takes one step at a zero dt, and
/// one turn's requests share it with the turn's operations and writes.
#[wasm_bindgen_test]
async fn a_requested_step_is_shared_with_the_turn_s_operations() {
    let bench = bench();
    let web = &bench.web;

    web.request_step();
    web.request_step();
    yield_to_tasks().await;
    assert_eq!(bench.ticks(), [ZERO_DT], "one step for both requests");

    bench.ticks.borrow_mut().clear();
    web.request_step();
    let called = web.call(&double_call(1.0));
    web.set_value("face/mouth", r#"{"f64": 0.5}"#)
        .expect("write");
    web.request_step();
    settles(called, "call").await.expect("the call answers");
    yield_to_tasks().await;
    assert_eq!(*bench.doubled.borrow(), [1.0]);
    assert_eq!(bench.ticks(), [ZERO_DT], "one step for the whole turn");

    bench.ticks.borrow_mut().clear();
    web.request_step();
    web.step(10.0).expect("step");
    yield_to_tasks().await;
    let ticks = bench.ticks();
    assert_eq!(ticks.len(), 1, "the page's step alone: {ticks:?}");
    assert_eq!(ticks[0].dt_ns, 10_000_000, "{ticks:?}");
}

/// Under `run()`, `request_step` adds no step: the run's steps are the only
/// ones, and stopping the run takes none at a zero dt.
#[wasm_bindgen_test]
async fn a_requested_step_under_run_adds_none() {
    let bench = bench();
    let web = &bench.web;

    let running = web.run(Some(20.0));
    sleep(30).await;
    for _ in 0..3 {
        web.request_step();
        yield_to_tasks().await;
    }
    sleep(30).await;
    web.stop();
    settles(running, "the run").await.expect("the run stops");
    sleep(20).await;
    let ticks = bench.ticks();
    assert!(ticks.len() >= 2, "the run stepped: {ticks:?}");
    assert!(
        ticks.iter().all(|tick| tick.running),
        "every step was the run's: {ticks:?}"
    );
}

/// `request_step` from inside a step nests none: the step it asks for is
/// taken as a task once the current one has ended.
#[wasm_bindgen_test]
async fn a_step_requested_from_inside_a_step_follows_it() {
    let bench = bench();
    let web = &bench.web;

    let ticks_in_hook: Rc<RefCell<Vec<usize>>> = Rc::default();
    {
        let ticks = bench.ticks.clone();
        let ticks_in_hook = ticks_in_hook.clone();
        let mut fired = false;
        *bench.hook.borrow_mut() = Some(Box::new(move |web: &AroraWeb| {
            if std::mem::replace(&mut fired, true) {
                return;
            }
            web.request_step();
            web.request_step();
            ticks_in_hook.borrow_mut().push(ticks.borrow().len());
        }));
    }

    web.step(10.0).expect("step");
    assert_eq!(*ticks_in_hook.borrow(), [1], "nothing nested in the step");
    yield_to_tasks().await;
    assert_eq!(bench.ticks().len(), 1, "no step in the step's microtasks");
    sleep(20).await;
    let ticks = bench.ticks();
    assert_eq!(
        ticks.len(),
        2,
        "the page's step, then one at a zero dt: {ticks:?}"
    );
    assert_eq!(ticks[1], ZERO_DT, "{ticks:?}");
}

// =============================================================================
// Draining the store's changes
// =============================================================================

/// A demo device and a sibling handle of its store, through which a test
/// writes as the device's own writers do.
fn device_and_store() -> (AroraWeb, Box<dyn DataStore>) {
    let arora = Arora::builder().build().expect("build the device");
    let store = arora.store().clone_box();
    (AroraWeb::from(arora), store)
}

fn value_json(value: Value) -> serde_json::Value {
    serde_json::to_value(value).expect("a value serializes")
}

/// A drain holds the net change since the previous one: a key set twice reads
/// its second value, a key set then unset reads cleared.
#[wasm_bindgen_test]
fn a_drain_holds_the_net_change_since_the_previous_one() {
    let (web, store) = device_and_store();
    web.drain_changes().expect("the first drain");

    let set = |path: &str, value: f64| {
        store
            .write(StateChange::set(path, Value::F64(value)))
            .expect("set a key");
    };
    set("face/mouth", 0.25);
    set("face/brow", 1.5);
    web.step(10.0).expect("step");
    set("face/mouth", 0.5);
    set("face/brow", 2.5);
    web.step(10.0).expect("step");
    let mut unset = StateChange::new();
    unset.unset.insert(Key::new("face/brow"));
    store.write(unset).expect("unset a key");
    web.step(10.0).expect("step");

    let drained = json(web.drain_changes().expect("drain"));
    assert_eq!(
        drained["face/mouth"],
        value_json(Value::F64(0.5)),
        "{drained}"
    );
    assert_eq!(drained["face/brow"], serde_json::Value::Null, "{drained}");

    let again = json(web.drain_changes().expect("drain again"));
    assert_eq!(
        again,
        serde_json::json!({}),
        "nothing changed since: {again}"
    );
}

/// The first drain holds the store's whole state, the keys written before the
/// device was wrapped included.
#[wasm_bindgen_test]
fn the_first_drain_holds_the_full_state() {
    let arora = Arora::builder().build().expect("build the device");
    let mut before = StateChange::new();
    before
        .set
        .insert(Key::new("face/mouth"), Some(Value::F64(0.25)));
    before
        .set
        .insert(Key::new("face/brow"), Some(Value::F64(1.5)));
    arora.store().write(before).expect("write before wrapping");
    let web = AroraWeb::from(arora);
    for _ in 0..3 {
        web.step(10.0).expect("step");
    }

    let drained = json(web.drain_changes().expect("the first drain"));
    let snapshot = json(web.snapshot().expect("snapshot"));
    assert_eq!(drained, snapshot, "the first drain is the store's state");
    assert_eq!(
        drained["face/mouth"],
        value_json(Value::F64(0.25)),
        "{drained}"
    );
    assert_eq!(
        drained["face/brow"],
        value_json(Value::F64(1.5)),
        "{drained}"
    );
}

fn jsval_to_string(v: JsValue) -> String {
    v.as_string().unwrap_or_else(|| format!("{v:?}"))
}
