//! WebSocket server implementation.
//!
//! Provides a ready-to-use WebSocket server that bridges the Arora API.
//! Each server supports at most one active client at a time.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info, warn};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use crate::handlers::{
    DeviceMethodsHandler, OnClientConnectedHandler, ReadValuesHandler, WriteValuesHandler,
};
use arora_types::value::Value;
use arora_types::Uuid;

use crate::messages::{Incoming, Outgoing};
use crate::method::InvokeResult;
use crate::registry::Registry;

/// Configuration for the WebSocket server.
#[derive(Clone)]
pub struct ServerConfig {
    /// Port to listen on.
    pub port: u16,
    /// Address to bind to. Defaults to loopback: the protocol is
    /// unauthenticated, so binding all interfaces is an explicit opt-in via
    /// [`ServerConfig::bind_address`].
    pub bind_address: String,
    /// Whether to validate written paths against the registered input keys.
    pub validate_paths: bool,
    /// Whether to serve the built-in control panel on plain HTTP requests.
    pub serve_control_panel: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 9000,
            bind_address: "127.0.0.1".to_string(),
            validate_paths: true,
            serve_control_panel: false,
        }
    }
}

impl ServerConfig {
    /// Create a new config with the specified port.
    pub fn with_port(port: u16) -> Self {
        Self {
            port,
            ..Default::default()
        }
    }

    /// Set the bind address.
    pub fn bind_address(mut self, addr: impl Into<String>) -> Self {
        self.bind_address = addr.into();
        self
    }

    /// Set whether to validate written paths.
    pub fn validate_paths(mut self, validate: bool) -> Self {
        self.validate_paths = validate;
        self
    }

    /// Enable or disable the built-in control panel served on plain HTTP requests.
    pub fn serve_control_panel(mut self, enable: bool) -> Self {
        self.serve_control_panel = enable;
        self
    }
}

/// WebSocket server bridging the Arora API.
///
/// Handles connections, parses messages, and dispatches to registered handlers.
/// Supports at most one active client at a time -- when a new client connects,
/// the previous one is disconnected.
pub struct AroraWSServer {
    config: ServerConfig,
    registry: Arc<Registry>,
    write_values_handler: RwLock<Option<WriteValuesHandler>>,
    read_values_handler: RwLock<Option<ReadValuesHandler>>,
    device_methods: RwLock<Option<DeviceMethodsHandler>>,
    on_client_connected_handler: RwLock<Option<OnClientConnectedHandler>>,
    /// Cancel token for the single active client. When cancelled, the client is disconnected.
    active_client: Arc<RwLock<Option<CancellationToken>>>,
    is_running: RwLock<bool>,
    /// Server-initiated pushes (Bridge::send_data) reach the active client here.
    outbound_tx: broadcast::Sender<Outgoing>,
    /// Cancelled when the serve loop exits — on the external cancel, a bind
    /// failure, or any accept-loop end — so an observer
    /// ([`stopped`](Self::stopped), e.g. the `WsBridge` inbound stream) sees
    /// the server die.
    lifecycle: CancellationToken,
}

impl AroraWSServer {
    /// Create a new server with the given configuration.
    pub fn new(config: ServerConfig) -> Self {
        Self {
            config,
            registry: Arc::new(Registry::new()),
            write_values_handler: RwLock::new(None),
            read_values_handler: RwLock::new(None),
            device_methods: RwLock::new(None),
            on_client_connected_handler: RwLock::new(None),
            active_client: Arc::new(RwLock::new(None)),
            is_running: RwLock::new(false),
            outbound_tx: broadcast::channel(256).0,
            lifecycle: CancellationToken::new(),
        }
    }

    /// Create a new server with default configuration.
    pub fn with_port(port: u16) -> Self {
        Self::new(ServerConfig::with_port(port))
    }

    /// Push a server-initiated message to the connected client(s).
    pub fn push(&self, msg: Outgoing) {
        let _ = self.outbound_tx.send(msg);
    }

    /// Subscribe to the outbound push channel.
    pub fn subscribe(&self) -> broadcast::Receiver<Outgoing> {
        self.outbound_tx.subscribe()
    }

    /// Get a reference to the registry.
    pub fn registry(&self) -> &Arc<Registry> {
        &self.registry
    }

