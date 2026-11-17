//! [`UnixBridge`]: a socket file as an Arora [`Bridge`].

use std::collections::{HashMap, VecDeque};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arora_bridge::{Bridge, BridgeCommand, BridgeResult, DeviceInfo, Inbound, InboundStream};
use arora_types::call::CallResult;
use arora_types::data::{prefix_covers, Key, StateChange};
use arora_types::value::Value;
use async_trait::async_trait;
use futures::channel::{mpsc, oneshot};
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use tokio::io::AsyncWriteExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;
use tokio::task::JoinSet;
use tokio_util::codec::FramedRead;

use crate::wire::{self, Request};

/// How many steps' changes the broadcast to the connections holds. Each
/// subscribed connection's task moves them into its own queue as they come,
/// so only a task starved this long falls behind it, and is closed.
const BROADCAST: usize = 1024;

/// A connection's cap on the requests it reads ahead of the answers it writes,
/// so a client that sends without reading stops being read.
const IN_FLIGHT: usize = 1024;

/// A device's bridge over a Unix socket file: every client that opens the
/// file reads and writes keys, calls functions, and is pushed the keys it
/// subscribes to, in Arora Buffers ([wire format](crate#wire-format)).
///
/// Access is the file's: it is created with mode `0600`, so only its owner
/// connects, as the user they are, locally or through an `ssh` forward of the
/// file. Opening it to a group is a `chmod` on [`path`](Self::path) after
/// binding. Writes are then checked by the device like every bridge's: a key
/// is closed to remote writers unless its meta opens it.
///
/// The connections are served by tasks of the Tokio runtime the bridge is
/// bound in; dropping the bridge stops them and removes the file, unless
/// another device has replaced it since.
pub struct UnixBridge {
    path: PathBuf,
    /// The file's identity as bound (device and inode), so the bridge removes
    /// only its own file.
    identity: (u64, u64),
    inbound: Option<mpsc::UnboundedReceiver<Inbound>>,
    /// Resolves (cancelled) when the accept task ends.
    stopped: Option<oneshot::Receiver<()>>,
    changes: broadcast::Sender<Arc<StateChange>>,
    accept: tokio::task::JoinHandle<()>,
}

/// What the connections share with the bridge: the inbound channel, and how
/// many of them subscribe to anything.
struct Shared {
    inbound: mpsc::UnboundedSender<Inbound>,
    /// The connections subscribed to at least one prefix. Held while sending
    /// the `DataRequested` its change implies, so the device receives those
    /// in the order the count moved.
    subscribed: Mutex<usize>,
}

impl Shared {
    /// Count a connection in (`true`) or out of the subscribed ones, telling
    /// the device when the first comes in or the last goes out.
    fn count(&self, subscribed: bool) {
        let mut count = self.subscribed.lock().expect("the count is never poisoned");
        let was = *count > 0;
        if subscribed {
            *count += 1;
        } else {
            *count -= 1;
        }
        if was != (*count > 0) {
            let _ = self
                .inbound
                .unbounded_send(Inbound::DataRequested(*count > 0));
        }
    }
}

impl UnixBridge {
    /// The size of the values a connection may have queued and not yet
    /// written, beyond the change it is writing, before it is closed. A change
    /// is a difference from the previous one, so a client that misses one would
    /// hold a stale state with no sign of it; closing it tells the client to
    /// read again. Keys pushed [newest only](crate#newest-only) are not queued
    /// and do not count.
    pub const BACKLOG_BYTES: usize = 64 << 20;

