//! [`UnixBridge`]: a socket file as an Arora [`Bridge`].

use std::collections::{HashMap, VecDeque};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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
/// another device has replaced it since, or another process holds the lock
/// on its directory at that moment: the file is then left, for the next bind
/// to find stale and replace.
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
    /// and do not count. The answers a connection has to write have a cap of
    /// the same size, apart.
    ///
    /// A queued change holds the step's other keys too only while they weigh
    /// less than its queued ones; heavier, they are left out, at the cost of
    /// a copy of the queued ones. So a connection holds at most twice the cap,
    /// and a newest-only frame it replaced is freed.
    pub const BACKLOG_BYTES: usize = 64 << 20;

    /// Listen on the socket file `path`, creating its directory if missing.
    /// A socket file there that no device answers on (one a crashed device
    /// left) is replaced; one a device answers on fails with
    /// [`AddrInUse`](std::io::ErrorKind::AddrInUse), and any other file with
    /// [`AlreadyExists`](std::io::ErrorKind::AlreadyExists), and is left as
    /// it is.
    ///
    /// The socket is bound in a private directory beside `path`, given mode
    /// `0600` there, and only then linked at `path`, which a link never
    /// replaces: no one connects to it before its mode is set, and of two
    /// devices binding one path at once, one fails.
    ///
    /// Binding takes a lock on the file's directory, which bridges hold for
    /// a few system calls; one held by another process for more than two
    /// seconds fails the bind with
    /// [`WouldBlock`](std::io::ErrorKind::WouldBlock). In a directory others
    /// can open, such as `/tmp`, anyone can hold it: the device user's own
    /// (`$XDG_RUNTIME_DIR`) is the place for a device's socket.
    ///
    /// Must be called within a Tokio runtime with I/O enabled, which serves
    /// the connections.
    pub fn bind(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        // A path a client cannot connect to is refused here, not served.
        std::os::unix::net::SocketAddr::from_pathname(&path)?;
        let dir = path.parent().filter(|dir| !dir.as_os_str().is_empty());
        if let Some(dir) = dir {
            std::fs::create_dir_all(dir)?;
        }
        let private = private_dir(dir.unwrap_or(Path::new(".")))?;
        let bound = (|| {
            let listener = bind_in(&private, "s")?;
            let unplaced = private.join("s");
            std::fs::set_permissions(&unplaced, std::fs::Permissions::from_mode(0o600))?;
            // The socket's own identity, before anyone else can reach it.
            let metadata = std::fs::metadata(&unplaced)?;
            install(&unplaced, &path)?;
            listener.set_nonblocking(true)?;
            Ok::<_, std::io::Error>((UnixListener::from_std(listener)?, metadata))
        })();
        let _ = std::fs::remove_dir_all(&private);
        let (listener, metadata) = bound?;

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

/// Numbers the binds of this process, naming each its own private directory.
static BINDS: AtomicU64 = AtomicU64::new(0);

/// Create a directory of mode `0700` in `dir`, under a name no one else
/// holds: one taken (by another process with this PID, in another container
/// sharing `dir`) is skipped, never removed.
fn private_dir(dir: &Path) -> std::io::Result<PathBuf> {
    loop {
        let private = dir.join(format!(
            ".arora-bridge-unix.{}.{}",
            std::process::id(),
            BINDS.fetch_add(1, Ordering::Relaxed)
        ));
        match std::fs::DirBuilder::new().mode(0o700).create(&private) {
            Ok(()) => return Ok(private),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}

/// Bind a socket named `name` in `dir`. A path longer than a socket address
/// holds is reached, on Linux, through the directory's file descriptor, so
/// the private directory takes nothing from the length `path` may have.
fn bind_in(dir: &Path, name: &str) -> std::io::Result<std::os::unix::net::UnixListener> {
    match std::os::unix::net::UnixListener::bind(dir.join(name)) {
        #[cfg(target_os = "linux")]
        Err(e) if e.kind() == std::io::ErrorKind::InvalidInput => {
            use std::os::fd::AsRawFd;
            let dir = std::fs::File::open(dir)?;
            std::os::unix::net::UnixListener::bind(format!(
                "/proc/self/fd/{}/{name}",
                dir.as_raw_fd()
            ))
        }
        bound => bound,
    }
}

/// Give the socket file `unplaced` the name `path` too, never replacing
/// what is there: a stale socket file is removed and the link tried again,
/// anything else is an error (see [`remove_stale`]). A link either creates
/// the name or fails, so two devices binding one path at once never both
/// succeed; and they do it holding their directory's lock, so the stale file
/// one of them probed is the one it removes, never a socket the other
/// installed since.
fn install(unplaced: &Path, path: &Path) -> std::io::Result<()> {
    let _locked = lock_dir_of(path, LOCK_WAIT)?;
    match std::fs::hard_link(unplaced, path) {
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            remove_stale(path)?;
            std::fs::hard_link(unplaced, path)
        }
        linked => linked,
    }
}

/// How long a bind waits for its directory's lock. A bridge holds it for
/// the few system calls of an install; one held longer is someone else's.
const LOCK_WAIT: Duration = Duration::from_secs(2);

/// An exclusive lock on the directory holding `path`, released when the
/// returned file is dropped: the bridges binding, replacing or removing a
/// socket file there take it, so they do so one at a time. Waits up to
/// `wait`, then fails with [`WouldBlock`](std::io::ErrorKind::WouldBlock):
/// in a directory others can open (`/tmp`), anyone can hold the lock, so it
/// is never waited for without bound.
fn lock_dir_of(path: &Path, wait: Duration) -> std::io::Result<std::fs::File> {
    use std::os::fd::AsRawFd;
    let dir = path.parent().filter(|dir| !dir.as_os_str().is_empty());
    let dir = std::fs::File::open(dir.unwrap_or(Path::new(".")))?;
    let deadline = std::time::Instant::now() + wait;
    loop {
        // SAFETY: `dir` is an open file descriptor for the call's duration.
        if unsafe { libc::flock(dir.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(dir);
        }
        let error = std::io::Error::last_os_error();
        match error.kind() {
            std::io::ErrorKind::Interrupted => {}
            std::io::ErrorKind::WouldBlock if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            std::io::ErrorKind::WouldBlock => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    format!(
                        "the directory of {} stays locked by another process",
                        path.display()
                    ),
                ))
            }
            _ => return Err(error),
        }
    }
}

