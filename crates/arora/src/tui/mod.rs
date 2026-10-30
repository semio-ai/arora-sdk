//! The terminal operator UI: a device header, a scrollable log pane, a strip of
//! live indicators, and the prompt line the operator answers questions on.
//!
//! `State` holds everything shown and all the input logic (drawing-free, so it
//! is unit-tested on its own); `view` renders a frame from an immutable
//! `&State`. This module is the moving part around them: it owns the `State`
//! behind a shared mutex, runs the terminal in a background thread (input,
//! periodic ticks, an own-process CPU sample, and drawing), and captures the
//! `log` crate into the pane. [`Tui`](crate::tui::Tui) is also an
//! [`Operator`](crate::operator::Operator): it asks the
//! operator by pushing a `Prompt` and awaiting its reply, so a question raised
//! anywhere in the runtime appears in the prompt line. In a `studio-bridge`
//! build it is its own [`Enroller`](crate::studio::Enroller): it asks whether
//! to enroll the device with Semio Studio and what describes it, shows each
//! enrollment code in the prompt line, and the device's Studio record and id
//! in the header.

mod state;
mod view;

use std::io::{self, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::Terminal;
use sysinfo::{ProcessesToUpdate, System};

use arora_bridge::{AccessDecision, DeviceInfo};

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::operator::{
    AccessRequestSummary, AccessRuling, Frontend, Operator, DEFAULT_ACCESS_GRACE,
};
use arora_types::data::Subscription;
use state::State;

/// The shared UI state: the render thread, the input handling, the log capture,
/// and the operator prompts all touch this one instance.
type SharedState = Arc<Mutex<State>>;

/// A key-driven command an embedding application adds to the terminal UI
/// (via [`commands_frontend`]): the footer advertises it, and pressing its key
/// outside a prompt fires it.
pub struct TuiCommand {
    /// The key that triggers the command. `q` (quit) and `m` (toggle the mouse
    /// between scrolling the logs and selecting text) are the UI's own
    /// shortcuts and cannot trigger a command.
    pub key: char,
    /// What the footer shows next to the key, e.g. `load GLB`.
    pub label: String,
    /// When set, the key opens a text prompt with this label and the command
    /// fires with what the operator types (an empty answer cancels it). When
    /// `None`, the command fires immediately.
    pub prompt: Option<String>,
}

/// A fired [`TuiCommand`]: its trigger key, and the prompt answer (`None` for
/// a prompt-less command).
#[derive(Debug, PartialEq, Eq)]
pub struct TuiCommandEvent {
    pub key: char,
    pub input: Option<String>,
}

/// Pick the standard front end — the terminal UI on an interactive terminal,
/// the headless front end otherwise — with `commands` installed when the
/// terminal UI is the pick. The receiver yields one event per fired command;
/// on the headless front end it never yields (its sender side is dropped —
/// nobody types where nobody watches).
pub fn commands_frontend(
    commands: Vec<TuiCommand>,
) -> (Frontend, UnboundedReceiver<TuiCommandEvent>) {
    let (tx, rx) = mpsc::unbounded();
    {
        use std::io::IsTerminal;
        if io::stdout().is_terminal() {
            match tui_frontend_with_commands(commands, Some(tx)) {
                Ok(frontend) => return (frontend, rx),
                Err(e) => eprintln!("arora: terminal UI unavailable ({e}); running headless"),
            }
        }
    }
    (crate::operator::default_frontend(), rx)
}

/// A live terminal operator UI. It owns the render thread and restores the
/// terminal when dropped; as an [`Operator`] it renders questions in the
/// prompt line and resolves them from the reply typed there.
pub struct Tui {
    state: SharedState,
    running: Arc<AtomicBool>,
    restored: Arc<AtomicBool>,
    ui: Mutex<Option<JoinHandle<()>>>,
}

impl Tui {
    /// Take over the terminal: install the log capture, enter the alternate
    /// screen, and spawn the render/input thread. `commands` (with their event
    /// sender) are the embedding application's — empty for the plain device UI.
    fn start(
        commands: Vec<TuiCommand>,
        command_tx: Option<UnboundedSender<TuiCommandEvent>>,
    ) -> anyhow::Result<Self> {
        let mut initial = State::new();
        initial.commands = commands;
        initial.command_tx = command_tx;
        let state: SharedState = Arc::new(Mutex::new(initial));
        install_logger(state.clone());
        let terminal = setup_terminal()?;
        let running = Arc::new(AtomicBool::new(true));
        let restored = Arc::new(AtomicBool::new(false));
        let ui = std::thread::Builder::new()
            .name("arora-tui".to_string())
            .spawn({
                let state = state.clone();
                let running = running.clone();
                let restored = restored.clone();
                move || run_ui(terminal, state, running, restored)
            })?;
        Ok(Self {
            state,
            running,
            restored,
            ui: Mutex::new(Some(ui)),
        })
    }
}

#[async_trait]
impl Operator for Tui {
    async fn ask_text(&self, label: &str, required: bool) -> Option<String> {
        ask_text(&self.state, label, required).await
    }

    async fn decide_access(&self, request: &AccessRequestSummary) -> AccessRuling {
        let options = vec![
            "Allow".to_string(),
            "Reject".to_string(),
            "Always allow".to_string(),
            "Always reject".to_string(),
        ];
        // Default to allowing after the grace period, matching the unattended
        // operator's fallback when the operator does not answer in time.
        let (question, reply) =
            state::decision_prompt(request.describe(), options, Some((DEFAULT_ACCESS_GRACE, 0)));
        if let Ok(mut state) = self.state.lock() {
            state.prompts.push_back(question);
        }
        match reply.await.unwrap_or(0) {
            1 => AccessRuling {
                decision: AccessDecision::Rejected,
                remember: false,
            },
            2 => AccessRuling {
                decision: AccessDecision::Allowed,
                remember: true,
            },
            3 => AccessRuling {
                decision: AccessDecision::Rejected,
                remember: true,
            },
            _ => AccessRuling {
                decision: AccessDecision::Allowed,
                remember: false,
            },
        }
    }

    #[cfg(feature = "studio-bridge")]
    fn enroller(self: Arc<Self>) -> Arc<dyn crate::studio::Enroller> {
        Arc::new(Enroller {
            state: self.state.clone(),
        })
    }
}

/// Asks `label` in the prompt line of the UI showing `state`, and returns
/// the answer typed there: `None` when skipped, which only an answer not
/// `required` may be, or once the UI has stopped.
async fn ask_text(state: &SharedState, label: &str, required: bool) -> Option<String> {
    let question = state::text_prompt(label.to_string(), required);
    let reply = state.lock().ok()?.ask(question)?;
    reply.await.ok().flatten()
}

/// The question offering to enroll a device that is not enrolled with Semio
/// Studio, and its answers: the first enrolls it.
#[cfg(feature = "studio-bridge")]
const OFFER_LABEL: &str = "This device is not enrolled with Semio Studio";
#[cfg(feature = "studio-bridge")]
const OFFER_OPTIONS: [&str; 2] = ["Enroll it", "Not now"];

/// The prompt for the name of a device that enrolls: left empty, the device
/// enrolls with a generated name, so it is never nameless.
#[cfg(feature = "studio-bridge")]
const NAME_LABEL: &str = "Device name — leave empty for a generated name";

/// The prompt for the model family of a device that enrolls when neither
/// `MODEL_FAMILY` nor the HAL gives one: left empty, the device enrolls with
/// no model family.
#[cfg(feature = "studio-bridge")]
const MODEL_FAMILY_LABEL: &str = "Model family — optional, leave empty to skip";

/// The terminal UI presenting the device's enrollment with Semio Studio
/// ([`Tui::enroller`](Operator::enroller)): it asks in the prompt line,
/// shows each code there while no question waits, and the device's record
/// and id in the header.
#[cfg(feature = "studio-bridge")]
struct Enroller {
    state: SharedState,
}

#[cfg(feature = "studio-bridge")]
#[async_trait]
impl crate::studio::Enroller for Enroller {
    /// Waits for the operator's answer, for as long as that takes; a terminal
    /// UI that has stopped declines.
    async fn offer_enrollment(&self) -> bool {
        let options = OFFER_OPTIONS
            .iter()
            .map(|option| option.to_string())
            .collect();
        let question = state::decision_prompt(OFFER_LABEL.to_string(), options, None);
        let reply = self
            .state
            .lock()
            .ok()
            .and_then(|mut state| state.ask(question));
        match reply {
            Some(reply) => matches!(reply.await, Ok(0)),
            None => false,
        }
    }

    async fn ask_name(&self) -> Option<String> {
        ask_text(&self.state, NAME_LABEL, false).await
    }

    async fn ask_model_family(&self) -> Option<String> {
        ask_text(&self.state, MODEL_FAMILY_LABEL, false).await
    }

    /// Shows nothing once the UI has stopped.
    fn show_code(&self, code: Option<&crate::studio::EnrollmentCode>) -> bool {
        match self.state.lock() {
            Ok(mut state) if !state.stopped => {
                state.enrollment_code = code.map(state::EnrollmentCode::from);
                true
            }
            _ => false,
        }
    }

    fn show_device_id(&self, device_id: &str) {
        if let Ok(mut state) = self.state.lock() {
            state.identity.device_id = Some(device_id.to_string());
        }
    }

    fn show_record(&self, record: Option<&DeviceInfo>) {
        if let Ok(mut state) = self.state.lock() {
            state.identity.show_record(record);
        }
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        // An enroller outliving the UI asks nothing more, and its questions
        // waiting get no answer.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .stop();
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.ui.lock().ok().and_then(|mut ui| ui.take()) {
            let _ = handle.join();
        }
        restore_terminal(&self.restored);
    }
}

/// Build a [`Frontend`] driven by the terminal UI: the [`Tui`] answers the
/// operator's questions, and the ready hook feeds it the live telemetry handle
/// and the device identity once the runtime and bridge are up.
pub(crate) fn tui_frontend() -> anyhow::Result<Frontend> {
    tui_frontend_with_commands(Vec::new(), None)
}

/// [`tui_frontend`] with an embedding application's commands installed — the
/// terminal-UI arm of [`commands_frontend`].
fn tui_frontend_with_commands(
    commands: Vec<TuiCommand>,
    command_tx: Option<UnboundedSender<TuiCommandEvent>>,
) -> anyhow::Result<Frontend> {
    let tui = Arc::new(Tui::start(commands, command_tx)?);
    let state = tui.state.clone();
    let operator: Arc<dyn Operator> = tui.clone();
    let on_ready = Box::new(
        move |device_state: Subscription, info: Option<DeviceInfo>, device_id: Option<String>| {
            if let Ok(mut state) = state.lock() {
                state.device_state = Some(device_state);
                state.identity.show_known(info.as_ref(), device_id);
            }
        },
    );
    Ok(Frontend { operator, on_ready })
}

/// The render/input thread: drain terminal events into the state, advance
/// deadline-driven prompts, sample own-process CPU, and draw — until the handle
/// is dropped or the operator asks to quit.
fn run_ui(
    mut terminal: Terminal<CrosstermBackend<Stdout>>,
    state: SharedState,
    running: Arc<AtomicBool>,
    restored: Arc<AtomicBool>,
) {
    // Without a process table or our own pid, the indicator shows no CPU.
    let mut system = System::new().ok();
    let pid = sysinfo::get_current_pid().ok();
    // Backdate so the first pass takes a CPU sample immediately.
    let mut last_cpu_sample = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);
    let mut quit = false;
    // What the terminal was last told about mouse capture: on, from
    // `setup_terminal`.
    let mut mouse_captured = true;

    while running.load(Ordering::SeqCst) {
        // Terminal events (the poll timeout also paces the redraw).
        if event::poll(Duration::from_millis(100)).unwrap_or(false) {
            if let Ok(event) = event::read() {
                if let Ok(mut state) = state.lock() {
                    match event {
                        Event::Key(key) if key.kind == KeyEventKind::Press => state.handle_key(key),
                        Event::Mouse(mouse) => state.handle_mouse(mouse),
                        _ => {}
                    }
                }
            }
        }

        // Own-process CPU, sampled about once a second.
        if last_cpu_sample.elapsed() >= Duration::from_secs(1) {
            let cpu = system.as_mut().zip(pid).and_then(|(system, pid)| {
                system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
                system.process(pid).map(|process| process.cpu_usage())
            });
            last_cpu_sample = Instant::now();
            if let Ok(mut state) = state.lock() {
                state.cpu_percent = cpu;
            }
        }

        let now = Instant::now();
        if let Ok(mut state) = state.lock() {
            state.read_device_state();
            state.tick(now);
            if state.quit_requested {
                quit = true;
                break;
            }
            // The operator toggled the mouse (`m`): apply it here, where the
            // terminal is owned. Without capture, the terminal selects text.
            if state.mouse_capture != mouse_captured {
                let applied = if state.mouse_capture {
                    execute!(terminal.backend_mut(), EnableMouseCapture)
                } else {
                    execute!(terminal.backend_mut(), DisableMouseCapture)
                };
                if applied.is_ok() {
                    mouse_captured = state.mouse_capture;
                }
            }
            let size = terminal.size().unwrap_or_default();
            state.viewport_height = view::log_height(size, state.prompt_line_shown()) as usize;
            let _ = terminal.draw(|frame| view::draw(frame, &state, now));
        }
    }

    restore_terminal(&restored);
    if quit {
        // The operator asked to quit. The synchronous step loop on the main
        // thread does not return on its own, so end the process now that the
        // terminal is back to its normal state.
        std::process::exit(0);
    }
}

