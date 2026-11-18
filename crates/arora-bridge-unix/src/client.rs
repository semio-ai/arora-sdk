//! [`UnixClient`]: a connection to a [`UnixBridge`](crate::UnixBridge).

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use arora_bridge::{BridgeOp, CallFuture, Caller};
use arora_types::call::{Call, CallError, CallResult};
use arora_types::data::StateChange;
use futures::channel::{mpsc, oneshot};
use futures::{Stream, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio_util::codec::FramedRead;

use crate::wire::{self, Message, Request};
use crate::UnixBridge;

type Pending = oneshot::Sender<Result<CallResult, String>>;

/// A connection to a device's Unix bridge: at the socket file, at an `ssh`
/// forward of it, or over any byte stream joined to it, such as the standard
/// streams of `ssh robot python relay.py SOCKET` ([`over`](Self::over)).
///
/// [`ask`](Self::ask) sends any [`BridgeOp`] and resolves on the device's
/// answer; requests may be in flight together. The changes the connection
/// [`subscribe`](Self::subscribe)s to arrive on the [`Changes`] stream
/// [`connect`](Self::connect) returns beside it.
///
/// One task of the Tokio runtime it was connected in owns the socket; it ends,
/// closing the connection, when the client and its `Changes` are both dropped
/// or the device closes it. Requests then fail, and `Changes` ends.
pub struct UnixClient {
    requests: mpsc::UnboundedSender<(Box<[u8]>, u32, Pending)>,
    next_id: AtomicU32,
}

/// The changes a [`UnixClient`] subscribed to, in the order the device made
/// them; each holds the subscribed keys of one step. Ends when the connection
/// closes.
///
/// The changes received and not read yet are held for it, up to
/// [`UnixBridge::BACKLOG_BYTES`](crate::UnixBridge::BACKLOG_BYTES) of them
/// beyond the first, as received: a client that reads its changes slower
/// than they come is then closed, as the device closes one that reads its
/// socket too slowly, so it reconnects and reads the state again.
pub struct Changes {
    changes: tokio::sync::mpsc::UnboundedReceiver<(StateChange, usize)>,
    held: Arc<AtomicUsize>,
}

impl Stream for Changes {
    type Item = StateChange;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<StateChange>> {
        self.changes.poll_recv(cx).map(|change| {
            change.map(|(change, size)| {
                self.held.fetch_sub(size, Ordering::Relaxed);
                change
            })
        })
    }
}

impl UnixClient {
    /// Connect to the bridge listening on `path`. Must be called within a
    /// Tokio runtime with I/O enabled.
    pub async fn connect(path: impl AsRef<Path>) -> std::io::Result<(Self, Changes)> {
        let (read, write) = UnixStream::connect(path).await?.into_split();
        Ok(Self::over(read, write))
    }

    /// Speak to the bridge over `read` and `write`, a byte stream joined to
    /// its socket: an ssh server too old to forward a socket file (OpenSSH
    /// before 6.7) runs `relay.py`, shipped beside this crate, and the child
    /// process's stdout and stdin are the stream. Must be called within a
    /// Tokio runtime.
    pub fn over<R, W>(read: R, write: W) -> (Self, Changes)
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (requests, outgoing) = mpsc::unbounded();
        let (changes, incoming) = tokio::sync::mpsc::unbounded_channel();
        let held = Arc::new(AtomicUsize::new(0));
        tokio::spawn(run(read, write, outgoing, changes, held.clone()));
        (
            Self {
                requests,
                next_id: AtomicU32::new(0),
            },
            Changes {
                changes: incoming,
                held,
            },
        )
    }

    /// Send `op` to the device and resolve on its answer: the result, or the
    /// device's reason for refusing it. A value-plane answer (`ListKeys`,
    /// `DescribeMethods`) decodes with `arora_types::value_serde::from_value`.
    pub async fn ask(&self, op: BridgeOp) -> Result<CallResult, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.request(Request::Op { id, op }, id).await
    }

    /// Receive, on [`Changes`], the keys one of `prefixes` covers, replacing
    /// the previous subscription. A prefix covers the key itself and the
    /// subtree under it, on a `/` boundary, as a store's prefix does
    /// ([`prefix_covers`](arora_types::data::prefix_covers)); the empty prefix
    /// covers every key. No prefix stops the changes, which is where a
    /// connection starts.
    pub async fn subscribe(&self, prefixes: Vec<String>) -> Result<(), String> {
        self.subscribe_latest(prefixes, Vec::new()).await
    }

    /// [`subscribe`](Self::subscribe) to `prefixes`, and receive only the
    /// newest value of the keys one of `latest` covers: while this client
    /// reads slower than the device changes them, their values are replaced
    /// rather than queued, and the other keys do not fall behind for them
    /// ([newest only](crate#newest-only)). For large values a client may skip,
    /// such as camera frames.
    pub async fn subscribe_latest(
        &self,
        prefixes: Vec<String>,
        latest: Vec<String>,
    ) -> Result<(), String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.request(
            Request::Subscribe {
                id,
                prefixes,
                latest,
            },
            id,
        )
        .await
        .map(|_| ())
    }

    async fn request(&self, request: Request, id: u32) -> Result<CallResult, String> {
        let bytes = wire::encode_request(&request)?;
        let (reply, answer) = oneshot::channel();
        self.requests
            .unbounded_send((bytes, id, reply))
            .map_err(|_| "the connection is closed".to_string())?;
        answer
            .await
            .unwrap_or_else(|_| Err("the connection closed before the answer".into()))
    }
}

