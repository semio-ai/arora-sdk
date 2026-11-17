//! [`UnixClient`]: a connection to a [`UnixBridge`](crate::UnixBridge).

use std::collections::HashMap;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
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
pub struct Changes(tokio::sync::mpsc::UnboundedReceiver<StateChange>);

impl Stream for Changes {
    type Item = StateChange;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<StateChange>> {
        self.0.poll_recv(cx)
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
        tokio::spawn(run(read, write, outgoing, changes));
        (
            Self {
                requests,
                next_id: AtomicU32::new(0),
            },
            Changes(incoming),
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
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.request(Request::Subscribe { id, prefixes }, id)
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
/// answer to its request's sender, a change to `changes`.
async fn run(
    read: impl AsyncRead + Unpin,
    mut write: impl AsyncWrite + Unpin,
    mut outgoing: mpsc::UnboundedReceiver<(Box<[u8]>, u32, Pending)>,
    changes: tokio::sync::mpsc::UnboundedSender<StateChange>,
) {
    let mut incoming = FramedRead::new(read, crate::codec());
    let mut pending: HashMap<u32, Pending> = HashMap::new();
    let mut client_open = true;
    loop {
        tokio::select! {
            request = outgoing.next(), if client_open => match request {
                Some((bytes, id, reply)) => {
                    // A pipe's writer may buffer: flush each request.
                    let written = match write.write_all(&bytes).await {
                        Ok(()) => write.flush().await,
                        Err(e) => Err(e),
                    };
                    if let Err(e) = written {
                        let _ = reply.send(Err(format!("the connection failed: {e}")));
                        break;
                    }
                    pending.insert(id, reply);
                }
                // The client is gone: serve the changes while they are read.
                None => client_open = false,
            },
            _ = changes.closed(), if !client_open => break,
            message = incoming.next() => {
                let message = match message {
                    Some(Ok(bytes)) => wire::decode_message(&bytes),
                    Some(Err(e)) => Err(format!("reading failed: {e}")),
                    None => break,
                };
                match message {
                    Ok(Message::Answer { id, result }) => {
                        if let Some(reply) = pending.remove(&id) {
                            let _ = reply.send(result);
                        }
                    }
                    Ok(Message::Changed(change)) => {
                        let _ = changes.send(change);
                    }
                    Err(e) => {
                        log::warn!("unix bridge client: closing the connection: {e}");
                        break;
                    }
                }
            }
        }
    }
}
