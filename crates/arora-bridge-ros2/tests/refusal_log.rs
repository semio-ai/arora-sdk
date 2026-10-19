//! The bridge logs the device's refusal of a write from an input topic: the
//! one trace a ROS 2 writer has of why the robot does not move.
//!
//! The device is a stand-in. It lists one `F64` input, `face/mouth/open`, so
//! the bridge subscribes its topic, and refuses every write to it, as a device
//! that closed the key since would. A ROS 2 peer publishes on that topic. The
//! bridge logs the refusals at `warn`, naming the topic, the keys and the
//! device's reason, once per interval for one topic and set of keys; the
//! interval itself is unit-tested on `Refusals` in `bridge.rs`.
//!
//! A process has one logger, and this test installs one that records what the
//! bridge logs, so the test is a binary of its own. The peer joins the
//! bridge's graph the way [`peer`] states for each backend.

mod peer;

use std::sync::Mutex;
use std::time::Duration;

use arora_bridge::{Bridge, BridgeOp, Inbound};
use arora_bridge_ros2::conversions::topic_name;
use arora_bridge_ros2::msg_types;
use arora_bridge_ros2::{Ros2Bridge, Ros2BridgeConfig, Type};
use arora_types::call::CallResult;
use arora_types::data::KeyMeta;
use futures::StreamExt;
use log::{Level, LevelFilter, Log, Metadata, Record};
use peer::Peer;
use rand::RngExt;

const KEY: &str = "face/mouth/open";
const REASON: &str = "not an input of this device: face/mouth/open";

/// Every record the bridge logs, at any level, with its message.
struct Capture {
    records: Mutex<Vec<(Level, String)>>,
}

static CAPTURE: Capture = Capture {
    records: Mutex::new(Vec::new()),
};

impl Log for Capture {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.target().starts_with("arora_bridge_ros2")
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            self.records
                .lock()
                .unwrap()
                .push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

/// The bridge's records that report a refusal, whatever their level.
fn refusal_records() -> Vec<(Level, String)> {
    CAPTURE
        .records
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, message)| message.contains("refused a write"))
        .cloned()
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    all(feature = "dds", target_os = "macos"),
    ignore = "DDS multicast SPDP discovery is unreliable on macOS loopback (rustdds 0.11 \
              has no unicast-peer/interface config); runs on Linux CI. To run locally, \
              ensure an active multicast-capable interface and use `--ignored`."
)]
async fn a_refused_write_from_an_input_topic_is_logged_once_per_interval() {
    log::set_logger(&CAPTURE).expect("the test's logger is the process's first");
    log::set_max_level(LevelFilter::Trace);

    let domain_id = rand::rng().random_range(1..=200);
    let namespace = format!("refusal_{domain_id}_{}", std::process::id());
    let topic = topic_name(&namespace, KEY);

    let mut peer = Peer::join(domain_id);
    let mut bridge = Ros2Bridge::new(Ros2BridgeConfig::new(&namespace, domain_id)).await;
    let mut inbound = bridge.take_inbound();

    // The device answers concurrently, because the bridge asks for its inputs
    // before it subscribes them. It tells the test of each write it refuses.
    let (refused_tx, mut refused) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(event) = inbound.next().await {
            let Inbound::Command(cmd) = event else {
                continue; // DataRequested and other non-command signals
            };
            match &cmd.op {
                BridgeOp::ListKeys { .. } => {
                    let inputs = vec![(
                        KEY.to_string(),
                        KeyMeta::new().editable().of_type(Type::F64),
                    )];
                    cmd.reply(
                        arora_types::value_serde::to_value(&inputs)
                            .map(|ret| CallResult {
                                ret,
                                mutated: Vec::new(),
                            })
                            .map_err(|e| e.to_string()),
                    );
                }
                BridgeOp::Update(_) => {
                    cmd.reply(Err(REASON.to_string()));
                    let _ = refused_tx.send(());
                }
                // DescribeMethods and the rest: this device describes none.
                _ => {}
            }
        }
    });

    // Refusals arrive ten times a second, far within one log interval, from
    // the first message that reaches the bridge's subscription.
    let feed = peer.feed(&topic, |n| msg_types::Float64 { data: f64::from(n) });
    let mut writes = 0;
    while writes < 5 {
        tokio::time::timeout(Duration::from_secs(30), refused.recv())
            .await
            .expect("timed out waiting for a write on the input topic")
            .expect("the device task stopped before a write arrived");
        writes += 1;
    }
    feed.abort();

    // The bridge logs as it reads the device's answers. Once it has logged
    // one, a line for each further refusal would follow within the second.
    let _ = tokio::time::timeout(Duration::from_secs(10), async {
        while refusal_records().is_empty() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;
    tokio::time::sleep(Duration::from_secs(1)).await;

    assert_eq!(
        refusal_records(),
        vec![(
            Level::Warn,
            format!("Ros2Bridge: the device refused a write from '{topic}' to {KEY}: {REASON}")
        )],
        "{writes} refused writes from one topic to one key within an interval are logged \
         once, at warn, naming the topic, the keys and the reason",
    );
}
