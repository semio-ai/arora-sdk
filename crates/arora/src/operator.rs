//! The operator interface — how a running arora asks whoever operates the
//! device for input and decisions, independently of *how* they are asked.
//!
//! An [`Operator`] answers two kinds of questions: free-text prompts (device
//! info at registration, e.g. the device name or its owner) and access
//! decisions (a remote client asking to join the session — see
//! [`arora_bridge::AccessRequest`]). Implementations decide the medium:
//!
//! - [`crate::tui`]'s handle renders them in the terminal UI's prompt line;
//! - [`UnattendedOperator`] answers alone, allowing access after a grace
//!   period — the daemon behavior;
//! - a device-specific build with its own GUI implements [`Operator`] with
//!   native dialogs and reuses everything else unchanged.
//!
//! [`serve_access_requests`] is the reusable pump between a
//! [`Bridge`](arora_bridge::Bridge) and an [`Operator`]: it forwards each
//! request, applies remembered ("always …") rulings, and replies on the
//! request's channel.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use arora_bridge::{AccessDecision, AccessRequestStream, DeviceInfo};
use async_trait::async_trait;
use futures::StreamExt;
use log::{info, warn};

use crate::runtime::Telemetry;

/// How long an unanswered access request waits before it is granted, when no
/// configured value applies. (The access-control spec's time-based approval;
/// 10 s is the fallback default.)
pub const DEFAULT_ACCESS_GRACE: Duration = Duration::from_secs(10);

/// The identifying parts of an [`arora_bridge::AccessRequest`], for display
/// and decision-making (the request itself stays with the pump, which owns
/// the reply channel).
#[derive(Debug, Clone)]
pub struct AccessRequestSummary {
    pub client_id: String,
    pub user_id: Option<String>,
    pub permission: String,
}

impl AccessRequestSummary {
    /// The stable subject a remembered ruling applies to: the user when known,
    /// the client session otherwise.
    pub fn subject(&self) -> &str {
        self.user_id.as_deref().unwrap_or(&self.client_id)
    }

    /// A short human-readable description, e.g.
    /// `Studio client 1a2b3c4d (user 9f8e7d6c) wants to join the session`.
    pub fn describe(&self) -> String {
        let client = shorten(&self.client_id);
        match &self.user_id {
            Some(user) => format!(
                "Studio client {client} (user {}) wants to {}",
                shorten(user),
                self.permission
            ),
            None => format!("Studio client {client} wants to {}", self.permission),
        }
    }
}

/// The first eight characters of an identifier, for compact display.
pub fn shorten(id: &str) -> &str {
    match id.char_indices().nth(8) {
        Some((idx, _)) => &id[..idx],
        None => id,
    }
}

/// An operator's answer to an access request: the decision, and whether to
/// remember it for the same subject for the rest of the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessRuling {
    pub decision: AccessDecision,
    pub remember: bool,
}

/// Whoever (or whatever) answers the device's questions.
#[async_trait]
pub trait Operator: Send + Sync {
    /// Ask a free-text question. Returns `None` when the operator skips it —
    /// implementations only allow skipping when `required` is false.
    async fn ask_text(&self, label: &str, required: bool) -> Option<String>;

    /// Decide whether to grant `request`. Implementations are expected to
    /// resolve within a bounded time (e.g. a countdown that falls back to
    /// allowing after [`DEFAULT_ACCESS_GRACE`]).
    async fn decide_access(&self, request: &AccessRequestSummary) -> AccessRuling;
}

/// The operator of a device nobody is watching: never answers text prompts,
/// and grants access requests after a grace period (leaving a log trail an
/// operator can audit later, e.g. in journald).
pub struct UnattendedOperator {
    grace: Duration,
}

impl UnattendedOperator {
    pub fn new(grace: Duration) -> Self {
        Self { grace }
    }
}

impl Default for UnattendedOperator {
    fn default() -> Self {
        Self::new(DEFAULT_ACCESS_GRACE)
    }
}

#[async_trait]
impl Operator for UnattendedOperator {
    async fn ask_text(&self, label: &str, required: bool) -> Option<String> {
        if required {
            warn!("unattended: no operator to answer required prompt \"{label}\"");
        }
        None
    }

    async fn decide_access(&self, request: &AccessRequestSummary) -> AccessRuling {
        info!(
            "{} — allowing in {}s (unattended)",
            request.describe(),
            self.grace.as_secs()
        );
        tokio::time::sleep(self.grace).await;
        AccessRuling {
            decision: AccessDecision::Allowed,
            remember: false,
        }
    }
}

/// A chosen operator front end plus the hook that finishes wiring it once the
/// runtime is up.
///
/// Selecting a front end also picks the log sink (the terminal UI captures logs
/// into its pane; the headless one prints them), which is why building a
/// `Frontend` is what installs the logger. [`on_ready`](Frontend::on_ready) is
/// called by the launcher after the runtime and bridge exist, to hand the front
/// end the live [`Telemetry`] handle and the device identity — a no-op for a
/// front end that shows neither.
pub struct Frontend {
    /// Who answers the device's questions.
    pub operator: Arc<dyn Operator>,
    /// Called once, after the runtime and bridge are up, with the telemetry
    /// handle, the registered device info, and the backend device id.
    pub on_ready: ReadyHook,
}