    /// Listen on the socket file `path`, creating its directory if missing.
    /// A file there that no device answers on (one a crashed device left) is
    /// replaced; one a device answers on fails with
    /// [`AddrInUse`](std::io::ErrorKind::AddrInUse).
    ///
    /// Must be called within a Tokio runtime with I/O enabled, which serves
    /// the connections.
    pub fn bind(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if path.exists() {
            if std::os::unix::net::UnixStream::connect(&path).is_ok() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    format!("a device already listens on {}", path.display()),
                ));
            }
            std::fs::remove_file(&path)?;
        }
        if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let metadata = std::fs::metadata(&path)?;

        let (inbound_tx, inbound) = mpsc::unbounded();
        let shared = Arc::new(Shared {
            inbound: inbound_tx,
            subscribed: Mutex::new(0),
        });
        let (changes, _) = broadcast::channel(BROADCAST);
        let (stopped_tx, stopped) = oneshot::channel();
        let accept = tokio::spawn(accept(
            listener,
            shared.clone(),
            changes.clone(),
            stopped_tx,
        ));
        Ok(Self {
            path,
            identity: (metadata.dev(), metadata.ino()),
            inbound: Some(inbound),
            stopped: Some(stopped),
            changes,
            accept,
        })
    }

    /// The socket file the bridge listens on.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for UnixBridge {
    fn drop(&mut self) {
        // Ending the accept task drops its connections' tasks with it.
        self.accept.abort();
        if std::fs::metadata(&self.path).is_ok_and(|m| (m.dev(), m.ino()) == self.identity) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Accept connections until the listener fails for good, serving each on a
/// task of its own that ends with this one.
async fn accept(
    listener: UnixListener,
    shared: Arc<Shared>,
    changes: broadcast::Sender<Arc<StateChange>>,
    _stopped: oneshot::Sender<()>,
) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    connections.spawn(serve(stream, shared.clone(), changes.clone()));
                }
                // Out of file descriptors, or a connection reset before it
                // was accepted: the listener itself is fine, so wait and go on.
                Err(e) => {
                    log::warn!("unix bridge: accepting a connection failed: {e}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            },
            // Reap the ended connections, so the set holds only live ones.
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
        }
    }
}