impl Caller for UnixClient {
    fn call(&self, call: Call) -> CallFuture<'_> {
        Box::pin(async move {
            self.ask(BridgeOp::Call(call))
                .await
                .map_err(|message| CallError::Generic { message })
        })
    }
}

/// Own the socket: write each request, and route what the device sends — an
/// answer to its request's sender, a change to `changes`. Reading goes on
/// while a request is written, so a large request and a large answer cross
/// without either side waiting on the other.
async fn run(
    read: impl AsyncRead + Unpin,
    mut write: impl AsyncWrite + Unpin,
    mut outgoing: mpsc::UnboundedReceiver<(Box<[u8]>, u32, Pending)>,
    changes: tokio::sync::mpsc::UnboundedSender<(StateChange, usize)>,
    held: Arc<AtomicUsize>,
) {
    let mut incoming = FramedRead::new(read, crate::codec());
    let mut pending: HashMap<u32, Pending> = HashMap::new();
    let mut client_open = true;
    // The requests to write, the first partly written up to its offset.
    let mut unwritten: VecDeque<Box<[u8]>> = VecDeque::new();
    let mut at = 0;
    // Whether a written request may sit in the writer's buffer.
    let mut unflushed = false;
    let reason = loop {
        tokio::select! {
            request = outgoing.next(), if client_open => match request {
                Some((bytes, id, reply)) => {
                    pending.insert(id, reply);
                    unwritten.push_back(bytes);
                }
                // The client is gone: serve the changes while they are read.
                None => client_open = false,
            },
            _ = changes.closed(), if !client_open => break None,
            // Write the first request, or flush once all are written (a
            // pipe's writer may buffer). Both are cancel safe: `write` writes
            // nothing unless it completes.
            written = async {
                match unwritten.front() {
                    Some(request) => write.write(&request[at..]).await.map(Some),
                    None => write.flush().await.map(|()| None),
                }
            }, if !unwritten.is_empty() || unflushed => match written {
                Ok(Some(0)) => break Some("writing failed: the device closed".to_string()),
                Ok(Some(n)) => {
                    at += n;
                    if at == unwritten.front().map_or(0, |m| m.len()) {
                        unwritten.pop_front();
                        at = 0;
                        unflushed = true;
                    }
                }
                Ok(None) => unflushed = false,
                Err(e) => break Some(format!("writing failed: {e}")),
            },
            message = incoming.next() => {
                let message = match message {
                    Some(Ok(bytes)) => wire::decode_message(&bytes).map(|m| (m, bytes.len())),
                    Some(Err(e)) => Err(format!("reading failed: {e}")),
                    None => break None,
                };
                match message {
                    Ok((Message::Answer { id, result }, _)) => {
                        if let Some(reply) = pending.remove(&id) {
                            let _ = reply.send(result);
                        }
                    }
                    // Nobody reads the changes any more: drop them uncounted.
                    Ok((Message::Changed(_), _)) if changes.is_closed() => {}
                    Ok((Message::Changed(change), size)) => {
                        let before = held.fetch_add(size, Ordering::Relaxed);
                        if before > 0 && before + size > UnixBridge::BACKLOG_BYTES {
                            break Some(format!(
                                "the changes not read went past {} bytes",
                                UnixBridge::BACKLOG_BYTES
                            ));
                        }
                        let _ = changes.send((change, size));
                    }
                    Err(e) => break Some(e),
                }
            }
        }
    };
    if let Some(reason) = reason {
        log::warn!("unix bridge client: closing the connection: {reason}");
    }
    for (_, reply) in pending {
        let _ = reply.send(Err("the connection closed before the answer".into()));
    }
}
