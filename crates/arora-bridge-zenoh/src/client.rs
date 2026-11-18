//! The client's side: requests as queries, changes from subscribers.

use std::collections::{HashMap, VecDeque};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use arora_bridge::{BridgeOp, CallFuture, Caller};
use arora_buffers::froto_value;
use arora_types::call::{Call, CallError, CallResult};
use arora_types::data::{prefix_covers, Key, StateChange};
use arora_types::value::Value;
use futures::Stream;
use zenoh::key_expr::nonwild_keyexpr;
use zenoh::matching::MatchingListener;
use zenoh::pubsub::Subscriber;
use zenoh::query::Querier;
use zenoh::sample::{Sample, SampleKind};

use crate::messages::{self, Request};

/// A client of a [`ZenohBridge`](crate::ZenohBridge) under one prefix, on a
/// session the application opened. Its subscriptions' changes arrive on the
/// [`Changes`] [`new`](Self::new) returns.
pub struct ZenohClient {
    session: zenoh::Session,
    prefix: String,
    requests: Querier<'static>,
    /// Whether the session knows a device answering on `PREFIX/rpc`.
    reachable: tokio::sync::watch::Receiver<bool>,
    _matching: MatchingListener<()>,
    unread: Arc<Mutex<Unread>>,
    subscribers: Mutex<Vec<Subscriber<()>>>,
}

impl ZenohClient {
    /// How long a request waits for the session to know a device on the
    /// prefix before it fails: Zenoh's default query timeout.
    pub const DEVICE_WAIT: Duration = Duration::from_secs(10);

    /// A client of the device under `prefix` on `session`, and where its
    /// subscriptions' changes arrive. It subscribes to nothing yet. Fails
    /// when `prefix` is not a key expression without wildcards, or when
    /// Zenoh refuses a declaration.
    pub async fn new(session: &zenoh::Session, prefix: &str) -> zenoh::Result<(Self, Changes)> {
        nonwild_keyexpr::new(prefix)?;
        let requests = session.declare_querier(crate::rpc(prefix)).await?;
        let (reached, reachable) = tokio::sync::watch::channel(false);
        let matching = requests
            .matching_listener()
            .callback(move |status| {
                let _ = reached.send(status.matching());
            })
            .await?;
        let unread = Arc::new(Mutex::new(Unread::default()));
        Ok((
            Self {
                session: session.clone(),
                prefix: prefix.to_string(),
                requests,
                reachable,
                _matching: matching,
                unread: unread.clone(),
                subscribers: Mutex::new(Vec::new()),
            },
            Changes(unread),
        ))
    }

    /// Send `op` to the device and resolve on its answer: the result, or the
    /// device's reason for refusing it, or why no answer came. A value-plane
    /// answer (`ListKeys`, `DescribeMethods`) decodes with
    /// `arora_types::value_serde::from_value`.
    ///
    /// A session that has only just connected learns of the device a moment
    /// later: a request waits up to [`DEVICE_WAIT`](Self::DEVICE_WAIT) for it,
    /// rather than reaching no one.
    pub async fn ask(&self, op: BridgeOp) -> Result<CallResult, String> {
        self.request(Request::Op(op)).await
    }

    /// Receive, on [`Changes`], the keys one of `prefixes` covers, replacing
    /// the previous subscription. A prefix covers the key itself and the
    /// subtree under it, on a `/` boundary, as a store's prefix does
    /// ([`prefix_covers`](arora_types::data::prefix_covers)); the empty
    /// prefix covers every key, and a key two prefixes cover arrives once. No
    /// prefix stops the changes. Fails, keeping the previous subscription,
    /// when a prefix is not a key expression without wildcards.
    ///
    /// Returns once the device publishes to the new subscription: a request
    /// sent after it is answered by a device already publishing, so a change
    /// it makes arrives. Fails, with the subscription in place, when no device
    /// answers within [`DEVICE_WAIT`](Self::DEVICE_WAIT).
    ///
    /// Each change holds one key. A subscriber receives changes only: read the
    /// current values with a `get` after subscribing.
    pub async fn subscribe(&self, prefixes: Vec<String>) -> Result<(), String> {
        let prefixes = widest(prefixes)?;
        let state = crate::state(&self.prefix);
        let mut subscribers = Vec::with_capacity(prefixes.len());
        for prefix in &prefixes {
            let expression = if prefix.is_empty() {
                format!("{state}/**")
            } else {
                format!("{state}/{prefix}/**")
            };
            let (unread, under) = (self.unread.clone(), format!("{state}/"));
            let subscriber = self
                .session
                .declare_subscriber(expression)
                .callback(move |sample| match decode(&under, &sample) {
                    Ok((key, value)) => unread.lock().expect("not poisoned").add(key, value),
                    Err(reason) => log::warn!("{}: {reason}", sample.key_expr()),
                })
                .await
                .map_err(|e| format!("the prefix {prefix:?}: {e}"))?;
            subscribers.push(subscriber);
        }
        *self.subscribers.lock().expect("not poisoned") = subscribers;
        // Zenoh delivers a session's declarations ahead of its later queries:
        // the device answers this one knowing the subscribers, and tells its
        // runtime they want changes before it answers.
        self.request(Request::Subscribed).await.map(|_| ())
    }