/// Remove the socket file a crashed device left at `path`: one that refuses
/// connections. Nothing there is fine; a live socket or any other file is an
/// error, and is left.
fn remove_stale(path: &Path) -> std::io::Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if !metadata.file_type().is_socket() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("{} exists and is not a socket file", path.display()),
        ));
    }
    // Nonblocking, so a live device with a full backlog answers at once
    // rather than holding the bind until it accepts.
    let probe = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)?;
    probe.set_nonblocking(true)?;
    let in_use = || {
        Err(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            format!("a device already listens on {}", path.display()),
        ))
    };
    match probe.connect(&socket2::SockAddr::unix(path)?) {
        Ok(()) => in_use(),
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => std::fs::remove_file(path),
        // A backlog full, or a connection under way: someone listens.
        Err(e)
            if e.kind() == std::io::ErrorKind::WouldBlock
                || e.raw_os_error() == Some(libc::EINPROGRESS) =>
        {
            in_use()
        }
        Err(e) => Err(e),
    }
}

impl Drop for UnixBridge {
    fn drop(&mut self) {
        // Ending the accept task drops its connections' tasks with it.
        self.accept.abort();
        // Without waiting: a drop never blocks on someone else's lock. Held
        // by another, the file is left, and the next bind finds it stale and
        // replaces it: removing it unlocked could remove a replacement
        // installed between the identity check and the removal.
        let Ok(_locked) = lock_dir_of(&self.path, Duration::ZERO) else {
            return;
        };
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
/// pushed as they come. Ends when the client sends something that is not a
/// request, or falls [`UnixBridge::BACKLOG_BYTES`] behind in answers or in
/// pushes. A client that stops sending (closing its writing half, as the
/// relay does) is written the answers it waits for, then the connection
/// ends.
async fn serve(
    stream: UnixStream,
    shared: Arc<Shared>,
    changes: broadcast::Sender<Arc<StateChange>>,
) {
    let (read, mut write) = stream.into_split();
    let mut requests = FramedRead::new(read, crate::codec());
    let mut pending = FuturesUnordered::new();
    let mut answers = Answers::default();
    let mut pushes = Pushes::default();
    // Whether the client may still send requests.
    let mut reading = true;
    // The message being written, and how much of it is.
    let mut out: Option<(Box<[u8]>, usize)> = None;
    let reason = loop {
        if out.is_none() {
            out = answers
                .pop()
                .or_else(|| reading.then(|| pushes.next_message()).flatten())
                .map(|m| (m, 0));
        }
        if !reading && out.is_none() && pending.is_empty() {
            break None;
        }
        tokio::select! {
            request = requests.next(), if reading && pending.len() + answers.len() < IN_FLIGHT => {
                let bytes = match request {
                    // The client sends no more: stop pushing to it, freeing
                    // what waits, and write the answers it waits for.
                    None => {
                        reading = false;
                        if !pushes.prefixes.is_empty() {
                            shared.count(false);
                        }
                        pushes = Pushes::default();
                        continue;
                    }
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
                        answers.push(wire::encode_answer(id, &Ok(CallResult { ret: arora_types::value::Value::Unit, mutated: vec![] })));
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
                if answers.push(wire::encode_answer(id, &result)) {
                    break Some(format!(
                        "the client fell more than {} bytes of answers behind",
                        UnixBridge::BACKLOG_BYTES
                    ));
                }
            }
            change = next_change(&mut pushes.changes), if reading => match change {
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
                    // A client that stopped sending may close without reading.
                    Ok(0) | Err(_) if !reading => break None,
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

/// The answers a connection has to write, oldest first, and their size.
#[derive(Default)]
struct Answers {
    queue: VecDeque<Box<[u8]>>,
    bytes: usize,
}

impl Answers {
    /// Queue `answer`, and tell whether the connection fell more than
    /// [`UnixBridge::BACKLOG_BYTES`] behind: one answer larger than the cap
    /// alone still goes through.
    fn push(&mut self, answer: Box<[u8]>) -> bool {
        self.bytes += answer.len();
        self.queue.push_back(answer);
        self.queue.len() > 1 && self.bytes > UnixBridge::BACKLOG_BYTES
    }

    fn pop(&mut self) -> Option<Box<[u8]>> {
        let answer = self.queue.pop_front()?;
        self.bytes -= answer.len();
        Some(answer)
    }

    fn len(&self) -> usize {
        self.queue.len()
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
    /// and the size of its queued values. A change keeps keys not queued (not
    /// subscribed, or newest only) only while they weigh less than its queued
    /// ones, and none a subscription stopped queueing since.
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

    /// Whether `key` is queued under the subscription: covered, and not
    /// newest only.
    fn queued_under(prefixes: &[String], latest: &[String], key: &Key) -> bool {
        Self::covered(prefixes, key) && !Self::covered(latest, key)
    }

    /// Replace the subscription. Values already waiting stay, sent under the
    /// new one: a key it no longer covers is dropped, one it now pushes
    /// newest only takes its newest waiting value, and one it now queues has
    /// only its newest waiting value sent, before the values that follow it.
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
        let was_prefixes = std::mem::replace(&mut self.prefixes, prefixes);
        let was_latest = std::mem::replace(&mut self.latest, latest);
        let was_queued = |key: &Key| Self::queued_under(&was_prefixes, &was_latest, key);
        let queued = |key: &Key| Self::queued_under(&self.prefixes, &self.latest, key);
        let mut kept = VecDeque::new();
        self.queued = 0;
        for (number, change, _) in std::mem::take(&mut self.queue) {
            let whole = size(&change, |_| true);
            for key in change.set.keys().chain(&change.unset) {
                if was_queued(key)
                    && Self::covered(&self.prefixes, key)
                    && Self::covered(&self.latest, key)
                    && self.newest.get(key).is_none_or(|(n, _)| *n < number)
                {
                    self.newest
                        .insert(key.clone(), (number, held_for(&change, key, whole)));
                }
            }
            if let Some(change) = only(change, |key| was_queued(key) && queued(key)) {
                let weighs = size(&change, queued);
                self.queued += weighs;
                kept.push_back((number, change, weighs));
            }
        }
        self.queue = kept;
        self.newest
            .retain(|key, _| Self::covered(&self.prefixes, key));
    }

    /// Take in a change of the device, and tell whether the connection fell
    /// more than [`UnixBridge::BACKLOG_BYTES`] behind.
    fn queue(&mut self, change: Arc<StateChange>) -> bool {
        let number = self.received;
        self.received += 1;
        let queued = |key: &Key| Self::queued_under(&self.prefixes, &self.latest, key);
        let whole = size(&change, |_| true);
        for key in change.set.keys().chain(&change.unset) {
            if Self::covered(&self.prefixes, key) && Self::covered(&self.latest, key) {
                self.newest
                    .insert(key.clone(), (number, held_for(&change, key, whole)));
            }
        }
        let weighs = size(&change, queued);
        if weighs > 0 {
            let change = if size(&change, |key| !queued(key)) > weighs {
                only(change, queued).expect("holds a queued key")
            } else {
                change
            };
            self.queue.push_back((number, change, weighs));
            self.queued += weighs;
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
            let queued = |key: &Key| Self::queued_under(&self.prefixes, &self.latest, key);
            let mut set = Vec::new();
            let mut unset = Vec::new();
            if let Some((_, change, _)) = &oldest {
                for (key, value) in change.set.iter().filter(|(key, _)| queued(key)) {
                    set.push((key.path.as_str(), value.as_ref()));
                }
                for key in change.unset.iter().filter(|key| queued(key)) {
                    unset.push(key.path.as_str());
                }
            }
            for (key, (_, change)) in &newest {
                // A key a new subscription queues: a queued value of it is
                // newer than the one kept from before.
                let superseded = oldest.as_ref().is_some_and(|(_, change, _)| {
                    queued(key) && (change.set.contains_key(key) || change.unset.contains(key))
                });
                if Self::covered(&self.prefixes, key) && !superseded {
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

/// `change` with only the keys `keep` holds, or none when it holds none:
/// `change` itself when it holds no other, so a change is copied only when
/// it has keys to leave out.
fn only(change: Arc<StateChange>, keep: impl Fn(&Key) -> bool) -> Option<Arc<StateChange>> {
    if change.set.keys().chain(&change.unset).all(&keep) {
        return (!change.is_empty()).then_some(change);
    }
    let mut kept = StateChange::new();
    for (key, value) in change.set.iter().filter(|(key, _)| keep(key)) {
        kept.set.insert(key.clone(), value.clone());
    }
    for key in change.unset.iter().filter(|key| keep(key)) {
        kept.unset.insert(key.clone());
    }
    (!kept.is_empty()).then(|| Arc::new(kept))
}

/// What a newest-only `key` keeps of `change`, which weighs `whole`: the
/// change itself while the key weighs at least as much as the rest (a frame
/// beside small records), or else a copy of the key alone, so a small value
/// never keeps a step's large ones alive. A key so holds at most twice its
/// own weight.
fn held_for(change: &Arc<StateChange>, key: &Key, whole: usize) -> Arc<StateChange> {
    let own = size(change, |k| k == key);
    if whole - own <= own {
        return change.clone();
    }
    only(change.clone(), |k| k == key).expect("holds the key")
}

/// About how many bytes the keys of `change` that `counted` holds take on
/// the wire.
fn size(change: &StateChange, counted: impl Fn(&Key) -> bool) -> usize {
    let set = change
        .set
        .iter()
        .filter(|(key, _)| counted(key))
        .map(|(key, value)| key.path.len() + value.as_ref().map_or(1, weight));
    let unset = change
        .unset
        .iter()
        .filter(|key| counted(key))
        .map(|key| key.path.len() + 1);
    set.chain(unset).sum()
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
        Value::ArrayValue(a) => 8 + a.iter().map(weight).sum::<usize>(),
        Value::Option(Some(v)) => 1 + weight(v),
        Value::Structure(s) => {
            24 + s
                .fields
                .iter()
                .map(|f| 16 + weight(&f.value))
                .sum::<usize>()
        }
        Value::Enumeration(e) => 32 + weight(&e.value),
        // Each element its header (a field count), then its fields.
        Value::ArrayStructure { elements, .. } => {
            24 + elements
                .iter()
                .map(|e| {
                    4 + e
                        .fields
                        .iter()
                        .map(|f| 16 + weight(&f.value))
                        .sum::<usize>()
                })
                .sum::<usize>()
        }
        Value::ArrayEnumeration { elements, .. } => {
            24 + elements
                .iter()
                .map(|e| 16 + weight(&e.value))
                .sum::<usize>()
        }
        Value::KeyValue(kv) => {
            24 + kv
                .fields
                .values()
                .map(|f| 32 + f.name.len() + f.value.as_deref().map_or(1, weight))
                .sum::<usize>()
        }
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

    /// As [`try_send`](Self::try_send), sharing the change rather than
    /// copying it: the runtime's way, so a step copies no value for the
    /// bridge.
    fn try_send_shared(&mut self, change: &Arc<StateChange>) {
        let _ = self.changes.send(change.clone());
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

    fn frames(sent: &[StateChange]) -> Vec<u8> {
        sent.iter()
            .filter_map(|c| match c.set.get(&Key::from("camera/frame")) {
                Some(Some(Value::ArrayU8(frame))) => Some(frame[0]),
                _ => None,
            })
            .collect()
    }

    /// A waiting change keeps no newest-only frame alive: the frames it
    /// would hold outweigh its queued stamp, so it holds the stamp alone.
    #[test]
    fn a_waiting_change_holds_no_replaced_frame() {
        let (changes, _) = broadcast::channel(BROADCAST);
        let mut pushes = Pushes::default();
        pushes.subscribe(vec!["".into()], vec!["camera".into()], &changes);
        for i in 0..3 {
            pushes.queue(step(i, 1 << 20));
        }
        for (_, change, _) in &pushes.queue {
            assert_eq!(
                change.set.keys().collect::<Vec<_>>(),
                [&Key::from("dcm/stamp")]
            );
        }
        let held = pushes.newest.values().map(|(_, change)| change);
        assert!(held
            .into_iter()
            .all(|change| change.set[&Key::from("dcm/stamp")] == Some(Value::U32(2))));
    }

    /// A key a new subscription queues rather than pushes newest only sends
    /// its newest waiting value once, then the values that follow it: never
    /// an older one.
    #[test]
    fn a_key_queued_again_sends_no_older_value() {
        let (changes, _) = broadcast::channel(BROADCAST);
        let mut pushes = Pushes::default();
        pushes.subscribe(vec!["".into()], vec!["camera".into()], &changes);
        for i in 0..3 {
            pushes.queue(step(i, 4));
        }
        pushes.subscribe(vec!["".into()], vec![], &changes);
        assert_eq!(frames(&drain(&mut pushes)), [2]);

        for i in 0..3 {
            pushes.queue(step(i, 4));
        }
        pushes.subscribe(vec!["".into()], vec![], &changes);
        pushes.queue(step(3, 4));
        pushes.subscribe(vec!["".into()], vec!["camera".into()], &changes);
        pushes.queue(step(4, 4));
        pushes.subscribe(vec!["".into()], vec![], &changes);
        pushes.queue(step(5, 4));
        let sent = drain(&mut pushes);
        let mut seen = frames(&sent);
        let sorted = {
            let mut sorted = seen.clone();
            sorted.sort();
            sorted
        };
        assert_eq!(seen, sorted, "frames never go back: {seen:?}");
        seen.dedup();
        assert_eq!(seen.last(), Some(&5));
    }

    /// A queued key keeps its values, sent once each, in order, when a new
    /// subscription leaves its delivery as it was.
    #[test]
    fn queued_values_survive_an_unrelated_subscription() {
        let (changes, _) = broadcast::channel(BROADCAST);
        let mut pushes = Pushes::default();
        pushes.subscribe(vec!["".into()], vec![], &changes);
        for i in 0..3 {
            pushes.queue(step(i, 4));
        }
        pushes.subscribe(vec!["".into()], vec!["face".into()], &changes);
        assert_eq!(frames(&drain(&mut pushes)), [0, 1, 2]);
        assert_eq!(pushes.queued, 0);
    }

    /// Composite values weigh what they hold, so a large one counts toward
    /// the backlog.
    #[test]
    fn a_composite_value_weighs_what_it_holds() {
        use arora_types::keyvalue::{KeyValue, KeyValueField};
        let field = KeyValueField {
            id: Default::default(),
            name: "frame".into(),
            value: Some(Box::new(Value::ArrayU8(vec![0; 1 << 20]))),
        };
        let kv = KeyValue {
            id: Default::default(),
            fields: [("frame".to_string(), field)].into(),
        };
        assert!(weight(&Value::KeyValue(kv)) > 1 << 20);
    }

    /// A small newest-only value keeps none of its step's large values alive:
    /// it holds a copy of itself alone, while a frame outweighing the rest of
    /// its step keeps the step, uncopied.
    #[test]
    fn a_newest_value_holds_only_what_it_outweighs() {
        let (changes, _) = broadcast::channel(BROADCAST);
        let mut pushes = Pushes::default();
        pushes.subscribe(
            vec!["".into()],
            vec!["status".into(), "camera".into()],
            &changes,
        );
        for i in 0..100u32 {
            let mut change = StateChange::new();
            change.set.insert(
                Key::from(format!("status/{i}").as_str()),
                Some(Value::U32(i)),
            );
            change.set.insert(
                Key::from("camera/frame"),
                Some(Value::ArrayU8(vec![i as u8; 1 << 20])),
            );
            let change = Arc::new(change);
            pushes.queue(change.clone());
            let (_, held) = &pushes.newest[&Key::from("camera/frame")];
            assert!(Arc::ptr_eq(held, &change), "the frame is not copied");
        }
        for (key, (_, held)) in &pushes.newest {
            if key.path.starts_with("status/") {
                assert_eq!(held.set.len(), 1, "{key:?} holds {:?}", held.set.keys());
            }
        }
    }

    /// An array of empty structures weighs its elements' headers.
    #[test]
    fn empty_elements_weigh_their_headers() {
        let elements = vec![arora_types::value::StructureWithoutId { fields: vec![] }; 1 << 20];
        let array = Value::ArrayStructure {
            id: Default::default(),
            elements,
        };
        assert!(weight(&array) >= 4 << 20);
    }

    /// Answers count in bytes too: past the cap, with more than one waiting,
    /// the connection is closed.
    #[test]
    fn answers_are_counted_in_bytes() {
        let mut answers = Answers::default();
        assert!(!answers.push(vec![0; UnixBridge::BACKLOG_BYTES + 1].into()));
        assert!(answers.pop().is_some());
        assert_eq!(answers.bytes, 0);
        assert!(!answers.push(vec![0; UnixBridge::BACKLOG_BYTES / 2].into()));
        assert!(!answers.push(vec![0; UnixBridge::BACKLOG_BYTES / 2].into()));
        assert!(answers.push(vec![0; 1].into()));
    }
}
