//! The TUI's state and input logic, free of any terminal drawing so it is
//! unit-testable: the log ring buffer, the scroll position, the prompt queue,
//! and how key/mouse events mutate them.

use std::collections::VecDeque;
use std::time::{Duration, Instant, SystemTime};

use futures::channel::oneshot;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

use arora_behavior::built_in;
use arora_types::data::{Key, Subscription};
use arora_types::value::Value;

/// How many log lines the pane remembers.
pub(crate) const LOG_CAPACITY: usize = 4000;
/// Lines scrolled per mouse-wheel notch.
const WHEEL_LINES: usize = 3;

/// Who this device is, for the header line.
#[derive(Debug, Clone, Default)]
pub struct DeviceIdentity {
    pub name: Option<String>,
    pub device_id: Option<String>,
    pub model_family: Option<String>,
    pub owners: Vec<String>,
}

/// One captured log line.
pub(crate) struct LogLine {
    /// Seconds since the Unix epoch (rendered as UTC HH:MM:SS).
    pub epoch_secs: u64,
    pub level: log::Level,
    pub target: String,
    pub message: String,
}

/// A question waiting on (or being answered in) the prompt line.
pub(crate) enum Prompt {
    /// Free-text question; answered with `None` when skipped (allowed only
    /// when not required).
    Text {
        label: String,
        required: bool,
        tx: Option<oneshot::Sender<Option<String>>>,
    },
    /// Pick-one question; resolves to the selected option's index. When
    /// `deadline` passes unanswered, resolves to its default index.
    Decision {
        message: String,
        options: Vec<String>,
        selected: usize,
        deadline: Option<(Instant, usize)>,
        tx: Option<oneshot::Sender<usize>>,
    },
}

impl Prompt {
    fn resolve_text(&mut self, answer: Option<String>) {
        if let Prompt::Text { tx, .. } = self {
            if let Some(tx) = tx.take() {
                let _ = tx.send(answer);
            }
        }
    }

    fn resolve_decision(&mut self, choice: usize) {
        if let Prompt::Decision { tx, .. } = self {
            if let Some(tx) = tx.take() {
                let _ = tx.send(choice);
            }
        }
    }
}

/// Everything the TUI displays and mutates. One instance, behind the shared
/// mutex; the render thread and the handle both touch it.
pub(crate) struct State {
    pub logs: VecDeque<LogLine>,
    /// 0 = follow the tail; N = scrolled up by N lines.
    pub scroll_from_bottom: usize,
    /// Log-pane height from the last draw, for page-sized scrolling.
    pub viewport_height: usize,
    pub identity: DeviceIdentity,
    /// The device's state as it changes, opening on everything the store held
    /// when the front end attached.
    pub device_state: Option<Subscription>,
    /// Step frequency, from the `dt` the device publishes each step.
    pub loop_hz: Option<f32>,
    /// Own-process CPU usage (percent of one core), sampled by the UI thread.
    pub cpu_percent: Option<f32>,
    /// Front = the prompt currently shown in the prompt line.
    pub prompts: VecDeque<Prompt>,
    /// The text being typed for a `Prompt::Text`.
    pub input: String,
    /// One-shot hint shown in the shortcuts line (e.g. "an answer is required").
    pub hint: Option<String>,
    /// Set when the operator asked to quit (Ctrl-C, or `q` outside a prompt).
    pub quit_requested: bool,
}

impl State {
    pub fn new() -> Self {
        Self {
            logs: VecDeque::new(),
            scroll_from_bottom: 0,
            viewport_height: 20,
            identity: DeviceIdentity::default(),
            device_state: None,
            loop_hz: None,
            cpu_percent: None,
            prompts: VecDeque::new(),
            input: String::new(),
            hint: None,
            quit_requested: false,
        }
    }

