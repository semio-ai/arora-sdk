//! The device's side: serve requests on `PREFIX/rpc`, publish each step's
//! change under `PREFIX/state/`.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use arora_bridge::{Bridge, BridgeCommand, BridgeResult, DeviceInfo, Inbound, InboundStream};
use arora_buffers::{froto_value, BufferWriter};
use arora_types::call::CallResult;
use arora_types::data::StateChange;
use arora_types::value::Value;
use async_trait::async_trait;
use futures::channel::{mpsc, oneshot};
use futures::StreamExt;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinSet;
use zenoh::key_expr::nonwild_keyexpr;
use zenoh::qos::CongestionControl;
use zenoh::query::Query;
use zenoh::Wait;

use crate::messages::{self, Request};
use crate::pending::Pending;

/// A device's bridge over Zenoh, under one prefix of a session the
/// application opened: requests are queries on `PREFIX/rpc`, each key's value
/// is put on `PREFIX/state/PATH` ([key space](crate#key-space)).
///
/// It serves from a task on the Tokio runtime it is created on, and publishes
/// from a thread of its own; dropping it stops both, with the requests in
/// flight, and leaves the session to the application.
pub struct ZenohBridge {
    inbound: Option<mpsc::UnboundedReceiver<Inbound>>,
    waiting: Arc<Waiting>,
    serving: tokio::task::JoinHandle<()>,
}

impl ZenohBridge {
    /// How many requests the device may hold at once, from every client; a
    /// query beyond is answered at once with an error, so a client sending
    /// faster than the device steps cannot grow what the device holds.
    pub const REQUESTS_IN_FLIGHT: usize = 1024;

    /// Serve a device under `prefix` on `session`. Fails when `prefix` is not
    /// a key expression without wildcards, or when Zenoh refuses a
    /// declaration.
    ///
    /// Must be called within a Tokio runtime, which runs the task answering
    /// queries.
    pub async fn new(session: &zenoh::Session, prefix: &str) -> zenoh::Result<Self> {
        nonwild_keyexpr::new(prefix)?;
        let queryable = session.declare_queryable(crate::rpc(prefix)).await?;
        // Nothing is put through this publisher: its matching status says
        // whether a subscriber watches any of the device's keys.
        let watched = session
            .declare_publisher(format!("{}/**", crate::state(prefix)))
            .await?;
        let matching = watched.matching_listener().await?;
        // Started first: if it cannot be, nothing is left serving.
        let waiting = Arc::new(Waiting::default());
        let (publisher, state, feed) = (session.clone(), crate::state(prefix), waiting.clone());
        std::thread::Builder::new()
            .name("arora-bridge-zenoh".into())
            .spawn(move || publish(&publisher, &state, &feed))?;
        let (inbound_tx, inbound) = mpsc::unbounded();
        // Whether anyone wants the device's changes is told from this task
        // only, as the status is when it is told: a listener's event, or a
        // `subscribed` request, has it read again, so an older reading never
        // follows a newer one.
        let (refresh, mut refreshes) = mpsc::unbounded::<oneshot::Sender<()>>();
        let serving = tokio::spawn(async move {
            let in_flight = Arc::new(Semaphore::new(Self::REQUESTS_IN_FLIGHT));
            // Owned here, so stopping this task stops the requests in flight.
            let mut answering = JoinSet::new();
            let tell = |inbound_tx: &mpsc::UnboundedSender<Inbound>| {
                let watching = watched
                    .matching_status()
                    .wait()
                    .is_ok_and(|status| status.matching());
                let _ = inbound_tx.unbounded_send(Inbound::DataRequested(watching));
            };
            tell(&inbound_tx);
            loop {
                tokio::select! {
                    query = queryable.recv_async() => match query {
                        Ok(query) => match in_flight.clone().try_acquire_owned() {
                            Ok(permit) => {
                                answering.spawn(answer(query, inbound_tx.clone(), refresh.clone(), permit));
                            }
                            Err(_) => {
                                let busy = format!("the device holds {} requests", Self::REQUESTS_IN_FLIGHT);
                                reply(&query, Err(busy)).await;
                            }
                        },
                        Err(_) => break,
                    },
                    Some(_) = answering.join_next(), if !answering.is_empty() => {}
                    status = matching.recv_async() => match status {
                        Ok(_) => tell(&inbound_tx),
                        Err(_) => break,
                    },
                    Some(told) = refreshes.next() => {
                        tell(&inbound_tx);
                        let _ = told.send(());
                    }
                }
            }
        });
        Ok(Self {
            inbound: Some(inbound),
            waiting,
            serving,
        })
    }
}

impl Drop for ZenohBridge {
    fn drop(&mut self) {
        self.serving.abort();
        self.waiting.stopped.store(true, Ordering::Relaxed);
        *self.waiting.pending.lock().expect("not poisoned") = None;
        self.waiting.ready.notify_one();
    }
}

#[async_trait]
impl Bridge for ZenohBridge {
    fn take_inbound(&mut self) -> InboundStream {
        Box::pin(
            self.inbound
                .take()
                .expect("the inbound stream is taken once"),
        )
    }

    fn try_send(&mut self, change: &StateChange) {
        self.try_send_shared(&Arc::new(change.clone()));
    }