    /// Set the write-values handler callback.
    /// This is called whenever a valid WriteValues message is received.
    pub async fn set_write_values_handler<F>(&self, handler: F)
    where
        F: Fn(HashMap<String, Value>) -> Result<(), String> + Send + Sync + 'static,
    {
        *self.write_values_handler.write().await = Some(Arc::new(handler));
    }

    /// Set the read-values handler callback.
    /// This is called whenever a valid ReadValues message is received.
    pub async fn set_read_values_handler(&self, handler: ReadValuesHandler) {
        *self.read_values_handler.write().await = Some(handler);
    }

    /// Set the device behind this server: where `list_methods` and `invoke`
    /// go for the methods the registry does not own.
    /// [`WsBridge`](crate::bridge::WsBridge) sets it to the device it bridges,
    /// so an embedder wiring the bridge gets the device's methods without
    /// declaring any of them here.
    pub async fn set_device_methods(&self, device: DeviceMethodsHandler) {
        *self.device_methods.write().await = Some(device);
    }

    /// Set the handler called when a new client connects.
    pub async fn set_on_client_connected_handler(&self, handler: OnClientConnectedHandler) {
        *self.on_client_connected_handler.write().await = Some(handler);
    }

    /// Disconnect the current active client (if any).
    pub async fn disconnect_client(&self) {
        let mut guard = self.active_client.write().await;
        if let Some(token) = guard.take() {
            token.cancel();
            info!(
                "Disconnected active client on ws://{}:{}",
                self.config.bind_address, self.config.port
            );
        }
    }

    /// Check if the server is running.
    pub async fn is_running(&self) -> bool {
        *self.is_running.read().await
    }

    /// Get the configured port.
    pub fn port(&self) -> u16 {
        self.config.port
    }

    /// Resolves when the serve loop has exited — whether by the external
    /// cancel, a bind failure, or the accept loop ending. The `WsBridge`
    /// inbound stream ends on this, so a device polling that stream sees a
    /// dead server as an endpoint disconnect instead of running on silently.
    pub fn stopped(&self) -> tokio_util::sync::WaitForCancellationFutureOwned {
        self.lifecycle.clone().cancelled_owned()
    }

    /// Bind the configured address, without serving yet. Splitting this from
    /// [`run_on`](Self::run_on) lets an embedder fail fast on an unusable
    /// address (port taken) instead of discovering it from a spawned task.
    pub async fn bind(&self) -> Result<TcpListener, String> {
        let addr = format!("{}:{}", self.config.bind_address, self.config.port);
        TcpListener::bind(&addr)
            .await
            .map_err(|e| format!("Failed to bind to {}: {}", addr, e))
    }

    /// Run the server until the cancellation token is triggered.
    pub async fn run(&self, cancel_token: CancellationToken) -> Result<(), String> {
        // The guard covers a bind failure too: any exit cancels `lifecycle`.
        let _stopped = self.lifecycle.clone().drop_guard();
        let listener = self.bind().await?;
        self.run_on(listener, cancel_token).await
    }