/// The pane the global logger currently feeds. The `log` crate's logger can
/// only ever be set once per process, so the logger itself is a fixed shim
/// over this slot — each terminal UI (a host may run several in sequence,
/// rebuilding its device) retargets the slot to its own state.
static PANE: std::sync::RwLock<Option<(SharedState, env_filter::Filter)>> =
    std::sync::RwLock::new(None);

/// Capture the `log` crate into the pane, filtered the `env_logger` way
/// (`RUST_LOG`, defaulting to `info`).
fn install_logger(state: SharedState) {
    let mut builder = env_filter::Builder::new();
    match std::env::var("RUST_LOG") {
        Ok(directives) => {
            builder.parse(&directives);
        }
        Err(_) => {
            builder.filter_level(log::LevelFilter::Info);
        }
    }
    let filter = builder.build();
    log::set_max_level(filter.filter());
    if let Ok(mut pane) = PANE.write() {
        *pane = Some((state, filter));
    }
    let _ = log::set_boxed_logger(Box::new(TuiLogger));
}

/// A `log::Log` that appends matching records to the active pane's state.
struct TuiLogger;

impl log::Log for TuiLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        let pane = PANE.read().expect("pane lock");
        matches!(&*pane, Some((_, filter)) if filter.enabled(metadata))
    }

    fn log(&self, record: &log::Record) {
        let Ok(pane) = PANE.read() else { return };
        if let Some((state, filter)) = &*pane {
            if !filter.matches(record) {
                return;
            }
            if let Ok(mut state) = state.lock() {
                state.push_log(
                    record.level(),
                    record.target().to_string(),
                    record.args().to_string(),
                );
            }
        }
    }

    fn flush(&self) {}
}