    /// Hand the change to the publishing thread, which encodes it from the
    /// shared value: a frame is copied only into Zenoh's buffers. What waits
    /// is each key's newest value only ([`Pending`]), so a stalled link holds
    /// no replaced frame.
    fn try_send_shared(&mut self, change: &Arc<StateChange>) {
        if let Some(pending) = self.waiting.pending.lock().expect("not poisoned").as_mut() {
            pending.push(change);
        }
        self.waiting.ready.notify_one();
    }

    async fn get_device_info(&self) -> BridgeResult<Option<DeviceInfo>> {
        Ok(None)
    }

    async fn update_device_info(
        &self,
        info: Option<DeviceInfo>,
    ) -> BridgeResult<Option<DeviceInfo>> {
        Ok(info)
    }
}

/// What the publishing thread has yet to publish; `None` once the bridge is
/// dropped, which also sets `stopped`, checked before each put.
struct Waiting {
    pending: Mutex<Option<Pending>>,
    ready: Condvar,
    stopped: AtomicBool,
}

impl Default for Waiting {
    fn default() -> Self {
        Self {
            pending: Mutex::new(Some(Pending::default())),
            ready: Condvar::new(),
            stopped: AtomicBool::new(false),
        }
    }
}

/// Answer one query: hand its request to the device, reply with the answer.
/// `permit` counts it in flight until it is answered.
async fn answer(
    query: Query,
    inbound: mpsc::UnboundedSender<Inbound>,
    refresh: mpsc::UnboundedSender<oneshot::Sender<()>>,
    permit: OwnedSemaphorePermit,
) {
    let request = match query.payload() {
        Some(payload) => messages::check_len(payload.len())
            .and_then(|()| messages::decode_request(&payload.to_bytes())),
        None => Err("a query on rpc carries a request".to_string()),
    };
    let answer = match request {
        Ok(Request::Op(op)) => {
            let (reply, result) = oneshot::channel();
            match inbound.unbounded_send(Inbound::Command(BridgeCommand::new(op, reply))) {
                Ok(()) => result
                    .await
                    .unwrap_or_else(|_| Err("the device dropped the request".into())),
                Err(_) => Err("the device is stopped".into()),
            }
        }
        // The querying session's subscribers reached this session before its
        // query did: have the device told, ahead of any request the client
        // sends next, whether anyone wants its changes.
        Ok(Request::Subscribed) => {
            let (told, done) = oneshot::channel();
            match refresh.unbounded_send(told).map(|()| done) {
                Ok(done) => match done.await {
                    Ok(()) => Ok(CallResult {
                        ret: Value::Unit,
                        mutated: Vec::new(),
                    }),
                    Err(_) => Err("the device is stopped".into()),
                },
                Err(_) => Err("the device is stopped".into()),
            }
        }
        Err(reason) => Err(reason),
    };
    drop(permit);
    reply(&query, answer).await;
}

async fn reply(query: &Query, answer: Result<CallResult, String>) {
    let bytes = messages::encode_answer(answer);
    if let Err(e) = query.reply(query.key_expr().clone(), bytes).await {
        log::debug!("a reply on {} failed: {e}", query.key_expr());
    }
}

/// Publish under `state` what `waiting` receives, until the bridge is
/// dropped. What waits while a batch is published goes out together, each
/// key's newest value only. Once the bridge is dropped, the batch under way
/// stops before its next put.
fn publish(session: &zenoh::Session, state: &str, waiting: &Waiting) {
    let mut refused = HashSet::new();
    loop {
        let batch = {
            let mut pending = waiting.pending.lock().expect("not poisoned");
            while pending.as_ref().is_some_and(Pending::is_empty) {
                pending = waiting.ready.wait(pending).expect("not poisoned");
            }
            // Taken whole under the lock, ordered after it: the step, which
            // takes the lock to add a change, never waits on a sort.
            match pending.as_mut() {
                Some(pending) => std::mem::take(pending),
                None => return,
            }
        }
        .take();
        for (key, held) in &batch {
            if waiting.stopped.load(Ordering::Relaxed) {
                return;
            }
            // Zenoh takes a put or a delete on a wildcard expression, and
            // delivers it to every key it intersects: a key named `arm/*`
            // would set, or unset, all of `arm`'s.
            let published = nonwild_keyexpr::new(key.path.as_str()).and_then(|_| {
                let expression = format!("{state}/{}", key.path);
                match held.value(key) {
                    Some(value) => session
                        .put(expression, encode(value.as_ref()))
                        .congestion_control(CongestionControl::Block)
                        .wait(),
                    None => session
                        .delete(expression)
                        .congestion_control(CongestionControl::Block)
                        .wait(),
                }
            });
            if let Err(e) = published {
                if refused.insert(key.clone()) {
                    log::warn!("the key {:?} is not published: {e}", key.path);
                }
            }
        }
    }
}

/// A key's value as published: an option, some value or none, written from
/// the borrowed value.
fn encode(value: Option<&Value>) -> Vec<u8> {
    let mut writer = BufferWriter::new();
    match value {
        Some(value) => {
            writer.add_option_some();
            froto_value::serialize_to_writer(value, &mut writer);
        }
        None => writer.add_option_none(),
    }
    writer.finalize().into_vec()
}
