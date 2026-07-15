//! The terminal operator UI: a device header, a scrollable log pane, a strip of
//! live indicators, and the prompt line the operator answers questions on.
//!
//! [`State`] holds everything shown and all the input logic (drawing-free, so it
//! is unit-tested on its own); [`view`] renders a frame from an immutable
//! `&State`. This module is the moving part around them: it owns the `State`
//! behind a shared mutex, runs the terminal in a background thread (input,
//! periodic ticks, an own-process CPU sample, and drawing), and captures the
//! `log` crate into the pane. [`Tui`] is also an [`Operator`]: it asks the
//! operator by pushing a [`Prompt`](state::Prompt) and awaiting its reply, so a
//! question raised anywhere in the runtime appears in the prompt line.

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

use crate::operator::{
    AccessRequestSummary, AccessRuling, Frontend, Operator, DEFAULT_ACCESS_GRACE,
};
use crate::runtime::Telemetry;
use state::{DeviceIdentity, State};

/// The shared UI state: the render thread, the input handling, the log capture,
/// and the operator prompts all touch this one instance.
type SharedState = Arc<Mutex<State>>;

/// A live terminal operator UI. Constructed through [`tui_frontend`], it owns the
/// render thread and restores the terminal when dropped; as an [`Operator`] it
/// renders questions in the prompt line and resolves them from the reply typed
/// there.
pub struct Tui {
    state: SharedState,
    running: Arc<AtomicBool>,
    restored: Arc<AtomicBool>,
    ui: Mutex<Option<JoinHandle<()>>>,
}

impl Tui {
    /// Take over the terminal: install the log capture, enter the alternate
    /// screen, and spawn the render/input thread.
    fn start() -> anyhow::Result<Self> {
        let state: SharedState = Arc::new(Mutex::new(State::new()));
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
        let (question, reply) = state::text_prompt(label.to_string(), required);
        if let Ok(mut state) = self.state.lock() {
            state.prompts.push_back(question);
        }
        reply.await.ok().flatten()
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
}

impl Drop for Tui {
    fn drop(&mut self) {
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
    let tui = Arc::new(Tui::start()?);
    let state = tui.state.clone();
    let operator: Arc<dyn Operator> = tui.clone();
    let on_ready = Box::new(
        move |telemetry: Telemetry, info: Option<DeviceInfo>, device_id: Option<String>| {
            if let Ok(mut state) = state.lock() {
                state.telemetry = Some(telemetry);
                state.identity = identity_from(info, device_id);
            }
        },
    );
    Ok(Frontend {
        operator,
        interactive: true,
        on_ready,
    })
}

/// The header identity, from what the bridge knows about the device.
fn identity_from(info: Option<DeviceInfo>, device_id: Option<String>) -> DeviceIdentity {
    DeviceIdentity {
        name: info.as_ref().and_then(|i| i.name.clone()),
        device_id,
        model_family: info.as_ref().and_then(|i| i.model_family.clone()),
        owners: info.map(|i| i.owners).unwrap_or_default(),
    }
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
    let mut system = System::new();
    let pid = sysinfo::get_current_pid().ok();
    // Backdate so the first pass takes a CPU sample immediately.
    let mut last_cpu_sample = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);
    let mut quit = false;

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
            let cpu = pid.and_then(|pid| {
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
            state.tick(now);
            if state.quit_requested {
                quit = true;
                break;
            }
            let size = terminal.size().unwrap_or_default();
            state.viewport_height =
                view::log_height(size, state.prompts.front().is_some()) as usize;
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
    let _ = log::set_boxed_logger(Box::new(TuiLogger { state, filter }));
}

/// A `log::Log` that appends matching records to the pane's state.
struct TuiLogger {
    state: SharedState,
    filter: env_filter::Filter,
}

impl log::Log for TuiLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        self.filter.enabled(metadata)
    }

    fn log(&self, record: &log::Record) {
        if !self.filter.matches(record) {
            return;
        }
        if let Ok(mut state) = self.state.lock() {
            state.push_log(
                record.level(),
                record.target().to_string(),
                record.args().to_string(),
            );
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

/// Leave the alternate screen and drop raw mode. Idempotent: only the first call
/// restores, so the render thread and the handle's `Drop` can both call it.
fn restore_terminal(restored: &AtomicBool) {
    if restored.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
}
