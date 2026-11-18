//! [`UnixBridge`]: a socket file as an Arora [`Bridge`].

use std::collections::{HashMap, HashSet, VecDeque};
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

/// About the most a `changed` gathering waiting values may weigh: half the
/// largest message, the other half a margin for what [`weight`] estimates.
const BATCH: usize = wire::MAX_MESSAGE_LEN / 2;

/// The same for what it takes decoded, against [`wire::MAX_DECODED_LEN`].
const DECODED_BATCH: usize = wire::MAX_DECODED_LEN / 2;

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
    /// The memory the values a connection has queued and not yet written may
    /// take, beyond the change it is writing, before it is closed. A change
    /// is a difference from the previous one, so a client that misses one would
    /// hold a stale state with no sign of it; closing it tells the client to
    /// read again. Keys pushed [newest only](crate#newest-only) are not queued
    /// and do not count. The answers a connection has to write, held encoded,
    /// have a cap of the same size on their bytes, apart.
    ///
    /// A queued change holds the step's other keys too only while they take
    /// less than its queued ones; more, they are left out, at the cost of
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
    /// The changes holding queued keys, oldest first, each with its number,
    /// what its queued keys weigh on the wire and what they take decoded. A change keeps keys not queued (not
    /// subscribed, or newest only) only while they weigh less than its queued
    /// ones, and none a subscription stopped queueing since.
    queue: VecDeque<(u64, Arc<StateChange>, usize, usize)>,
    /// What the queued values take decoded, which is what the queue holds.
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
        for (number, change, _, _) in std::mem::take(&mut self.queue) {
            let whole = wire::decoded_size(&change, |_| true);
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
                let held = wire::decoded_size(&change, queued);
                self.queued += held;
                let wire = size(&change, queued);
                kept.push_back((number, change, wire, held));
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
        let whole = wire::decoded_size(&change, |_| true);
        for key in change.set.keys().chain(&change.unset) {
            if Self::covered(&self.prefixes, key) && Self::covered(&self.latest, key) {
                self.newest
                    .insert(key.clone(), (number, held_for(&change, key, whole)));
            }
        }
        let held = wire::decoded_size(&change, queued);
        if held > 0 {
            let change = if whole - held > held {
                only(change, queued).expect("holds a queued key")
            } else {
                change
            };
            let wire = size(&change, queued);
            self.queue.push_back((number, change, wire, held));
            self.queued += held;
        }
        // One change larger than the cap alone still goes through.
        self.queue.len() > 1 && self.queued > UnixBridge::BACKLOG_BYTES
    }

    /// The next `changed` to write: the queued keys of the oldest queued
    /// change and the newest values of the keys pushed newest only, as many
    /// as fit in about [`BATCH`] bytes, or none when nothing waits.
    fn next_message(&mut self) -> Option<Box<[u8]>> {
        loop {
            // A key a new subscription queues rather than pushes newest only:
            // a queued value of it is newer than the one kept from before,
            // which is dropped.
            let queue = &self.queue;
            self.newest.retain(|key, _| {
                !(Self::queued_under(&self.prefixes, &self.latest, key)
                    && queue.iter().any(|(_, change, _, _)| {
                        change.set.contains_key(key) || change.unset.contains(key)
                    }))
            });
            let oldest = self.queue.pop_front();
            if oldest.is_none() && self.newest.is_empty() {
                return None;
            }
            let queued = |key: &Key| Self::queued_under(&self.prefixes, &self.latest, key);
            // A step too large for one message goes in several, in order: the
            // keys that fit now, the rest back at the front of the queue.
            let mut part: Option<HashSet<Key>> = None;
            if let Some((number, change, wire, held)) = &oldest {
                if *wire > BATCH || *held > DECODED_BATCH {
                    let (mut wire, mut held) = (0, 0);
                    let mut now = HashSet::new();
                    for key in change
                        .set
                        .keys()
                        .chain(&change.unset)
                        .filter(|key| queued(key))
                    {
                        let (key_wire, key_held) = key_cost(change, key);
                        if !now.is_empty()
                            && (wire + key_wire > BATCH || held + key_held > DECODED_BATCH)
                        {
                            continue;
                        }
                        wire += key_wire;
                        held += key_held;
                        now.insert(key.clone());
                    }
                    if let Some(rest) =
                        only(change.clone(), |key| queued(key) && !now.contains(key))
                    {
                        let rest_wire = size(&rest, queued);
                        let rest_held = wire::decoded_size(&rest, queued);
                        self.queued += rest_held;
                        self.queue.push_front((*number, rest, rest_wire, rest_held));
                    }
                    part = Some(now);
                }
            }
            let in_part = |key: &Key| part.as_ref().is_none_or(|now| now.contains(key));
            // The newest values that fit beside it, on the wire and decoded,
            // the others left for the next messages; one always goes,
            // whatever its size.
            let (mut wire, mut unpacked) = match (&oldest, &part) {
                (Some((_, change, _, _)), Some(now)) => now.iter().fold((0, 0), |(w, h), key| {
                    let (key_wire, key_held) = key_cost(change, key);
                    (w + key_wire, h + key_held)
                }),
                (Some((_, _, wire, held)), None) => (*wire, *held),
                (None, _) => (0, 0),
            };
            let mut taken = Vec::new();
            for (key, (_, change)) in &self.newest {
                let (own, own_unpacked) = key_cost(change, key);
                let first = wire == 0 && unpacked == 0;
                if !first && (wire + own > BATCH || unpacked + own_unpacked > DECODED_BATCH) {
                    continue;
                }
                wire += own;
                unpacked += own_unpacked;
                taken.push(key.clone());
            }
            let newest: Vec<(Key, (u64, Arc<StateChange>))> = taken
                .into_iter()
                .map(|key| {
                    let held = self.newest.remove(&key).expect("taken from it");
                    (key, held)
                })
                .collect();
            let mut set = Vec::new();
            let mut unset = Vec::new();
            if let Some((_, change, _, _)) = &oldest {
                for (key, value) in change
                    .set
                    .iter()
                    .filter(|(key, _)| queued(key) && in_part(key))
                {
                    set.push((key.path.as_str(), value.as_ref()));
                }
                for key in change
                    .unset
                    .iter()
                    .filter(|key| queued(key) && in_part(key))
                {
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
            if let Some((_, _, _, held)) = oldest {
                self.queued -= held;
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

/// What a newest-only `key` keeps of `change`, which takes `whole` decoded:
/// the change itself while the key takes at least as much as the rest (a
/// frame beside small records), or else a copy of the key alone, so a small
/// value never keeps a step's large ones alive. A key so holds at most twice
/// its own size. The key is found by lookup, so a step with many such keys
/// costs one pass, not one per key.
fn held_for(change: &Arc<StateChange>, key: &Key, whole: usize) -> Arc<StateChange> {
    let (_, own) = key_cost(change, key);
    if whole - own <= own {
        return change.clone();
    }
    let mut alone = StateChange::new();
    match change.set.get(key) {
        Some(value) => {
            alone.set.insert(key.clone(), value.clone());
        }
        None => {
            alone.unset.insert(key.clone());
        }
    }
    Arc::new(alone)
}

/// What `key` of `change` weighs on the wire, and takes decoded.
fn key_cost(change: &StateChange, key: &Key) -> (usize, usize) {
    let value = change.set.get(key);
    let wire = key.path.len() + value.and_then(Option::as_ref).map_or(1, weight);
    (wire, wire::decoded_key(key, value))
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

/// What an array takes on the wire before its elements: its tag, its
/// elements' type, their count and up to a whole alignment.
const ARRAY: usize = 16;

/// About how many bytes `value` takes on the wire, for the backlog's cap.
fn weight(value: &Value) -> usize {
    match value {
        Value::String(s) => s.len() + 4,
        Value::ArrayBoolean(a) => ARRAY + a.len(),
        Value::ArrayU8(a) => ARRAY + a.len(),
        Value::ArrayI8(a) => ARRAY + a.len(),
        Value::ArrayU16(a) => ARRAY + a.len() * 2,
        Value::ArrayI16(a) => ARRAY + a.len() * 2,
        Value::ArrayU32(a) => ARRAY + a.len() * 4,
        Value::ArrayI32(a) => ARRAY + a.len() * 4,
        Value::ArrayF32(a) => ARRAY + a.len() * 4,
        Value::ArrayU64(a) => ARRAY + a.len() * 8,
        Value::ArrayI64(a) => ARRAY + a.len() * 8,
        Value::ArrayF64(a) => ARRAY + a.len() * 8,
        Value::ArrayString(a) => ARRAY + a.iter().map(|s| s.len() + 4).sum::<usize>(),
        Value::ArrayValue(a) => ARRAY + a.iter().map(weight).sum::<usize>(),
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
            pushes.queued < 100 * 256,
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
        let sent = drain(&mut pushes);
        assert_eq!(
            sent.iter().map(|c| c.set.len()).sum::<usize>(),
            2,
            "the whole step"
        );
        assert_eq!(pushes.queued, 0);
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
        for (_, change, _, _) in &pushes.queue {
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

    /// Newest values waiting together go out in as many messages as keep
    /// each within the largest a client accepts.
    #[test]
    fn waiting_values_are_split_to_fit_a_message() {
        let (changes, _) = broadcast::channel(BROADCAST);
        let mut pushes = Pushes::default();
        pushes.subscribe(vec!["".into()], vec!["camera".into()], &changes);
        let mut change = StateChange::new();
        for eye in ["left", "right"] {
            change.set.insert(
                Key::from(format!("camera/{eye}").as_str()),
                Some(Value::ArrayU8(vec![1; BATCH / 2 + 1])),
            );
        }
        pushes.queue(Arc::new(change));
        let sent: Vec<_> = std::iter::from_fn(|| pushes.next_message()).collect();
        assert_eq!(sent.len(), 2, "one frame each");
        assert!(sent.iter().all(|m| m.len() <= wire::MAX_MESSAGE_LEN));
    }

    /// Waiting values are split by what they take decoded too: arrays of
    /// empty strings, small on the wire, large once decoded.
    #[test]
    fn waiting_values_are_split_to_fit_decoded() {
        let (changes, _) = broadcast::channel(BROADCAST);
        let mut pushes = Pushes::default();
        pushes.subscribe(vec!["".into()], vec!["log".into()], &changes);
        let strings = DECODED_BATCH / std::mem::size_of::<String>() / 2 + 1;
        for side in ["a", "b"] {
            let mut change = StateChange::new();
            change.set.insert(
                Key::from(format!("log/{side}").as_str()),
                Some(Value::ArrayString(vec![String::new(); strings])),
            );
            pushes.queue(Arc::new(change));
        }
        let sent: Vec<_> = std::iter::from_fn(|| pushes.next_message()).collect();
        assert_eq!(sent.len(), 2, "one array each");
        for message in &sent {
            arora_buffers::well_formed_within(message, wire::MAX_DECODED_LEN)
                .expect("each decodes within the limit");
        }
    }

    /// The queue counts what its values take decoded, which is what it
    /// holds: an array of units, small on the wire, counts its values.
    #[test]
    fn the_queue_counts_what_it_holds() {
        let (changes, _) = broadcast::channel(BROADCAST);
        let mut pushes = Pushes::default();
        pushes.subscribe(vec!["".into()], vec![], &changes);
        let mut change = StateChange::new();
        change.set.insert(
            Key::from("units"),
            Some(Value::ArrayValue(vec![Value::Unit; 1 << 16])),
        );
        pushes.queue(Arc::new(change));
        assert!(pushes.queued >= (1 << 16) * std::mem::size_of::<Value>());
    }

    /// A step whose queued keys are too large for one message goes in
    /// several, in order, each within the largest a client accepts.
    #[test]
    fn a_step_too_large_for_a_message_is_split() {
        let (changes, _) = broadcast::channel(BROADCAST);
        let mut pushes = Pushes::default();
        pushes.subscribe(vec!["".into()], vec![], &changes);
        let mut change = StateChange::new();
        for eye in ["left", "right"] {
            change.set.insert(
                Key::from(format!("camera/{eye}").as_str()),
                Some(Value::ArrayU8(vec![1; BATCH / 2 + 1])),
            );
        }
        pushes.queue(Arc::new(change));
        pushes.queue(step(7, 4));
        let sent: Vec<_> = std::iter::from_fn(|| pushes.next_message()).collect();
        assert!(sent.iter().all(|m| m.len() <= wire::MAX_MESSAGE_LEN));
        let decoded: Vec<StateChange> = sent
            .iter()
            .map(|bytes| match decode_message(bytes).unwrap() {
                Message::Changed(change) => change,
                other => panic!("a change, not {other:?}"),
            })
            .collect();
        assert_eq!(decoded.len(), 3, "the large step in two, then the next");
        let eyes: usize = decoded[..2].iter().map(|c| c.set.len()).sum();
        assert_eq!(eyes, 2, "both keys of the large step");
        assert!(
            decoded[2].set.contains_key(&Key::from("dcm/stamp")),
            "then the next step"
        );
        assert_eq!(pushes.queued, 0);
    }

    /// Empty arrays weigh their headers: a million of them is megabytes.
    #[test]
    fn empty_arrays_weigh_their_headers() {
        let empties = Value::ArrayValue(vec![Value::ArrayU8(vec![]); 1 << 20]);
        assert!(weight(&empties) >= 8 << 20);
        let bytes = arora_buffers::froto_value::serialize(&empties);
        assert!(
            weight(&empties) >= bytes.len(),
            "{} on the wire",
            bytes.len()
        );
    }

    /// A key queued again after newest only never goes back, however the
    /// waiting values split into messages: its kept value, superseded by a
    /// queued one, is dropped before the split.
    #[test]
    fn a_superseded_value_is_dropped_before_the_split() {
        let (changes, _) = broadcast::channel(BROADCAST);
        let mut pushes = Pushes::default();
        pushes.subscribe(vec!["".into()], vec!["camera".into()], &changes);
        let frame = |i: u8| {
            let mut change = StateChange::new();
            change.set.insert(
                Key::from("camera/frame"),
                Some(Value::ArrayU8(vec![i; BATCH / 2 + 1])),
            );
            Arc::new(change)
        };
        pushes.queue(frame(1));
        pushes.subscribe(vec!["".into()], vec![], &changes);
        pushes.queue(frame(2));
        assert_eq!(frames(&drain(&mut pushes)), [2]);
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