    /// Serve on an already-bound listener until the cancellation token is
    /// triggered (see [`bind`](Self::bind)).
    pub async fn run_on(
        &self,
        listener: TcpListener,
        cancel_token: CancellationToken,
    ) -> Result<(), String> {
        let _stopped = self.lifecycle.clone().drop_guard();
        let addr = format!("{}:{}", self.config.bind_address, self.config.port);

        info!("Arora WebSocket server listening on ws://{}", addr);
        if self.config.serve_control_panel {
            info!("Control panel available at http://{}", addr);
        }
        *self.is_running.write().await = true;

        let serve_control_panel = self.config.serve_control_panel;
        let conn_id = self.connection_id();
        let bind_addr = self.config.bind_address.clone();
        let port = self.config.port;

        // Snapshot the dispatch context once: the seams are wired before the
        // server serves and never change afterwards.
        let dispatch = Dispatch {
            registry: self.registry.clone(),
            write_values: self.write_values_handler.read().await.clone(),
            read_values: self.read_values_handler.read().await.clone(),
            device: self.device_methods.read().await.clone(),
            validate_paths: self.config.validate_paths,
        };
        let on_connected = self.on_client_connected_handler.read().await.clone();

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, peer_addr)) => {
                            // Spawn a task per connection so the accept loop never blocks.
                            // (peek can block if a client connects without sending data.)
                            let active_client = self.active_client.clone();
                            let dispatch = dispatch.clone();
                            let on_connected = on_connected.clone();
                            let conn_id = conn_id.clone();
                            let bind_addr = bind_addr.clone();
                            let parent_token = cancel_token.clone();
                            let outbound_tx = self.outbound_tx.clone();

                            tokio::spawn(async move {
                                // Peek with timeout to classify the connection
                                let is_ws_upgrade = {
                                    let mut peek_buf = [0u8; 4096];
                                    match tokio::time::timeout(
                                        std::time::Duration::from_secs(5),
                                        stream.peek(&mut peek_buf),
                                    ).await {
                                        Ok(Ok(n)) => {
                                            let req = String::from_utf8_lossy(&peek_buf[..n]);
                                            let lower = req.to_ascii_lowercase();
                                            lower.contains("upgrade") && lower.contains("websocket")
                                        }
                                        Ok(Err(e)) => {
                                            error!("Failed to peek connection from {}: {}", peer_addr, e);
                                            return;
                                        }
                                        Err(_) => {
                                            debug!("Connection from {} sent no data within timeout", peer_addr);
                                            return;
                                        }
                                    }
                                };

                                if !is_ws_upgrade {
                                    if serve_control_panel {
                                        serve_control_panel_http(stream).await;
                                    }
                                    return;
                                }

                                // WebSocket: enforce exclusive client policy
                                let client_token = parent_token.child_token();
                                {
                                    let mut guard = active_client.write().await;
                                    if let Some(old) = guard.take() {
                                        old.cancel();
                                        info!("Disconnected active client on ws://{}:{}", bind_addr, port);
                                    }
                                    *guard = Some(client_token.clone());
                                }

                                // Notify the on_client_connected handler
                                if let Some(ref handler) = on_connected {
                                    handler(conn_id);
                                }

                                handle_connection(
                                    stream, peer_addr, dispatch,
                                    client_token, active_client, outbound_tx,
                                ).await;
                            });
                        }
                        Err(e) => {
                            error!("Failed to accept connection: {}", e);
                        }
                    }
                }
                _ = cancel_token.cancelled() => {
                    info!("Arora WebSocket server shutting down");
                    // Disconnect the active client on shutdown
                    self.disconnect_client().await;
                    break;
                }
            }
        }

        *self.is_running.write().await = false;
        Ok(())
    }

    /// Get the connection identifier.
    pub fn connection_id(&self) -> String {
        format!("ws://127.0.0.1:{}", self.config.port)
    }
}

/// What a connection dispatches an incoming message against: the registry, the
/// value handlers, and the device behind the bridge.
#[derive(Clone)]
struct Dispatch {
    registry: Arc<Registry>,
    write_values: Option<WriteValuesHandler>,
    read_values: Option<ReadValuesHandler>,
    device: Option<DeviceMethodsHandler>,
    validate_paths: bool,
}