    /// Append a log message (one `LogLine` per text line, so scrolling stays
    /// line-accurate), keeping the view visually stable when the operator has
    /// scrolled up.
    pub fn push_log(&mut self, level: log::Level, target: String, message: String) {
        let epoch_secs = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut lines = message.lines();
        let first = lines.next().unwrap_or("").to_string();
        self.push_log_line(LogLine {
            epoch_secs,
            level,
            target,
            message: first,
        });
        for line in lines {
            self.push_log_line(LogLine {
                epoch_secs,
                level,
                target: String::new(),
                message: line.to_string(),
            });
        }
    }

    fn push_log_line(&mut self, line: LogLine) {
        self.logs.push_back(line);
        if self.logs.len() > LOG_CAPACITY {
            self.logs.pop_front();
        } else if self.scroll_from_bottom > 0 {
            self.scroll_from_bottom = (self.scroll_from_bottom + 1).min(self.max_scroll());
        }
    }

    fn max_scroll(&self) -> usize {
        self.logs.len().saturating_sub(1)
    }

    fn scroll_up(&mut self, lines: usize) {
        self.scroll_from_bottom = (self.scroll_from_bottom + lines).min(self.max_scroll());
    }

    fn scroll_down(&mut self, lines: usize) {
        self.scroll_from_bottom = self.scroll_from_bottom.saturating_sub(lines);
    }

    /// Drop the (resolved) front prompt and reset the edit line for the next.
    fn finish_front_prompt(&mut self) {
        self.prompts.pop_front();
        self.input.clear();
        self.hint = None;
    }

    /// Take what the device published since the last read and update the
    /// indicators derived from it: the step frequency, from the frame `dt`.
    pub fn read_device_state(&mut self) {
        let Some(feed) = self.device_state.as_ref() else {
            return;
        };
        let mut changes = Vec::new();
        while let Some(change) = feed.try_recv() {
            changes.push(change);
        }
        for change in changes {
            if let Some(Some(Value::U64(dt_ns))) = change.set.get(&Key::from(built_in::DT)) {
                self.loop_hz = (*dt_ns > 0).then(|| 1e9 / *dt_ns as f32);
            }
        }
    }

    /// Advance time-based behavior: a decision prompt whose deadline passed
    /// resolves to its default option.
    pub fn tick(&mut self, now: Instant) {
        if let Some(Prompt::Decision {
            deadline: Some((deadline, default)),
            ..
        }) = self.prompts.front()
        {
            if now >= *deadline {
                let default = *default;
                self.prompts
                    .front_mut()
                    .expect("front exists")
                    .resolve_decision(default);
                self.finish_front_prompt();
            }
        }
    }

    pub fn handle_mouse(&mut self, event: MouseEvent) {
        match event.kind {
            MouseEventKind::ScrollUp => self.scroll_up(WHEEL_LINES),
            MouseEventKind::ScrollDown => self.scroll_down(WHEEL_LINES),
            _ => {}
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        // Ctrl-C always requests quit (raw mode swallows the signal).
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit_requested = true;
            return;
        }
        match key.code {
            KeyCode::PageUp => self.scroll_up(self.viewport_height.max(1)),
            KeyCode::PageDown => self.scroll_down(self.viewport_height.max(1)),
            KeyCode::Home => self.scroll_up(self.logs.len()),
            KeyCode::End => self.scroll_from_bottom = 0,
            _ => self.handle_prompt_key(key),
        }
    }