    /// Send `request` once the session knows a device on the prefix, and
    /// resolve on its answer.
    async fn request(&self, request: Request) -> Result<CallResult, String> {
        let request = messages::encode_request(request)?;
        let mut reachable = self.reachable.clone();
        tokio::time::timeout(Self::DEVICE_WAIT, reachable.wait_for(|reached| *reached))
            .await
            .map_err(|_| format!("no device on {}", crate::rpc(&self.prefix)))?
            .map_err(|_| "the client is closed".to_string())?;
        let replies = self
            .requests
            .get()
            .payload(request)
            .await
            .map_err(|e| format!("the query failed: {e}"))?;
        let reply = replies
            .recv_async()
            .await
            .map_err(|_| format!("no device answered on {}", crate::rpc(&self.prefix)))?;
        let sample = reply
            .result()
            .map_err(|e| format!("the device replied with an error: {e:?}"))?;
        messages::check_len(sample.payload().len())?;
        messages::decode_answer(&sample.payload().to_bytes())
    }
}

/// `prefixes` without a trailing `/`, each checked as a key expression without
/// wildcards, less those another covers: the subscribers to declare, so no
/// key arrives twice.
fn widest(prefixes: Vec<String>) -> Result<Vec<String>, String> {
    let mut prefixes: Vec<String> = prefixes
        .into_iter()
        .map(|prefix| prefix.trim_end_matches('/').to_string())
        .collect();
    for prefix in prefixes.iter().filter(|prefix| !prefix.is_empty()) {
        nonwild_keyexpr::new(prefix.as_str()).map_err(|e| format!("the prefix {prefix:?}: {e}"))?;
    }
    prefixes.sort_by_key(String::len);
    let mut widest: Vec<String> = Vec::new();
    for prefix in prefixes {
        if !widest.iter().any(|wider| prefix_covers(wider, &prefix)) {
            widest.push(prefix);
        }
    }
    Ok(widest)
}

impl Caller for ZenohClient {
    fn call(&self, call: Call) -> CallFuture<'_> {
        Box::pin(async move {
            self.ask(BridgeOp::Call(call))
                .await
                .map_err(|message| CallError::Generic { message })
        })
    }
}

/// The key `sample`, published under `under`, changes, and its value: `Some`
/// when it is set, `None` when it is unset.
fn decode(under: &str, sample: &Sample) -> Result<(Key, Option<Option<Value>>), String> {
    let path = sample
        .key_expr()
        .as_str()
        .strip_prefix(under)
        .ok_or("a sample outside the device's state")?;
    let value = match sample.kind() {
        SampleKind::Delete => None,
        SampleKind::Put => {
            messages::check_len(sample.payload().len())?;
            let bytes = sample.payload().to_bytes();
            messages::whole(&bytes)?;
            match froto_value::deserialize(&bytes) {
                Value::Option(value) => Some(value.map(|value| *value)),
                _ => return Err("a key's value is an option".into()),
            }
        }
    };
    Ok((Key::from(path), value))
}

impl Drop for ZenohClient {
    fn drop(&mut self) {
        let mut unread = self.unread.lock().expect("not poisoned");
        unread.closed = true;
        if let Some(waker) = unread.waker.take() {
            waker.wake();
        }
    }
}

/// The changes received and not read yet: each key once, with its newest
/// value, in the order its oldest unread change came.
#[derive(Default)]
struct Unread {
    order: VecDeque<Key>,
    newest: HashMap<Key, Option<Option<Value>>>,
    waker: Option<Waker>,
    closed: bool,
}

impl Unread {
    fn add(&mut self, key: Key, value: Option<Option<Value>>) {
        if self.newest.insert(key.clone(), value).is_none() {
            self.order.push_back(key);
        }
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

/// The changes a [`ZenohClient`]'s subscriptions receive, one key each. What
/// is not read yet is held newest value only per key: a key changed again
/// before it is read is read once, with its newest value, so a reader slower
/// than the device skips values (camera frames) and holds no more than one
/// per key. It ends once the client is dropped and what it held is read.
pub struct Changes(Arc<Mutex<Unread>>);

impl Stream for Changes {
    type Item = StateChange;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<StateChange>> {
        let mut unread = self.0.lock().expect("not poisoned");
        if let Some(key) = unread.order.pop_front() {
            let mut change = StateChange::new();
            match unread.newest.remove(&key).flatten() {
                Some(value) => {
                    change.set.insert(key, value);
                }
                None => {
                    change.unset.insert(key);
                }
            }
            return Poll::Ready(Some(change));
        }
        if unread.closed {
            return Poll::Ready(None);
        }
        unread.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A prefix another covers, or given twice, is dropped; the empty prefix
    /// covers all; a wildcard is refused.
    #[test]
    fn prefixes_are_reduced_to_the_widest() {
        let strings = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            widest(strings(&[
                "joints/head",
                "joints/",
                "joints",
                "joints_extra",
                "arm"
            ]))
            .unwrap(),
            ["arm", "joints", "joints_extra"]
        );
        assert_eq!(widest(strings(&["arm", ""])).unwrap(), [""]);
        assert!(widest(strings(&["arm/*"])).is_err());
    }

    /// A key changed again before it is read is read once, with its newest
    /// value, in the place of its first unread change.
    #[test]
    fn unread_changes_are_held_newest_value_only() {
        let unread = Arc::new(Mutex::new(Unread::default()));
        {
            let mut held = unread.lock().unwrap();
            for i in 0..100u8 {
                held.add(
                    Key::from("camera/frame"),
                    Some(Some(Value::ArrayU8(vec![i; 8]))),
                );
                held.add(
                    Key::from(format!("status/{}", i % 2)),
                    Some(Some(Value::U8(i))),
                );
            }
            held.add(Key::from("status/1"), None);
            held.closed = true;
        }
        let read: Vec<StateChange> = futures::executor::block_on_stream(Changes(unread)).collect();
        assert_eq!(read.len(), 3);
        assert_eq!(
            read[0],
            StateChange::set("camera/frame", Value::ArrayU8(vec![99; 8]))
        );
        assert_eq!(read[1], StateChange::set("status/0", Value::U8(98)));
        assert!(read[2].unset.contains(&Key::from("status/1")));
    }
}