/// Serve one connection: its requests go to the device and their answers
/// back, in the order the device answers; the changes it subscribed to are
/// pushed as they come. Ends when the client closes, sends something that is
/// not a request, or falls [`UnixBridge::BACKLOG_BYTES`] behind.
async fn serve(
    stream: UnixStream,
    shared: Arc<Shared>,
    changes: broadcast::Sender<Arc<StateChange>>,
) {
    let (read, mut write) = stream.into_split();
    let mut requests = FramedRead::new(read, crate::codec());
    let mut pending = FuturesUnordered::new();
    let mut answers: VecDeque<Box<[u8]>> = VecDeque::new();
    let mut pushes = Pushes::default();
    // The message being written, and how much of it is.
    let mut out: Option<(Box<[u8]>, usize)> = None;
    let reason = loop {
        if out.is_none() {
            out = answers
                .pop_front()
                .or_else(|| pushes.next_message())
                .map(|m| (m, 0));
        }
        tokio::select! {
            request = requests.next(), if pending.len() + answers.len() < IN_FLIGHT => {
                let bytes = match request {
                    None => break None,
                    Some(Err(e)) => break Some(format!("reading failed: {e}")),
                    Some(Ok(bytes)) => bytes,
                };
                match wire::decode_request(&bytes) {
                    Err(e) => break Some(format!("not a request: {e}")),
                    Ok(Request::Subscribe { id, prefixes, latest }) => {
                        if pushes.prefixes.is_empty() != prefixes.is_empty() {
                            shared.count(!prefixes.is_empty());
                        }
                        pushes.subscribe(prefixes, latest, &changes);
                        answers.push_back(wire::encode_answer(id, &Ok(CallResult { ret: arora_types::value::Value::Unit, mutated: vec![] })));
                    }
                    Ok(Request::Op { id, op }) => {
                        let (reply, answer) = oneshot::channel();
                        if shared.inbound.unbounded_send(Inbound::Command(BridgeCommand::new(op, reply))).is_err() {
                            break Some("the device is gone".into());
                        }
                        pending.push(async move {
                            (id, answer.await.unwrap_or_else(|_| Err("the device dropped the request".into())))
                        });
                    }
                }
            }
            Some((id, result)) = pending.next(), if !pending.is_empty() => {
                answers.push_back(wire::encode_answer(id, &result));
            }
            change = next_change(&mut pushes.changes) => match change {
                Ok(change) => {
                    if pushes.queue(change) {
                        break Some(format!(
                            "the client fell more than {} bytes behind",
                            UnixBridge::BACKLOG_BYTES
                        ));
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    break Some(format!("the connection's task fell {n} steps behind"))
                }
                Err(broadcast::error::RecvError::Closed) => break None,
            },
            // `write` is cancel safe: it writes nothing unless it completes.
            written = write.write(out.as_ref().map_or(&[][..], |(m, at)| &m[*at..])), if out.is_some() => {
                match written {
                    Ok(0) => break Some("writing failed: the client closed".into()),
                    Ok(n) => {
                        let (message, at) = out.as_mut().expect("written only while set");
                        *at += n;
                        if *at == message.len() {
                            out = None;
                        }
                    }
                    Err(e) => break Some(format!("writing failed: {e}")),
                }
            }
        }
    };
    if !pushes.prefixes.is_empty() {
        shared.count(false);
    }
    if let Some(reason) = reason {
        log::warn!("unix bridge: closing a connection: {reason}");
    }
}

/// The next change of the device, from the connection's receiver, or never
/// while it subscribes to nothing.
async fn next_change(
    changes: &mut Option<broadcast::Receiver<Arc<StateChange>>>,
) -> Result<Arc<StateChange>, broadcast::error::RecvError> {
    match changes {
        Some(changes) => changes.recv().await,
        None => std::future::pending().await,
    }
}

/// What a connection pushes: its subscription, and the changes it has not
/// written yet.
#[derive(Default)]
struct Pushes {
    /// The subscribed prefixes.
    prefixes: Vec<String>,
    /// The prefixes, among the subscribed keys, pushed newest only.
    latest: Vec<String>,
    /// The device's changes, received while `prefixes` is not empty. Made at
    /// the first subscription, so a connection that never subscribes holds
    /// none of them back from being freed.
    changes: Option<broadcast::Receiver<Arc<StateChange>>>,
    /// The changes holding queued keys, oldest first, each with its number
    /// and the size of its queued values.
    queue: VecDeque<(u64, Arc<StateChange>, usize)>,
    /// The size of the queued values.
    queued: usize,
    /// For each key pushed newest only with a value not sent yet, the number
    /// of the change holding it, and that change.
    newest: HashMap<Key, (u64, Arc<StateChange>)>,
    /// The number of the next change received.
    received: u64,
}

impl Pushes {
    fn covered(prefixes: &[String], key: &Key) -> bool {
        prefixes.iter().any(|p| prefix_covers(p, &key.path))
    }

    /// Replace the subscription. Values already waiting stay, sent under the
    /// new one: a key it no longer covers is dropped, and one it now pushes
    /// newest only takes its newest waiting value.
    fn subscribe(
        &mut self,
        prefixes: Vec<String>,
        latest: Vec<String>,
        changes: &broadcast::Sender<Arc<StateChange>>,
    ) {
        if prefixes.is_empty() {
            *self = Self::default();
            return;
        }
        if self.changes.is_none() {
            self.changes = Some(changes.subscribe());
        }
        self.prefixes = prefixes;
        self.latest = latest;
        for (number, change, _) in &self.queue {
            for key in change.set.keys().chain(&change.unset) {
                if Self::covered(&self.latest, key)
                    && self.newest.get(key).is_none_or(|(n, _)| n < number)
                {
                    self.newest.insert(key.clone(), (*number, change.clone()));
                }
            }
        }
    }

    /// Take in a change of the device, and tell whether the connection fell
    /// more than [`UnixBridge::BACKLOG_BYTES`] behind.
    fn queue(&mut self, change: Arc<StateChange>) -> bool {
        let number = self.received;
        self.received += 1;
        let mut size = 0;
        for key in change.set.keys().chain(&change.unset) {
            if !Self::covered(&self.prefixes, key) {
                continue;
            }
            if Self::covered(&self.latest, key) {
                self.newest.insert(key.clone(), (number, change.clone()));
            } else {
                size += key.path.len()
                    + change
                        .set
                        .get(key)
                        .and_then(Option::as_ref)
                        .map_or(1, weight);
            }
        }
        if size > 0 {
            self.queue.push_back((number, change, size));
            self.queued += size;
        }
        // One change larger than the cap alone still goes through.
        self.queue.len() > 1 && self.queued > UnixBridge::BACKLOG_BYTES
    }

    /// The next `changed` to write: the queued keys of the oldest queued
    /// change and the newest values of the keys pushed newest only, or none
    /// when nothing waits.
    fn next_message(&mut self) -> Option<Box<[u8]>> {
        loop {
            let oldest = self.queue.pop_front();
            if oldest.is_none() && self.newest.is_empty() {
                return None;
            }
            let newest: Vec<(Key, (u64, Arc<StateChange>))> = self.newest.drain().collect();
            let pushed =
                |key: &Key| Self::covered(&self.prefixes, key) && !Self::covered(&self.latest, key);
            let mut set = Vec::new();
            let mut unset = Vec::new();
            if let Some((_, change, _)) = &oldest {
                for (key, value) in change.set.iter().filter(|(key, _)| pushed(key)) {
                    set.push((key.path.as_str(), value.as_ref()));
                }
                for key in change.unset.iter().filter(|key| pushed(key)) {
                    unset.push(key.path.as_str());
                }
            }
            for (key, (_, change)) in &newest {
                if Self::covered(&self.prefixes, key) {
                    match change.set.get(key) {
                        Some(value) => set.push((key.path.as_str(), value.as_ref())),
                        None => unset.push(key.path.as_str()),
                    }
                }
            }
            if let Some((_, _, size)) = oldest {
                self.queued -= size;
            }
            // Empty when a new subscription dropped every key waiting there.
            if !set.is_empty() || !unset.is_empty() {
                return Some(wire::encode_changed(&set, &unset));
            }
        }
    }
}

/// About how many bytes `value` takes on the wire, for the backlog's cap.
fn weight(value: &Value) -> usize {
    match value {
        Value::String(s) => s.len() + 4,
        Value::ArrayBoolean(a) => a.len(),
        Value::ArrayU8(a) => a.len(),
        Value::ArrayI8(a) => a.len(),
        Value::ArrayU16(a) => a.len() * 2,
        Value::ArrayI16(a) => a.len() * 2,
        Value::ArrayU32(a) => a.len() * 4,
        Value::ArrayI32(a) => a.len() * 4,
        Value::ArrayF32(a) => a.len() * 4,
        Value::ArrayU64(a) => a.len() * 8,
        Value::ArrayI64(a) => a.len() * 8,
        Value::ArrayF64(a) => a.len() * 8,
        Value::ArrayString(a) => a.iter().map(|s| s.len() + 4).sum(),
        Value::ArrayValue(a) => a.iter().map(weight).sum(),
        Value::Option(Some(v)) => 1 + weight(v),
        Value::Structure(s) => s.fields.iter().map(|f| 16 + weight(&f.value)).sum(),
        Value::Enumeration(e) => 32 + weight(&e.value),
        _ => 16,
    }
}

#[async_trait]
impl Bridge for UnixBridge {
    /// The requests of every connection, in arrival order, and whether any
    /// connection subscribes to anything. Ends when the bridge stops
    /// listening.
    fn take_inbound(&mut self) -> InboundStream {
        let inbound = self
            .inbound
            .take()
            .expect("UnixBridge inbound stream already taken");
        let stopped = self.stopped.take().expect("taken with the inbound stream");
        Box::pin(inbound.take_until(stopped))
    }

    /// Hand the change to the connections, which encode and write what each
    /// subscribed to on their own tasks: the step copies it once and writes
    /// nothing. The runtime sends only while a connection subscribes to
    /// anything (`DataRequested`).
    fn try_send(&mut self, change: &StateChange) {
        let _ = self.changes.send(Arc::new(change.clone()));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{decode_message, Message};

    fn step(i: u32, frame_len: usize) -> Arc<StateChange> {
        let mut change = StateChange::new();
        change
            .set
            .insert(Key::from("dcm/stamp"), Some(Value::U32(i)));
        change.set.insert(
            Key::from("camera/frame"),
            Some(Value::ArrayU8(vec![i as u8; frame_len])),
        );
        Arc::new(change)
    }

    fn drain(pushes: &mut Pushes) -> Vec<StateChange> {
        std::iter::from_fn(|| pushes.next_message())
            .map(|bytes| match decode_message(&bytes).unwrap() {
                Message::Changed(change) => change,
                other => panic!("a change, not {other:?}"),
            })
            .collect()
    }

    /// A connection holds a receiver of the device's changes only while it
    /// subscribes to something, so an idle one pins none of them.
    #[test]
    fn only_a_subscribed_connection_receives() {
        let (changes, _) = broadcast::channel(BROADCAST);
        let mut pushes = Pushes::default();
        assert_eq!(changes.receiver_count(), 0);
        pushes.subscribe(vec!["dcm".into()], vec![], &changes);
        assert_eq!(changes.receiver_count(), 1);
        pushes.subscribe(vec!["".into()], vec![], &changes);
        assert_eq!(changes.receiver_count(), 1, "a new subscription keeps it");
        pushes.subscribe(vec![], vec![], &changes);
        assert_eq!(changes.receiver_count(), 0);
    }

    /// Keys pushed newest only are not queued: a client behind by many steps
    /// is sent every step's other keys, in order, and one value of each such
    /// key, the newest.
    #[test]
    fn newest_only_keys_are_replaced_not_queued() {
        let (changes, _) = broadcast::channel(BROADCAST);
        let mut pushes = Pushes::default();
        pushes.subscribe(vec!["".into()], vec!["camera".into()], &changes);
        for i in 0..100 {
            assert!(!pushes.queue(step(i, 1 << 20)), "frames are not counted");
        }
        assert!(
            pushes.queued < 100 * 64,
            "only the stamps queued: {}",
            pushes.queued
        );
        let sent = drain(&mut pushes);
        let stamps: Vec<_> = sent
            .iter()
            .map(|c| c.set[&Key::from("dcm/stamp")].clone().unwrap())
            .collect();
        assert_eq!(stamps, (0..100).map(Value::U32).collect::<Vec<_>>());
        let frames: Vec<_> = sent
            .iter()
            .filter_map(|c| c.set.get(&Key::from("camera/frame")))
            .collect();
        assert_eq!(frames, [&Some(Value::ArrayU8(vec![99; 1 << 20]))]);
        assert_eq!(pushes.queued, 0);
    }

    /// Queued values count in bytes: a client behind by more than
    /// [`UnixBridge::BACKLOG_BYTES`] of them is closed, whatever the number of
    /// steps, while a single change larger than the cap still goes through.
    #[test]
    fn the_backlog_is_counted_in_bytes() {
        let (changes, _) = broadcast::channel(BROADCAST);
        let mut pushes = Pushes::default();
        pushes.subscribe(vec!["".into()], vec![], &changes);
        assert!(!pushes.queue(step(0, UnixBridge::BACKLOG_BYTES + 1)));
        assert_eq!(drain(&mut pushes).len(), 1);
        let frame = 1 << 20;
        let fits = UnixBridge::BACKLOG_BYTES / (frame + 64);
        for i in 0..fits as u32 {
            assert!(!pushes.queue(step(i, frame)), "{i} frames fit");
        }
        assert!(pushes.queue(step(0, frame)), "one frame more does not");
    }

    /// A new subscription applies to what waits: a key it no longer covers is
    /// dropped, and a key it now pushes newest only is sent once.
    #[test]
    fn a_new_subscription_applies_to_what_waits() {
        let (changes, _) = broadcast::channel(BROADCAST);
        let mut pushes = Pushes::default();
        pushes.subscribe(vec!["".into()], vec![], &changes);
        for i in 0..3 {
            pushes.queue(step(i, 4));
        }
        pushes.subscribe(vec!["camera".into()], vec!["camera".into()], &changes);
        let sent = drain(&mut pushes);
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert_eq!(
            sent[0].set,
            [(Key::from("camera/frame"), Some(Value::ArrayU8(vec![2; 4])))].into()
        );
    }
}
