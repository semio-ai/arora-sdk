//! Turning the [`State`] into a frame. Rendering only: every function reads an
//! immutable `&State` and draws, so what appears on screen is a pure function of
//! the state the input side maintains.
//!
//! Top to bottom: the identity header, the scrollable log pane, a one-line
//! telemetry strip, the prompt line (present only while a question waits), and
//! the shortcuts line.

use std::time::Instant;

use ratatui::layout::{Constraint, Layout, Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::state::{LogLine, Prompt, State};

/// Draw the whole UI for one frame.
pub(crate) fn draw(frame: &mut Frame, state: &State, now: Instant) {
    let has_prompt = state.prompts.front().is_some();
    let regions = regions(frame.area(), has_prompt);
    frame.render_widget(header(state), regions[0]);
    frame.render_widget(logs(state, regions[1].height), regions[1]);
    frame.render_widget(telemetry(state), regions[2]);
    if regions[3].height > 0 {
        frame.render_widget(prompt(state), regions[3]);
    }
    frame.render_widget(shortcuts(state, now), regions[4]);
}

/// The height the log pane gets for a terminal of `size` — the same split
/// [`draw`] uses, so the input side can keep [`State::viewport_height`] in step
/// with what is actually shown.
pub(crate) fn log_height(size: Size, has_prompt: bool) -> u16 {
    regions(Rect::new(0, 0, size.width, size.height), has_prompt)[1].height
}

/// The vertical split: a one-line header, the flexible log pane, a one-line
/// telemetry strip, the two-line prompt (nothing when no question waits), and
/// the one-line shortcuts row.
fn regions(area: Rect, has_prompt: bool) -> std::rc::Rc<[Rect]> {
    Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(if has_prompt { 2 } else { 0 }),
        Constraint::Length(1),
    ])
    .split(area)
}

/// The identity header: who this device is, from whatever the bridge told us.
fn header(state: &State) -> Paragraph<'_> {
    let id = &state.identity;
    let mut parts = Vec::new();
    parts.push(id.name.clone().unwrap_or_else(|| "arora".to_string()));
    if let Some(model) = &id.model_family {
        parts.push(model.clone());
    }
    if let Some(device_id) = &id.device_id {
        parts.push(format!("id {}", crate::operator::shorten(device_id)));
    }
    if !id.owners.is_empty() {
        parts.push(format!("owners: {}", id.owners.join(", ")));
    }
    Paragraph::new(Line::from(Span::styled(
        parts.join("  ·  "),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )))
}

/// The log pane: the last `height` visible lines, offset by how far the operator
/// has scrolled up.
fn logs(state: &State, height: u16) -> Paragraph<'_> {
    let height = height as usize;
    let end = state.logs.len().saturating_sub(state.scroll_from_bottom);
    let start = end.saturating_sub(height);
    let lines: Vec<Line> = state
        .logs
        .iter()
        .take(end)
        .skip(start)
        .map(log_line)
        .collect();
    Paragraph::new(lines)
}

/// One timestamped, level-colored log row.
fn log_line(line: &LogLine) -> Line<'_> {
    let mut spans = vec![
        Span::styled(
            format!("{} ", clock(line.epoch_secs)),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(format!("{:<5} ", line.level), level_style(line.level)),
    ];
    if !line.target.is_empty() {
        spans.push(Span::styled(
            format!("{}: ", line.target),
            Style::default().fg(Color::DarkGray),
        ));
    }
    spans.push(Span::raw(&line.message));
    Line::from(spans)
}

/// The live indicators: own-process CPU and step-loop frequency.
fn telemetry(state: &State) -> Paragraph<'_> {
    let dim = Style::default().fg(Color::DarkGray);
    let cpu = match state.cpu_percent {
        Some(pct) => format!("CPU {pct:.0}%"),
        None => "CPU --".to_string(),
    };
    let hz = match state.loop_hz {
        Some(hz) => format!("loop {hz:.0} Hz"),
        None => "loop -- Hz".to_string(),
    };
    let spans = vec![Span::raw(cpu), Span::styled("  ·  ", dim), Span::raw(hz)];
    Paragraph::new(Line::from(spans))
}

/// The prompt line: the question on top, the way to answer it below.
fn prompt(state: &State) -> Paragraph<'_> {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let cursor = Style::default().add_modifier(Modifier::REVERSED);
    match state.prompts.front() {
        Some(Prompt::Text { label, .. }) => Paragraph::new(vec![
            Line::from(Span::styled(label.clone(), bold)),
            Line::from(vec![
                Span::raw(format!("> {}", state.input)),
                Span::styled(" ", cursor),
            ]),
        ]),
        Some(Prompt::Decision {
            message,
            options,
            selected,
            ..
        }) => {
            let mut spans = Vec::new();
            for (i, option) in options.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw("  "));
                }
                let style = if i == *selected {
                    Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
                } else {
                    Style::default()
                };
                spans.push(Span::styled(format!(" {}: {} ", i + 1, option), style));
            }
            Paragraph::new(vec![
                Line::from(Span::styled(message.clone(), bold)),
                Line::from(spans),
            ])
        }
        None => Paragraph::new(""),
    }
}

/// The shortcuts row (the state folds any one-shot hint into it).
fn shortcuts(state: &State, now: Instant) -> Paragraph<'_> {
    Paragraph::new(Line::from(Span::styled(
        state.shortcuts(now),
        Style::default().fg(Color::DarkGray),
    )))
}

/// UTC `HH:MM:SS` for a Unix timestamp, without pulling in a date library.
fn clock(epoch_secs: u64) -> String {
    let secs = epoch_secs % 86_400;
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

/// The color a level is drawn in.
fn level_style(level: log::Level) -> Style {
    let color = match level {
        log::Level::Error => Color::Red,
        log::Level::Warn => Color::Yellow,
        log::Level::Info => Color::Green,
        log::Level::Debug => Color::Blue,
        log::Level::Trace => Color::DarkGray,
    };
    Style::default().fg(color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// A full frame renders the identity, a captured log line, and the prompt a
    /// question is waiting on — all readable in the backing buffer.
    #[test]
    fn a_frame_shows_the_identity_logs_and_prompt() {
        let mut state = State::new();
        state.identity.name = Some("Robo".to_string());
        state.push_log(log::Level::Info, "arora".into(), "engine started".into());
        let (question, _rx) = super::super::state::text_prompt("Device name".into(), true);
        state.prompts.push_back(question);

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test backend");
        terminal
            .draw(|frame| draw(frame, &state, Instant::now()))
            .expect("draw");

        let rendered: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(rendered.contains("Robo"), "header shows the device name");
        assert!(rendered.contains("engine started"), "log line is drawn");
        assert!(rendered.contains("Device name"), "prompt label is drawn");
    }
}