/// Enter raw mode + the alternate screen with mouse capture.
fn setup_terminal() -> anyhow::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

/// Leave the alternate screen, release the mouse, and drop raw mode.
/// Idempotent: only the first call restores, so the render thread and the
/// handle's `Drop` can both call it. Releasing a mouse the operator already
/// released (`m`) is harmless: it re-sends the same reset sequences.
fn restore_terminal(restored: &AtomicBool) {
    if restored.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
}

#[cfg(all(test, feature = "studio-bridge"))]
mod tests {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::state::Prompt;
    use super::*;
    use crate::studio::Enroller as _;

    fn press(state: &SharedState, code: KeyCode) {
        state
            .lock()
            .unwrap()
            .handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn typed(state: &SharedState, text: &str) {
        for c in text.chars() {
            press(state, KeyCode::Char(c));
        }
        press(state, KeyCode::Enter);
    }

    /// What the prompt line asks: its label or message, and its options.
    fn asked(state: &SharedState) -> Option<(String, Vec<String>)> {
        match state.lock().unwrap().prompts.front() {
            Some(Prompt::Decision {
                message, options, ..
            }) => Some((message.clone(), options.clone())),
            Some(Prompt::Text { label, .. }) => Some((label.clone(), Vec::new())),
            _ => None,
        }
    }

    /// What `question` resolves to once answered, failing the test when it
    /// is not within ten seconds.
    async fn answered<T>(question: impl std::future::Future<Output = T>) -> T {
        tokio::time::timeout(Duration::from_secs(10), question)
            .await
            .expect("the answer resolves the question")
    }

    fn enroller() -> (SharedState, Enroller) {
        let state = Arc::new(Mutex::new(State::new()));
        let enroller = Enroller {
            state: state.clone(),
        };
        (state, enroller)
    }

    /// The offer to enroll waits in the prompt line for the operator's
    /// answer: the first option enrolls, the second declines.
    #[tokio::test]
    async fn the_operator_accepts_or_declines_the_offer_to_enroll() {
        let (state, enroller) = enroller();
        let accepted = enroller.offer_enrollment();
        futures::pin_mut!(accepted);
        assert!(futures::poll!(&mut accepted).is_pending());
        assert_eq!(
            asked(&state),
            Some((
                OFFER_LABEL.to_string(),
                OFFER_OPTIONS.map(str::to_string).to_vec()
            ))
        );
        press(&state, KeyCode::Enter);
        assert!(answered(accepted).await);

        let declined = enroller.offer_enrollment();
        futures::pin_mut!(declined);
        assert!(futures::poll!(&mut declined).is_pending());
        press(&state, KeyCode::Right);
        press(&state, KeyCode::Enter);
        assert!(!answered(declined).await);
    }

    /// The device's name and model family are asked in the prompt line, and
    /// are what the operator types there.
    #[tokio::test]
    async fn the_operator_names_the_device_and_its_family() {
        let (state, enroller) = enroller();
        let name = enroller.ask_name();
        futures::pin_mut!(name);
        assert!(futures::poll!(&mut name).is_pending());
        assert_eq!(asked(&state), Some((NAME_LABEL.to_string(), Vec::new())));
        typed(&state, "pepper-lab");
        assert_eq!(answered(name).await.as_deref(), Some("pepper-lab"));

        let family = enroller.ask_model_family();
        futures::pin_mut!(family);
        assert!(futures::poll!(&mut family).is_pending());
        assert_eq!(
            asked(&state),
            Some((MODEL_FAMILY_LABEL.to_string(), Vec::new()))
        );
        typed(&state, "pepper");
        assert_eq!(answered(family).await.as_deref(), Some("pepper"));
    }

    /// Once the UI has stopped, a question waiting gets no answer and a new
    /// one is not asked: the offer is declined, the name not given, and a
    /// code not shown.
    #[tokio::test]
    async fn a_stopped_ui_asks_nothing_and_shows_no_code() {
        let (state, enroller) = enroller();
        let waiting = enroller.offer_enrollment();
        futures::pin_mut!(waiting);
        assert!(futures::poll!(&mut waiting).is_pending());
        state.lock().unwrap().stop();
        assert!(!answered(waiting).await);

        assert!(!answered(enroller.offer_enrollment()).await);
        assert_eq!(answered(enroller.ask_name()).await, None);
        assert!(state.lock().unwrap().prompts.is_empty());
        assert!(!enroller.show_code(None));
    }

    /// A code cleared leaves the prompt line; the device's id and record
    /// show in the header.
    #[test]
    fn a_code_clears_and_the_id_and_the_record_show() {
        let (state, enroller) = enroller();
        state.lock().unwrap().enrollment_code = Some(state::EnrollmentCode {
            code: "BCDF-GHJK".to_string(),
            approval_url: None,
        });
        assert!(enroller.show_code(None));
        assert_eq!(state.lock().unwrap().enrollment_code, None);

        enroller.show_device_id("device-1");
        enroller.show_record(Some(&DeviceInfo {
            name: Some("Robo".to_string()),
            model_family: Some("NAO".to_string()),
            owners: vec!["alice".to_string()],
            ..DeviceInfo::default()
        }));
        assert_eq!(
            state.lock().unwrap().identity,
            state::DeviceIdentity {
                name: Some("Robo".to_string()),
                device_id: Some("device-1".to_string()),
                model_family: Some("NAO".to_string()),
                owners: vec!["alice".to_string()],
            }
        );
    }
}