/// Handle a single WebSocket connection.
async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    dispatch: Dispatch,
    client_token: CancellationToken,
    active_client: Arc<RwLock<Option<CancellationToken>>>,
    outbound_tx: broadcast::Sender<Outgoing>,
) {
    info!("New WebSocket connection from: {}", addr);

    let ws_config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
        // The largest legitimate message is a few KiB; cap far below the
        // 64 MiB tungstenite default so a client cannot force huge allocations.
        max_message_size: Some(1 << 20),
        max_frame_size: Some(256 << 10),
        ..Default::default()
    };
    let ws_stream = match tokio_tungstenite::accept_async_with_config(stream, Some(ws_config)).await
    {
        Ok(ws) => ws,
        Err(e) => {
            error!("Error during WebSocket handshake: {}", e);
            return;
        }
    };

    let (mut write, mut read) = ws_stream.split();
    let mut outbound_rx = outbound_tx.subscribe();

    // The keys this connection is pushed. `None` until it subscribes: a client
    // that never does sees the whole feed.
    let mut subscription: Option<HashSet<String>> = None;

    loop {
        tokio::select! {
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        debug!("Received message: {}", text);

                        let response = match serde_json::from_str::<Incoming>(&text) {
                            // The subscription is this connection's own state,
                            // so it is answered here rather than in the shared
                            // dispatch.
                            Ok(Incoming::Subscribe { keys }) => {
                                subscription = keys.clone().map(HashSet::from_iter);
                                Outgoing::SubscribeResp { keys }
                            }
                            Ok(incoming) => process_message(incoming, &dispatch).await,
                            Err(e) => {
                                warn!("Failed to parse message: {}", e);
                                Outgoing::Error {
                                    request_id: None,
                                    message: format!("Invalid message format: {}", e),
                                }
                            }
                        };

                        let response_text = match serde_json::to_string(&response) {
                            Ok(text) => text,
                            Err(e) => {
                                error!("Failed to serialize response: {}", e);
                                break;
                            }
                        };
                        if let Err(e) = write.send(Message::Text(response_text)).await {
                            error!("Failed to send response: {}", e);
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        info!("Client {} disconnected", addr);
                        break;
                    }
                    Some(Ok(Message::Ping(data))) => {
                        if let Err(e) = write.send(Message::Pong(data)).await {
                            error!("Failed to send pong: {}", e);
                            break;
                        }
                    }
                    Some(Ok(_)) => {
                        // Ignore other message types
                    }
                    Some(Err(e)) => {
                        error!("Error reading message: {}", e);
                        break;
                    }
                    None => {
                        // Stream ended
                        break;
                    }
                }
            }
            pushed = outbound_rx.recv() => {
                match pushed {
                    Ok(msg) => {
                        // A subscribed connection is pushed the keys it asked
                        // for, and nothing is pushed for a change that holds
                        // none of them.
                        let msg = match (&subscription, msg) {
                            (Some(keys), Outgoing::ValuesChanged { values }) => {
                                let values: HashMap<String, Value> = values
                                    .into_iter()
                                    .filter(|(path, _)| keys.contains(path))
                                    .collect();
                                if values.is_empty() {
                                    continue;
                                }
                                Outgoing::ValuesChanged { values }
                            }
                            (_, msg) => msg,
                        };
                        let text = match serde_json::to_string(&msg) {
                            Ok(text) => text,
                            Err(e) => {
                                error!("Failed to serialize push message: {}", e);
                                continue;
                            }
                        };
                        if let Err(e) = write.send(Message::Text(text)).await {
                            error!("Failed to push message: {}", e);
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                }
            }
            _ = client_token.cancelled() => {
                info!("Client {} disconnected by server (exclusive client policy)", addr);
                // Send a close frame to the client
                let close_frame = CloseFrame {
                    code: CloseCode::Normal,
                    reason: "Another client connected".into(),
                };
                let _ = write.send(Message::Close(Some(close_frame))).await;
                break;
            }
        }
    }

    // Clear the active client only if this was a natural disconnect.
    // If our token was cancelled, we were replaced by a new client -- don't touch active_client.
    if !client_token.is_cancelled() {
        let mut guard = active_client.write().await;
        *guard = None;
    }

    info!("Connection closed for: {}", addr);
}

/// Serve the built-in control panel HTML over a plain HTTP response.
async fn serve_control_panel_http(mut stream: TcpStream) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const HTML: &str = include_str!("control_panel.html");

    // Read and consume the HTTP request from the buffer
    let mut buf = vec![0u8; 4096];
    let _ = stream.read(&mut buf).await;

    let body = HTML.as_bytes();
    let header = format!(
        "HTTP/1.1 200 OK\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n",
        body.len()
    );

    let _ = stream.write_all(header.as_bytes()).await;
    let _ = stream.write_all(body).await;
}

