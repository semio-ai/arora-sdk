//! `bridge-bench`: serve a device over the Unix bridge or the Zenoh bridge,
//! check what a client can count on through either, and measure them.
//!
//! - `serve` runs a device: `device`, the Arora runtime over a fake HAL of
//!   [`JOINTS`] joints, a `math` module and the open subtree `cmd`; or
//!   `standin`, which answers each request the moment it arrives and pushes
//!   the joints' state at a rate a client sets, so a measure is the
//!   transport's and the bridge's alone, not the step's.
//! - `check` runs the functional checks against a `device`.
//! - `bench` measures against a `standin`, printing one JSON object.
//!
//! A transport is `--unix PATH` or `--zenoh ENDPOINT` (`tcp/HOST:PORT`,
//! `unixsock-stream/PATH`), on both sides; an ssh forward sits between them
//! unseen.

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{bail, ensure, Context};
use arora::{Arora, ModuleBuilder};
use arora_bridge::{Bridge, BridgeOp};
use arora_bridge_unix::{Changes, UnixBridge, UnixClient};
use arora_bridge_zenoh::{ZenohBridge, ZenohClient};
use arora_hal::FakeHal;
use arora_simple_data_store::SimpleDataStore;
use arora_types::call::{Call, CallError, CallResult};
use arora_types::data::{DataStore, Key, KeyMeta, StateChange};
use arora_types::value::{StructureField, Value};
use arora_types::Uuid;
use clap::{Parser, Subcommand, ValueEnum};
use futures::StreamExt;
use serde_json::json;

/// NAO's joints.
const JOINTS: usize = 25;
const PREFIX: &str = "arora/bench";
const MATH: Uuid = Uuid::from_u128(0x6d61_7468);
const DOUBLE: Uuid = Uuid::from_u128(0x6d61_7468_0001);
const DOUBLE_X: Uuid = Uuid::from_u128(0x6d61_7468_0002);

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Args, Clone)]
struct Transport {
    /// The Unix bridge's socket file.
    #[arg(long, conflicts_with = "zenoh")]
    unix: Option<std::path::PathBuf>,
    /// A Zenoh endpoint: the device listens there, a client connects.
    #[arg(long, conflicts_with = "stdio")]
    zenoh: Option<String>,
    /// A client only: a shell command whose standard streams are joined to the
    /// Unix bridge, such as `ssh robot python relay.py SOCKET`.
    #[arg(long, conflicts_with = "unix")]
    stdio: Option<String>,
}

#[derive(Clone, Copy, ValueEnum)]
enum Mode {
    Device,
    Standin,
}

