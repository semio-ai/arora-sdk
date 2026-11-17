//! [`UnixBridge`]: a socket file as an Arora [`Bridge`].

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arora_bridge::{Bridge, BridgeCommand, BridgeResult, DeviceInfo, Inbound, InboundStream};
use arora_types::call::CallResult;
use arora_types::data::{prefix_covers, Key, StateChange};
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

/// How many steps' changes a connection may fall behind before it is closed.
/// A change is a difference from the previous one, so a connection that misses
/// one would hold a stale state with no sign of it; closing it tells its client
/// to read again. At the device's default 100 Hz, ten seconds.
const BACKLOG: usize = 1024;

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
        let (changes, _) = broadcast::channel(BACKLOG);
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
                    connections.spawn(serve(stream, shared.clone(), changes.subscribe()));
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
/// not a request, or falls [`BACKLOG`] changes behind.
async fn serve(
    stream: UnixStream,
    shared: Arc<Shared>,
    mut changes: broadcast::Receiver<Arc<StateChange>>,
) {
    let (read, mut write) = stream.into_split();
    let mut requests = FramedRead::new(read, crate::codec());
    let mut answers = FuturesUnordered::new();
    let mut prefixes: Vec<String> = Vec::new();
    let reason = loop {
        let out = tokio::select! {
            request = requests.next() => {
                let bytes = match request {
                    None => break None,
                    Some(Err(e)) => break Some(format!("reading failed: {e}")),
                    Some(Ok(bytes)) => bytes,
                };
                match wire::decode_request(&bytes) {
                    Err(e) => break Some(format!("not a request: {e}")),
                    Ok(Request::Subscribe { id, prefixes: new }) => {
                        if prefixes.is_empty() != new.is_empty() {
                            shared.count(!new.is_empty());
                            // Unread while unsubscribed: start from now.
                            changes = changes.resubscribe();
                        }
                        prefixes = new;
                        wire::encode_answer(id, &Ok(CallResult { ret: arora_types::value::Value::Unit, mutated: vec![] }))
                    }
                    Ok(Request::Op { id, op }) => {
                        let (reply, answer) = oneshot::channel();
                        if shared.inbound.unbounded_send(Inbound::Command(BridgeCommand::new(op, reply))).is_err() {
                            break Some("the device is gone".into());
                        }
                        answers.push(async move {
                            (id, answer.await.unwrap_or_else(|_| Err("the device dropped the request".into())))
                        });
                        continue;
                    }
                }
            }
            Some((id, result)) = answers.next(), if !answers.is_empty() => wire::encode_answer(id, &result),
            change = changes.recv(), if !prefixes.is_empty() => match change {
                Ok(change) => {
                    let subscribed = |key: &Key| prefixes.iter().any(|p| prefix_covers(p, &key.path));
                    if !change.set.keys().chain(&change.unset).any(subscribed) {
                        continue;
                    }
                    wire::encode_changed(&change, subscribed)
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    break Some(format!("the client fell {n} changes behind"))
                }
                Err(broadcast::error::RecvError::Closed) => break None,
            },
        };
        if let Err(e) = write.write_all(&out).await {
            break Some(format!("writing failed: {e}"));
        }
    };
    if !prefixes.is_empty() {
        shared.count(false);
    }
    if let Some(reason) = reason {
        log::warn!("unix bridge: closing a connection: {reason}");
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