    fn handle_prompt_key(&mut self, key: KeyEvent) {
        match self.prompts.front_mut() {
            None => {
                if key.code == KeyCode::Char('q') {
                    self.quit_requested = true;
                }
            }
            Some(Prompt::Text { required, .. }) => {
                let required = *required;
                match key.code {
                    KeyCode::Char(c) => {
                        self.input.push(c);
                        self.hint = None;
                    }
                    KeyCode::Backspace => {
                        self.input.pop();
                    }
                    KeyCode::Enter => {
                        let answer = self.input.trim().to_string();
                        if answer.is_empty() && required {
                            self.hint = Some("an answer is required".to_string());
                        } else {
                            let answer = (!answer.is_empty()).then_some(answer);
                            self.prompts
                                .front_mut()
                                .expect("front exists")
                                .resolve_text(answer);
                            self.finish_front_prompt();
                        }
                    }
                    _ => {}
                }
            }
            Some(Prompt::Decision {
                options, selected, ..
            }) => {
                let count = options.len().max(1);
                // Single-letter shortcuts: the first letter of each option
                // (case-insensitive) — e.g. A(llow), R(eject).
                let shortcut = |c: char| {
                    options
                        .iter()
                        .position(|o| o.chars().next().is_some_and(|f| f.eq_ignore_ascii_case(&c)))
                };
                match key.code {
                    KeyCode::Left => *selected = selected.checked_sub(1).unwrap_or(count - 1),
                    KeyCode::Right => *selected = (*selected + 1) % count,
                    KeyCode::Char(c) if c.is_ascii_digit() => {
                        let idx = (c as usize).wrapping_sub('1' as usize);
                        if idx < count {
                            *selected = idx;
                        }
                    }
                    KeyCode::Char(c) => {
                        if let Some(idx) = shortcut(c) {
                            self.prompts
                                .front_mut()
                                .expect("front exists")
                                .resolve_decision(idx);
                            self.finish_front_prompt();
                        }
                    }
                    KeyCode::Enter => {
                        let choice = *selected;
                        self.prompts
                            .front_mut()
                            .expect("front exists")
                            .resolve_decision(choice);
                        self.finish_front_prompt();
                    }
                    _ => {}
                }
            }
        }
    }

    /// The shortcut reminders for the current mode (the first prompt-line row).
    pub fn shortcuts(&self, now: Instant) -> String {
        if let Some(hint) = &self.hint {
            return format!("! {hint}");
        }
        match self.prompts.front() {
            None => "wheel/PgUp/PgDn: scroll logs · End: follow · q / Ctrl-C: quit".to_string(),
            Some(Prompt::Text { required, .. }) => {
                if *required {
                    "type the answer · Enter: submit".to_string()
                } else {
                    "type the answer · Enter: submit (empty: skip)".to_string()
                }
            }
            Some(Prompt::Decision {
                deadline, options, ..
            }) => {
                let mut s = "←/→ or 1-9: choose · Enter: confirm".to_string();
                if let Some((deadline, default)) = deadline {
                    let remaining = deadline.saturating_duration_since(now);
                    s.push_str(&format!(
                        " · auto \"{}\" in {}s",
                        options.get(*default).map(String::as_str).unwrap_or("?"),
                        remaining.as_secs()
                    ));
                }
                s
            }
        }
    }
}

/// Build a decision prompt (kept here next to [`Prompt`]; pushed by the
/// handle).
pub(crate) fn decision_prompt(
    message: String,
    options: Vec<String>,
    default_after: Option<(Duration, usize)>,
) -> (Prompt, oneshot::Receiver<usize>) {
    let (tx, rx) = oneshot::channel();
    let deadline = default_after.map(|(after, default)| (Instant::now() + after, default));
    (
        Prompt::Decision {
            message,
            options,
            selected: deadline.map(|(_, d)| d).unwrap_or(0),
            deadline,
            tx: Some(tx),
        },
        rx,
    )
}