#[derive(Subcommand)]
enum Command {
    Serve {
        #[command(flatten)]
        transport: Transport,
        #[arg(long, value_enum, default_value = "device")]
        mode: Mode,
        /// The device's step period.
        #[arg(long, default_value_t = 10)]
        period_ms: u64,
    },
    Check {
        #[command(flatten)]
        transport: Transport,
    },
    Bench {
        #[command(flatten)]
        transport: Transport,
        /// A name for the run, copied to the output.
        #[arg(long, default_value = "")]
        label: String,
        /// Round trips per measured phase.
        #[arg(long, default_value_t = 3000, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
        n: usize,
        /// Seconds of each push phase.
        #[arg(long, default_value_t = 5)]
        push_s: u64,
    },
    /// The step-bound round trip of a call through a `device`.
    DeviceRtt {
        #[command(flatten)]
        transport: Transport,
        #[arg(long, default_value = "")]
        label: String,
        #[arg(long, default_value_t = 300, value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..))]
        n: usize,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    match Cli::parse().command {
        Command::Serve {
            transport,
            mode,
            period_ms,
        } => {
            // Stop on SIGTERM or ^C by dropping the device, so its bridge removes
            // its socket file.
            let mut term =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
            tokio::select! {
                served = serve(transport, mode, Duration::from_millis(period_ms)) => served,
                _ = term.recv() => Ok(()),
                _ = tokio::signal::ctrl_c() => Ok(()),
            }
        }
        Command::Check { transport } => check(&transport).await,
        Command::Bench {
            transport,
            label,
            n,
            push_s,
        } => bench(&transport, &label, n, Duration::from_secs(push_s)).await,
        Command::DeviceRtt {
            transport,
            label,
            n,
        } => device_rtt(&transport, &label, n).await,
    }
}

async fn bridge_of(transport: &Transport) -> anyhow::Result<Box<dyn Bridge>> {
    Ok(match (&transport.unix, &transport.zenoh) {
        (Some(path), None) => Box::new(UnixBridge::bind(path).context("bind the unix bridge")?),
        (None, Some(endpoint)) => {
            let session = zenoh::open(zenoh_config(Some(endpoint), None)?)
                .await
                .map_err(|e| anyhow::anyhow!("zenoh open: {e}"))?;
            Box::new(
                ZenohBridge::new(&session, PREFIX)
                    .await
                    .map_err(|e| anyhow::anyhow!("the zenoh bridge: {e}"))?,
            )
        }
        _ => bail!("a device serves one of --unix and --zenoh"),
    })
}

/// A Zenoh configuration with no scouting: a peer that listens on `listen`, or
/// a client that connects to `connect`.
fn zenoh_config(listen: Option<&str>, connect: Option<&str>) -> anyhow::Result<zenoh::Config> {
    let mut config = zenoh::Config::default();
    let mut insert = |key: &str, value: String| {
        config
            .insert_json5(key, &value)
            .map_err(|e| anyhow::anyhow!("zenoh config {key}: {e}"))
    };
    insert("scouting/multicast/enabled", "false".into())?;
    insert("scouting/gossip/enabled", "false".into())?;
    if let Some(listen) = listen {
        insert("mode", r#""peer""#.into())?;
        insert("listen/endpoints", format!(r#"["{listen}"]"#))?;
    }
    if let Some(connect) = connect {
        insert("mode", r#""client""#.into())?;
        insert("connect/endpoints", format!(r#"["{connect}"]"#))?;
    }
    Ok(config)
}

fn joint_names() -> Vec<String> {
    (0..JOINTS).map(|j| format!("j{j:02}")).collect()
}

async fn serve(transport: Transport, mode: Mode, period: Duration) -> anyhow::Result<()> {
    let bridge = bridge_of(&transport).await?;
    eprintln!("serving");
    match mode {
        Mode::Device => {
            let hal = FakeHal::new();
            hal.add_joints(joint_names());
            let store = SimpleDataStore::new();
            store
                .set_prefix_meta(HashMap::from([(
                    "cmd".to_string(),
                    KeyMeta::new().editable(),
                )]))
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let mut device = Arora::builder()
                .with_hal(Box::new(hal))
                .with_data_store(Box::new(store))
                .with_host_module(math())
                .with_bridge(bridge)
                .build()?;
            device.run(period).await?;
            Ok(())
        }
        Mode::Standin => standin(bridge).await,
    }
}

fn math() -> arora::HostModule {
    use arora_types::record::module::frozen::{Function, Parameter};
    use arora_types::record::ty::{FrozenTy, PrimitiveKind};
    let signature = Function {
        parameters: HashMap::from([(
            DOUBLE_X,
            Parameter {
                name: "x".to_string(),
                ty: FrozenTy::from(PrimitiveKind::F64),
                mutable: false,
            },
        )]),
        parameter_ordering: vec![DOUBLE_X],
        return_ty: FrozenTy::from(PrimitiveKind::F64),
    };
    ModuleBuilder::new(MATH)
        .described_function(DOUBLE, "double", signature, |call: Call| double_of(&call))
        .build()
}

fn double_of(call: &Call) -> Result<CallResult, CallError> {
    match call
        .args
        .iter()
        .find(|arg| arg.id == DOUBLE_X)
        .map(|a| &*a.value)
    {
        Some(Value::F64(x)) => Ok(CallResult {
            ret: Value::F64(2.0 * x),
            mutated: Vec::new(),
        }),
        other => Err(CallError::Generic {
            message: format!("x is not an f64: {other:?}"),
        }),
    }
}

fn double(x: f64) -> Call {
    Call {
        module_id: Some(MATH),
        id: DOUBLE,
        args: vec![StructureField {
            id: DOUBLE_X,
            value: Box::new(Value::F64(x)),
        }],
    }
}

/// CPU time of this process, all its threads, in nanoseconds.
fn cpu_ns() -> u64 {
    std::fs::read_dir("/proc/self/task")
        .map(|tasks| {
            tasks
                .flatten()
                .filter_map(|task| std::fs::read_to_string(task.path().join("schedstat")).ok())
                .filter_map(|s| s.split_whitespace().next()?.parse::<u64>().ok())
                .sum()
        })
        .unwrap_or(0)
}

fn rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))?
                .split_whitespace()
                .nth(1)?
                .parse()
                .ok()
        })
        .unwrap_or(0)
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

/// Answer each request the moment it comes; push the joints at the rate the
/// key `bench/push_hz` sets.
async fn standin(mut bridge: Box<dyn Bridge>) -> anyhow::Result<()> {
    let mut inbound = bridge.take_inbound();
    let mut push: Option<tokio::time::Interval> = None;
    let names: Vec<Key> = joint_names()
        .into_iter()
        .map(|j| Key::from(format!("joints/{j}")))
        .collect();
    let mut step = 0u64;
    loop {
        tokio::select! {
            event = inbound.next() => {
                let Some(event) = event else { return Ok(()) };
                let arora_bridge::Inbound::Command(command) = event else { continue };
                let result = match &command.op {
                    BridgeOp::Call(call) if call.id == DOUBLE => double_of(call).map(|r| r.ret).map_err(|e| format!("{e:?}")),
                    BridgeOp::Call(_) => Err("no such function".into()),
                    BridgeOp::Update(change) => {
                        if let Some(Some(Value::F64(hz))) = change.set.get(&Key::from("bench/push_hz")) {
                            push = (*hz > 0.0).then(|| {
                                let mut interval = tokio::time::interval(Duration::from_secs_f64(1.0 / hz));
                                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                                interval
                            });
                        }
                        Ok(Value::Unit)
                    }
                    BridgeOp::Get(keys) => Ok(Value::ArrayValue(keys.iter().map(|k| Value::Option(match k.path.as_str() {
                        "bench/cpu_ns" => Some(Box::new(Value::U64(cpu_ns()))),
                        "bench/rss_kb" => Some(Box::new(Value::U64(rss_kb()))),
                        _ => None,
                    })).collect())),
                    _ => Ok(Value::ArrayValue(vec![])),
                };
                command.reply(result.map(|ret| CallResult { ret, mutated: vec![] }));
            }
            _ = async { push.as_mut().unwrap().tick().await }, if push.is_some() => {
                step += 1;
                let mut change = StateChange::new();
                for (j, key) in names.iter().enumerate() {
                    change.set.insert(key.clone(), Some(Value::F32((step as f32) * 0.001 + j as f32)));
                }
                change.set.insert(Key::from("joints/stamp"), Some(Value::U64(now_ns())));
                bridge.try_send(&change);
            }
        }
    }
}

/// A client over either transport.
enum Client {
    Unix(UnixClient, Changes),
    /// Over a command's standard streams; the child, held to be killed on
    /// drop, ends with the client.
    Stdio(
        UnixClient,
        Changes,
        #[allow(dead_code)] tokio::process::Child,
    ),
    Zenoh(ZenohClient, arora_bridge_zenoh::Changes),
}

impl Client {
    async fn connect(transport: &Transport) -> anyhow::Result<Self> {
        if let Some(command) = &transport.stdio {
            let mut child = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .with_context(|| format!("run {command}"))?;
            let (client, changes) = UnixClient::over(
                child.stdout.take().context("stdout")?,
                child.stdin.take().context("stdin")?,
            );
            return Ok(Client::Stdio(client, changes, child));
        }
        Ok(match (&transport.unix, &transport.zenoh) {
            (Some(path), None) => {
                let (client, changes) = UnixClient::connect(path)
                    .await
                    .with_context(|| format!("connect to {}", path.display()))?;
                Client::Unix(client, changes)
            }
            (None, Some(endpoint)) => {
                let session = zenoh::open(zenoh_config(None, Some(endpoint))?)
                    .await
                    .map_err(|e| anyhow::anyhow!("zenoh open: {e}"))?;
                let (client, changes) = ZenohClient::new(&session, PREFIX)
                    .await
                    .map_err(|e| anyhow::anyhow!("the zenoh client: {e}"))?;
                Client::Zenoh(client, changes)
            }
            _ => bail!("give one of --unix, --stdio and --zenoh"),
        })
    }

    async fn ask(&self, op: BridgeOp) -> Result<CallResult, String> {
        match self {
            Client::Unix(client, _) | Client::Stdio(client, ..) => client.ask(op).await,
            Client::Zenoh(client, ..) => client.ask(op).await,
        }
    }

    async fn subscribe(&mut self, prefixes: Vec<String>) -> anyhow::Result<()> {
        match self {
            Client::Unix(client, _) | Client::Stdio(client, ..) => {
                client.subscribe(prefixes).await.map_err(anyhow::Error::msg)
            }
            Client::Zenoh(client, _) => {
                client.subscribe(prefixes).await.map_err(anyhow::Error::msg)
            }
        }
    }

    async fn next_change(&mut self, within: Duration) -> Option<StateChange> {
        let next = async {
            match self {
                Client::Unix(_, changes) | Client::Stdio(_, changes, _) => changes.next().await,
                Client::Zenoh(_, changes) => changes.next().await,
            }
        };
        tokio::time::timeout(within, next).await.ok().flatten()
    }

    async fn write(&self, key: &str, value: Value) -> Result<CallResult, String> {
        self.ask(BridgeOp::Update(StateChange::set(key, value)))
            .await
    }

    async fn read(&self, key: &str) -> anyhow::Result<Option<Value>> {
        let read = self
            .ask(BridgeOp::Get(vec![Key::from(key)]))
            .await
            .map_err(anyhow::Error::msg)?;
        match read.ret {
            Value::ArrayValue(mut items) if items.len() == 1 => match items.pop() {
                Some(Value::Option(value)) => Ok(value.map(|v| *v)),
                other => bail!("not an option: {other:?}"),
            },
            other => bail!("not one value: {other:?}"),
        }
    }
}

macro_rules! pass {
    ($name:expr, $check:expr) => {{
        let started = Instant::now();
        match $check {
            Ok(()) => println!("PASS {} ({:?})", $name, started.elapsed()),
            Err(e) => {
                println!("FAIL {}: {e:#}", $name);
                std::process::exit(1);
            }
        }
    }};
}

async fn check(transport: &Transport) -> anyhow::Result<()> {
    let mut client = Client::connect(transport).await?;
    pass!(
        "list_keys shows the HAL's inputs open, its outputs closed",
        async {
            let listed = client
                .ask(BridgeOp::ListKeys {
                    prefix: Some("j00".into()),
                })
                .await
                .map_err(anyhow::Error::msg)?;
            let keys: Vec<(String, KeyMeta)> = arora_types::value_serde::from_value(listed.ret)?;
            let keys: Vec<(&str, bool)> =
                keys.iter().map(|(p, m)| (p.as_str(), m.editable)).collect();
            ensure!(
                keys == [
                    ("j00.position", false),
                    ("j00.target_position", true),
                    ("j00/target.position", true)
                ],
                "{keys:?}"
            );
            Ok(())
        }
        .await
    );
    pass!(
        "describe_methods names double under its ids",
        async {
            let described = client
                .ask(BridgeOp::DescribeMethods { prefix: None })
                .await
                .map_err(anyhow::Error::msg)?;
            let methods: Vec<arora::MethodSignature> =
                arora_types::value_serde::from_value(described.ret)?;
            ensure!(methods
                .iter()
                .any(|m| m.name == "double" && m.id == DOUBLE && m.module_id == MATH));
            Ok(())
        }
        .await
    );
    pass!(
        "a call by ids answers",
        async {
            let answer = client
                .ask(BridgeOp::Call(double(21.0)))
                .await
                .map_err(anyhow::Error::msg)?;
            ensure!(answer.ret == Value::F64(42.0), "{answer:?}");
            Ok(())
        }
        .await
    );
    pass!(
        "a write to an open key holds",
        async {
            client
                .write("cmd/head_yaw", Value::F32(0.3))
                .await
                .map_err(anyhow::Error::msg)?;
            ensure!(client.read("cmd/head_yaw").await? == Some(Value::F32(0.3)));
            Ok(())
        }
        .await
    );
    pass!(
        "a write to an output is refused with the reason",
        async {
            let error = client
                .write("j00.position", Value::F64(1.0))
                .await
                .err()
                .context("refused")?;
            ensure!(error.contains("not an input"), "{error}");
            Ok(())
        }
        .await
    );
    pass!(
        "a target moves the HAL, and the subscriber is pushed it",
        async {
            // A target no earlier run left: an unchanged key is not pushed.
            let target = (now_ns() % 1_000_000) as f64 / 1e6;
            client.subscribe(vec!["j00.position".into()]).await?;
            client
                .write("j00.target_position", Value::F64(target))
                .await
                .map_err(anyhow::Error::msg)?;
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                ensure!(
                    Instant::now() < deadline,
                    "no push of j00.position = {target}"
                );
                if let Some(change) = client.next_change(Duration::from_secs(5)).await {
                    if change.set.get(&Key::from("j00.position")) == Some(&Some(Value::F64(target)))
                    {
                        break;
                    }
                }
            }
            client.subscribe(vec![]).await?;
            Ok(())
        }
        .await
    );
    pass!(
        "an 8 MB value goes and comes back whole",
        async {
            let big = Value::ArrayF32((0..2_000_000).map(|i| i as f32).collect());
            client
                .write("cmd/big", big.clone())
                .await
                .map_err(anyhow::Error::msg)?;
            ensure!(client.read("cmd/big").await? == Some(big));
            Ok(())
        }
        .await
    );
    pass!(
        "500 calls in flight each get their own answer",
        async {
            let answers = futures::future::join_all(
                (0..500).map(|i| client.ask(BridgeOp::Call(double(i as f64)))),
            )
            .await;
            for (i, answer) in answers.into_iter().enumerate() {
                let answer = answer.map_err(anyhow::Error::msg)?;
                ensure!(answer.ret == Value::F64(2.0 * i as f64), "{i}: {answer:?}");
            }
            Ok(())
        }
        .await
    );
    Ok(())
}

fn stats(mut samples: Vec<f64>) -> serde_json::Value {
    samples.sort_by(f64::total_cmp);
    let at = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
    json!({
        "n": samples.len(),
        "p50_us": at(0.5), "p90_us": at(0.9), "p99_us": at(0.99), "max_us": at(1.0),
        "mean_us": samples.iter().sum::<f64>() / samples.len() as f64,
    })
}

async fn server_cpu_ns(client: &Client) -> anyhow::Result<u64> {
    match client.read("bench/cpu_ns").await? {
        Some(Value::U64(ns)) => Ok(ns),
        other => bail!("the stand-in's cpu: {other:?}"),
    }
}

/// Run `n` sequential round trips of `op`, returning their latencies and the
/// CPU each side spent per round trip.
async fn round_trips(
    client: &Client,
    n: usize,
    op: impl Fn(usize) -> BridgeOp,
) -> anyhow::Result<serde_json::Value> {
    for i in 0..n / 10 {
        client.ask(op(i)).await.map_err(anyhow::Error::msg)?;
    }
    let (server0, client0) = (server_cpu_ns(client).await?, cpu_ns());
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let started = Instant::now();
        client.ask(op(i)).await.map_err(anyhow::Error::msg)?;
        samples.push(started.elapsed().as_secs_f64() * 1e6);
    }
    let (server1, client1) = (server_cpu_ns(client).await?, cpu_ns());
    let mut out = stats(samples);
    out["server_cpu_us_per_op"] = json!((server1 - server0) as f64 / n as f64 / 1e3);
    out["client_cpu_us_per_op"] = json!((client1 - client0) as f64 / n as f64 / 1e3);
    Ok(out)
}

async fn bench(
    transport: &Transport,
    label: &str,
    n: usize,
    push_for: Duration,
) -> anyhow::Result<()> {
    let connect_started = Instant::now();
    let mut client = Client::connect(transport).await?;
    // The first round trip completes the connection (Zenoh's session joins).
    client
        .ask(BridgeOp::Call(double(1.0)))
        .await
        .map_err(anyhow::Error::msg)?;
    let connect_ms = connect_started.elapsed().as_secs_f64() * 1e3;

    let call = round_trips(&client, n, |i| BridgeOp::Call(double(i as f64))).await?;
    let command = round_trips(&client, n, |i| {
        let mut change = StateChange::new();
        for j in 0..JOINTS {
            change.set.insert(
                Key::from(format!("cmd/j{j:02}")),
                Some(Value::F32(i as f32 + j as f32)),
            );
        }
        BridgeOp::Update(change)
    })
    .await?;
    let big = Value::ArrayF32((0..250_000).map(|i| i as f32).collect());
    let megabyte = round_trips(&client, 50, |_| {
        BridgeOp::Update(StateChange::set("cmd/big", big.clone()))
    })
    .await?;

    // Calls with 64 in flight.
    let pipelined_n = n * 4;
    let started = Instant::now();
    let mut in_flight = futures::stream::iter(
        (0..pipelined_n).map(|i| client.ask(BridgeOp::Call(double(i as f64)))),
    )
    .buffer_unordered(64);
    while let Some(answer) = in_flight.next().await {
        answer.map_err(anyhow::Error::msg)?;
    }
    drop(in_flight);
    let pipelined_per_s = pipelined_n as f64 / started.elapsed().as_secs_f64();

    let mut push = serde_json::Map::new();
    client.subscribe(vec!["joints".into()]).await?;
    for hz in [100.0, 1000.0] {
        client
            .write("bench/push_hz", Value::F64(hz))
            .await
            .map_err(anyhow::Error::msg)?;
        let (server0, client0) = (server_cpu_ns(&client).await?, cpu_ns());
        let started = Instant::now();
        let (mut latencies, mut values) = (Vec::new(), 0usize);
        while started.elapsed() < push_for {
            let Some(change) = client.next_change(Duration::from_secs(2)).await else {
                break;
            };
            values += change.set.len();
            if let Some(Some(Value::U64(stamp))) = change.set.get(&Key::from("joints/stamp")) {
                latencies.push(now_ns().saturating_sub(*stamp) as f64 / 1e3);
            }
        }
        let elapsed = started.elapsed().as_secs_f64();
        let (server1, client1) = (server_cpu_ns(&client).await?, cpu_ns());
        client
            .write("bench/push_hz", Value::F64(0.0))
            .await
            .map_err(anyhow::Error::msg)?;
        // Drain what was sent before the stop.
        while client
            .next_change(Duration::from_millis(300))
            .await
            .is_some()
        {}
        let steps = latencies.len();
        let mut out = if steps > 0 {
            stats(latencies)
        } else {
            json!({ "n": 0 })
        };
        out["steps_per_s"] = json!(steps as f64 / elapsed);
        out["expected_per_s"] = json!(hz);
        out["values_per_step"] = json!(values as f64 / steps.max(1) as f64);
        // The device's CPU per step it produced (the rate it was set, for the
        // time measured); a bridge that skips values on a slow reader sends
        // fewer stamps than it was given steps.
        let produced = (hz * elapsed).max(1.0);
        out["server_cpu_us_per_step"] = json!((server1 - server0) as f64 / produced / 1e3);
        out["client_cpu_us_per_step"] =
            json!((client1 - client0) as f64 / steps.max(1) as f64 / 1e3);
        push.insert(format!("{hz}hz"), out);
    }
    let server_rss_kb = match client.read("bench/rss_kb").await? {
        Some(Value::U64(kb)) => kb,
        _ => 0,
    };
    println!(
        "{}",
        json!({
            "label": label,
            "connect_ms": connect_ms,
            "call_rtt": call,
            "command_25_keys_rtt": command,
            "update_1MB_rtt": megabyte,
            "pipelined_calls_per_s": pipelined_per_s,
            "push": push,
            "server_rss_kb": server_rss_kb,
            "client_rss_kb": rss_kb(),
        })
    );
    Ok(())
}

async fn device_rtt(transport: &Transport, label: &str, n: usize) -> anyhow::Result<()> {
    let client = Client::connect(transport).await?;
    client
        .ask(BridgeOp::Call(double(1.0)))
        .await
        .map_err(anyhow::Error::msg)?;
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let started = Instant::now();
        client
            .ask(BridgeOp::Call(double(i as f64)))
            .await
            .map_err(anyhow::Error::msg)?;
        samples.push(started.elapsed().as_secs_f64() * 1e6);
    }
    println!(
        "{}",
        json!({ "label": label, "device_call_rtt": stats(samples) })
    );
    Ok(())
}