/// Answer one incoming message. Everything but the subscription, which belongs
/// to the connection that made it.
async fn process_message(incoming: Incoming, dispatch: &Dispatch) -> Outgoing {
    let Dispatch {
        registry,
        write_values: write_values_handler,
        read_values: read_values_handler,
        device,
        validate_paths,
    } = dispatch;
    let validate_paths = *validate_paths;
    match incoming {
        Incoming::WriteValues { values } => {
            // Validate paths if enabled
            if validate_paths {
                let input_paths = registry.get_input_paths().await;
                let invalid_paths: Vec<&str> = values
                    .keys()
                    .filter(|path| !input_paths.iter().any(|p| p == *path))
                    .map(|s| s.as_str())
                    .collect();

                if !invalid_paths.is_empty() {
                    warn!("Invalid paths in WriteValues: {:?}", invalid_paths);
                    return Outgoing::WriteValuesResp {
                        success: false,
                        message: Some(format!(
                            "Unknown input path(s): {}",
                            invalid_paths.join(", ")
                        )),
                    };
                }
            }

            // Call WriteValues handler if registered
            if let Some(handler) = write_values_handler {
                match handler(values) {
                    Ok(()) => {
                        debug!("WriteValues handled successfully");
                        Outgoing::WriteValuesResp {
                            success: true,
                            message: None,
                        }
                    }
                    Err(e) => {
                        error!("WriteValues handler error: {}", e);
                        Outgoing::WriteValuesResp {
                            success: false,
                            message: Some(e),
                        }
                    }
                }
            } else {
                // No handler registered, just acknowledge
                debug!("No WriteValues handler registered, acknowledging");
                Outgoing::WriteValuesResp {
                    success: true,
                    message: None,
                }
            }
        }

        Incoming::ReadValues { keys } => {
            // Call ReadValues handler if registered
            if let Some(handler) = read_values_handler {
                let values = handler(keys).await;
                Outgoing::ReadValuesResp { values }
            } else {
                // No handler registered, return empty values
                debug!("No ReadValues handler registered, returning empty values");
                Outgoing::ReadValuesResp {
                    values: HashMap::new(),
                }
            }
        }

        Incoming::ListKeys { path } => {
            let keys = registry.get_keys_filtered(path.as_deref()).await;
            Outgoing::ListKeysResp { keys }
        }

        Incoming::ListMethods { path } => {
            // The registry's own methods, then the device's — a module function
            // is listed under its declared name, with the signature the device
            // described, so nothing has to mirror it here.
            let mut methods = registry.get_methods_filtered(path.as_deref()).await;
            if let Some(device) = device {
                let owned: HashSet<String> = methods.iter().map(|m| m.path.clone()).collect();
                let prefix = path.as_deref().map(|p| p.trim_end_matches('/').to_string());
                methods.extend(device.methods().await.into_iter().filter(|method| {
                    !owned.contains(&method.path)
                        && prefix
                            .as_ref()
                            .is_none_or(|prefix| method.path.starts_with(prefix.as_str()))
                }));
            }
            Outgoing::ListMethodsResp { methods }
        }

        Incoming::Invoke {
            method,
            args,
            request_id,
        } => {
            // A name the registry owns is the server's own method; anything else
            // is the device's, called by name on its described signature.
            let result = if registry.has_method(&method).await {
                registry.invoke_method(&method, args).await
            } else if let Some(device) = device {
                device.invoke(&method, args).await
            } else {
                InvokeResult::err(format!("Method not found: {method}"))
            };
            Outgoing::InvokeResp {
                success: result.success,
                request_id,
                value: result.value,
                message: result.message,
            }
        }

        Incoming::Halt { run, request_id } => {
            let result = match (Uuid::parse_str(&run), device) {
                (Ok(run), Some(device)) => device.halt(run).await,
                (Ok(_), None) => InvokeResult::err("no device to halt a run on"),
                (Err(e), _) => InvokeResult::err(format!("'{run}' is not a run id: {e}")),
            };
            Outgoing::HaltResp {
                success: result.success,
                request_id,
                message: result.message,
            }
        }

        // The connection owns its subscription, and answers it there.
        Incoming::Subscribe { keys } => Outgoing::SubscribeResp { keys },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_config_default() {
        let config = ServerConfig::default();
        assert_eq!(config.port, 9000);
        assert_eq!(config.bind_address, "127.0.0.1");
        assert!(config.validate_paths);
        assert!(!config.serve_control_panel);
    }

    #[test]
    fn test_server_config_builder() {
        let config = ServerConfig::with_port(8080)
            .bind_address("127.0.0.1")
            .validate_paths(false)
            .serve_control_panel(true);

        assert_eq!(config.port, 8080);
        assert_eq!(config.bind_address, "127.0.0.1");
        assert!(!config.validate_paths);
        assert!(config.serve_control_panel);
    }
}