/// Build a text prompt.
pub(crate) fn text_prompt(
    label: String,
    required: bool,
) -> (Prompt, oneshot::Receiver<Option<String>>) {
    let (tx, rx) = oneshot::channel();
    (
        Prompt::Text {
            label,
            required,
            tx: Some(tx),
        },
        rx,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn fill_logs(state: &mut State, n: usize) {
        for i in 0..n {
            state.push_log(log::Level::Info, "test".into(), format!("line {i}"));
        }
    }

    #[test]
    fn scrolling_clamps_to_the_buffer_and_follows_on_end() {
        let mut state = State::new();
        fill_logs(&mut state, 10);
        state.viewport_height = 4;
        state.handle_key(key(KeyCode::PageUp));
        assert_eq!(state.scroll_from_bottom, 4);
        state.handle_key(key(KeyCode::Home));
        assert_eq!(state.scroll_from_bottom, 9, "clamped to len - 1");
        state.handle_key(key(KeyCode::End));
        assert_eq!(state.scroll_from_bottom, 0);
    }

    #[test]
    fn appending_logs_keeps_a_scrolled_view_stable() {
        let mut state = State::new();
        fill_logs(&mut state, 10);
        state.scroll_from_bottom = 5;
        state.push_log(log::Level::Info, "t".into(), "new".into());
        assert_eq!(state.scroll_from_bottom, 6, "view did not slide");
    }

    #[test]
    fn text_prompt_requires_an_answer_when_required() {
        let mut state = State::new();
        let (prompt, mut rx) = text_prompt("Device name".into(), true);
        state.prompts.push_back(prompt);

        state.handle_key(key(KeyCode::Enter));
        assert!(state.hint.is_some(), "empty answer rejected");
        assert!(rx.try_recv().unwrap().is_none(), "not resolved yet");

        for c in "Robo".chars() {
            state.handle_key(key(KeyCode::Char(c)));
        }
        state.handle_key(key(KeyCode::Enter));
        assert_eq!(rx.try_recv().unwrap(), Some(Some("Robo".into())));
        assert!(state.prompts.is_empty());
    }

    #[test]
    fn optional_text_prompt_skips_on_empty_enter() {
        let mut state = State::new();
        let (prompt, mut rx) = text_prompt("Description".into(), false);
        state.prompts.push_back(prompt);
        state.handle_key(key(KeyCode::Enter));
        assert_eq!(rx.try_recv().unwrap(), Some(None));
    }

    #[test]
    fn decision_prompt_resolves_by_arrows_enter_and_shortcut() {
        let mut state = State::new();
        let options = vec!["Allow".to_string(), "Reject".to_string()];
        let (prompt, mut rx) = decision_prompt("join?".into(), options.clone(), None);
        state.prompts.push_back(prompt);
        state.handle_key(key(KeyCode::Right));
        state.handle_key(key(KeyCode::Enter));
        assert_eq!(rx.try_recv().unwrap(), Some(1), "arrow moved to Reject");

        let (prompt, mut rx) = decision_prompt("join?".into(), options, None);
        state.prompts.push_back(prompt);
        state.handle_key(key(KeyCode::Char('r')));
        assert_eq!(rx.try_recv().unwrap(), Some(1), "shortcut letter resolves");
    }

    #[test]
    fn decision_prompt_times_out_to_its_default() {
        let mut state = State::new();
        let (prompt, mut rx) = decision_prompt(
            "join?".into(),
            vec!["Allow".into(), "Reject".into()],
            Some((Duration::from_secs(10), 0)),
        );
        state.prompts.push_back(prompt);
        state.tick(Instant::now());
        assert!(rx.try_recv().unwrap().is_none(), "not expired yet");
        state.tick(Instant::now() + Duration::from_secs(11));
        assert_eq!(rx.try_recv().unwrap(), Some(0), "timed out to Allow");
        assert!(state.prompts.is_empty());
    }

    #[test]
    fn quit_needs_ctrl_c_or_q_outside_prompts() {
        let mut state = State::new();
        let (prompt, _rx) = text_prompt("Device name".into(), true);
        state.prompts.push_back(prompt);
        state.handle_key(key(KeyCode::Char('q')));
        assert!(!state.quit_requested, "q types into the prompt");
        assert_eq!(state.input, "q");
        state.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(state.quit_requested, "Ctrl-C always quits");

        let mut state = State::new();
        state.handle_key(key(KeyCode::Char('q')));
        assert!(state.quit_requested, "q quits outside prompts");
    }

    #[test]
    fn prompts_queue_one_at_a_time() {
        let mut state = State::new();
        let (p1, mut rx1) = text_prompt("first".into(), false);
        let (p2, mut rx2) = text_prompt("second".into(), false);
        state.prompts.push_back(p1);
        state.prompts.push_back(p2);
        state.handle_key(key(KeyCode::Char('a')));
        state.handle_key(key(KeyCode::Enter));
        assert_eq!(rx1.try_recv().unwrap(), Some(Some("a".into())));
        assert!(rx2.try_recv().unwrap().is_none(), "second still pending");
        assert_eq!(state.prompts.len(), 1);
    }
}