/// The [`Frontend::on_ready`] hook: hands a front end the live telemetry handle,
/// the registered device info, and the backend device id, once they exist.
pub type ReadyHook = Box<dyn FnOnce(Telemetry, Option<DeviceInfo>, Option<String>) + Send>;

/// The headless front end: plain `env_logger` output and the
/// [`UnattendedOperator`]. Its ready hook does nothing — there is no UI to feed.
pub fn default_frontend() -> Frontend {
    // `try_init` so a caller that set its own logger first is not overridden
    // (and so this never panics on a second front end in one process).
    let _ = env_logger::try_init();
    Frontend {
        operator: Arc::new(UnattendedOperator::default()),
        on_ready: Box::new(|_telemetry, _info, _device_id| {}),
    }
}

/// Forward every access request from `requests` to `operator` and reply with
/// its ruling, one request at a time. Rulings marked `remember` short-circuit
/// later requests from the same subject (per-run "always allow" / "always
/// reject"). Runs until the stream ends — spawn it alongside the runtime.
pub async fn serve_access_requests(requests: AccessRequestStream, operator: Arc<dyn Operator>) {
    let mut remembered: HashMap<String, AccessDecision> = HashMap::new();
    let mut requests = requests;
    while let Some(request) = requests.next().await {
        let summary = AccessRequestSummary {
            client_id: request.client_id.clone(),
            user_id: request.user_id.clone(),
            permission: request.permission.clone(),
        };
        if let Some(decision) = remembered.get(summary.subject()) {
            info!(
                "{} — {:?} (remembered ruling)",
                summary.describe(),
                decision
            );
            request.respond(*decision);
            continue;
        }
        let ruling = operator.decide_access(&summary).await;
        info!("{} — {:?}", summary.describe(), ruling.decision);
        if ruling.remember {
            remembered.insert(summary.subject().to_string(), ruling.decision);
        }
        request.respond(ruling.decision);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arora_bridge::AccessRequest;
    use std::sync::Mutex;

    /// Scripted operator: pops pre-programmed rulings, recording what it saw.
    struct Scripted {
        rulings: Mutex<Vec<AccessRuling>>,
        seen: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl Operator for Scripted {
        async fn ask_text(&self, _label: &str, _required: bool) -> Option<String> {
            None
        }
        async fn decide_access(&self, request: &AccessRequestSummary) -> AccessRuling {
            self.seen
                .lock()
                .unwrap()
                .push(request.subject().to_string());
            self.rulings.lock().unwrap().remove(0)
        }
    }

    #[tokio::test]
    async fn remembered_rulings_short_circuit_the_operator() {
        let (req1, rx1) = AccessRequest::new("c1", Some("user-a".into()), "join the session");
        let (req2, rx2) = AccessRequest::new("c2", Some("user-a".into()), "join the session");
        let (req3, rx3) = AccessRequest::new("c3", Some("user-b".into()), "join the session");
        let operator = Arc::new(Scripted {
            rulings: Mutex::new(vec![
                AccessRuling {
                    decision: AccessDecision::Rejected,
                    remember: true,
                },
                AccessRuling {
                    decision: AccessDecision::Allowed,
                    remember: false,
                },
            ]),
            seen: Mutex::new(Vec::new()),
        });

        let stream: AccessRequestStream = Box::pin(futures::stream::iter([req1, req2, req3]));
        serve_access_requests(stream, operator.clone()).await;

        // user-a's first ruling was remembered: the second request never
        // reached the operator and got the same rejection.
        assert_eq!(rx1.await.unwrap(), AccessDecision::Rejected);
        assert_eq!(rx2.await.unwrap(), AccessDecision::Rejected);
        assert_eq!(rx3.await.unwrap(), AccessDecision::Allowed);
        assert_eq!(*operator.seen.lock().unwrap(), vec!["user-a", "user-b"]);
    }

    #[tokio::test(start_paused = true)]
    async fn unattended_operator_allows_after_the_grace_period() {
        let (req, rx) = AccessRequest::new("c1", None, "join the session");
        let stream: AccessRequestStream = Box::pin(futures::stream::iter([req]));
        let served = tokio::spawn(serve_access_requests(
            stream,
            Arc::new(UnattendedOperator::default()),
        ));
        // Paused time: the 10s grace elapses instantly once awaited.
        served.await.unwrap();
        assert_eq!(rx.await.unwrap(), AccessDecision::Allowed);
    }

    #[test]
    fn summaries_describe_the_request_with_shortened_ids() {
        let summary = AccessRequestSummary {
            client_id: "0123456789abcdef".into(),
            user_id: Some("fedcba9876543210".into()),
            permission: "join the session".into(),
        };
        assert_eq!(
            summary.describe(),
            "Studio client 01234567 (user fedcba98) wants to join the session"
        );
        assert_eq!(summary.subject(), "fedcba9876543210");
    }
}
