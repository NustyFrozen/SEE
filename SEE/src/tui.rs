use crate::{tui_input, tui_input_date};
use chrono::{Local, NaiveDateTime, TimeZone};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use futures::task::noop_waker_ref;
use journald::JournalEntry;
use journald::reader::{JournalReader, JournalReaderConfig};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, LineGauge, List, ListItem, ListState, Padding};
use ratatui::widgets::{Clear, Paragraph};
use regex::Regex;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use tokio::task::JoinHandle;

use tokio::{self};
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum InputMode {
    InputFilter,
    InputFrom,
    InputTo,
    SelectLog,
    Unfocused,
    DetailedEntry,
}
pub(crate) struct SEETui {
    clipboard: Option<arboard::Clipboard>,
    pub unit: String,
    filter: tui_input::TuiInput,
    from: tui_input_date::TuiInputDate,
    to: tui_input_date::TuiInputDate,
    pub inputstate: InputMode,
    pub oldinputstate: InputMode,
    cursor_map: Vec<String>,
    log_data: Vec<ListItem<'static>>,
    fetch_task: Option<JoinHandle<(Vec<String>, Vec<ListItem<'static>>)>>,
    pub is_cancelled: Arc<AtomicBool>,
    lstate: ListState,
    tstate: ListState,
    temp_cursor_log: JournalEntry,
}
impl SEETui {
    pub const FOCUSED_COLOR: Color = Color::Rgb(121, 88, 221);
    pub const UNFOCUSED_COLOR: Color = Color::Rgb(91, 88, 91);
    pub fn refocus(&mut self) {
        self.reformwidgets();
    }
    pub fn dispose(&mut self) {
        self.is_cancelled.store(true, Ordering::SeqCst);

        if let Some(task) = self.fetch_task.take() {
            task.abort();
        }

        self.is_cancelled.store(false, Ordering::SeqCst);
    }
    fn select_log(&mut self) {
        if let Some(idx) = self.lstate.selected() {
            if let Some(cursor_data) = self.cursor_map.get(idx) {
                if !cursor_data.is_empty() {
                    let mut reader = match JournalReader::open(&JournalReaderConfig::default()) {
                        Ok(r) => r,
                        Err(_) => return,
                    };
                    reader
                        .add_filter(format!("_SYSTEMD_UNIT={}", self.unit).as_str())
                        .ok();
                    reader
                        .add_filter(format!("__CURSOR={}", cursor_data).as_str())
                        .ok();
                    if let Ok(Some(entry)) = reader.next_entry() {
                        self.temp_cursor_log = entry;
                        self.inputstate = InputMode::DetailedEntry;
                    }
                }
            }
        }
    }
    fn fetch_log_data(
        unit: String,
        filter: String,
        from: String,
        to: String,
        stop_flag: Arc<AtomicBool>,
    ) -> (Vec<String>, Vec<ListItem<'static>>) {
        let mut reader = match JournalReader::open(&JournalReaderConfig::default()) {
            Ok(r) => r,
            Err(_) => return (vec![], vec![]),
        };
        reader
            .add_filter(format!("_SYSTEMD_UNIT={}", unit).as_str())
            .ok();
        let from_us = SEETui::parse_human_time(&from);
        let to_us = if to.is_empty() {
            i64::MAX
        } else {
            SEETui::parse_human_time(&to)
        };

        let mut cursor = Vec::new();
        let mut items = Vec::new();
        let re = if !filter.is_empty() {
            Regex::new(&filter).ok()
        } else {
            None
        };
        let mut pid = String::new();
        while let Ok(Some(entry)) = reader.next_entry() {
            if stop_flag.load(Ordering::SeqCst) {
                return (cursor, items);
            }

            let wallclock = match entry.get_wallclock_time() {
                Some(ts) => ts.timestamp_us,
                None => continue,
            };
            if wallclock < from_us {
                continue;
            }
            if wallclock > to_us {
                break;
            }

            let message = entry.get_field("MESSAGE").unwrap_or_default();

            // Regex / Text Filter
            if let Some(ref regex) = re {
                if !regex.is_match(&message) {
                    continue;
                }
            } else if !filter.is_empty() {
                if !message.contains(filter.as_str()) {
                    continue;
                }
            }
            if let Some(newpid) = entry.get_field("_PID") {
                if newpid != pid {
                    cursor.push(String::new());
                    items.push(SEETui::format_styled_line(&entry, -1, newpid));
                    pid = newpid.to_string();
                }
            }
            if let Some(curs) = entry.get_field("__CURSOR") {
                cursor.push(curs.to_string());
            } else {
                cursor.push(String::new());
            }
            items.push(SEETui::format_styled_line(&entry, wallclock, &message));
        }
        return (cursor, items);
    }
    pub fn new(unit: String) -> Self {
        let mut res = Self {
            filter: tui_input::TuiInput::new(
                "📌Filter".to_string(),
                "Filter for example \"service (daemon) has not started \" or regex h*o w??rld"
                    .to_string(),
            ),
            lstate: ListState::default(),
            is_cancelled: Arc::new(AtomicBool::new(false)),
            fetch_task: None,
            oldinputstate: InputMode::InputFilter,
            inputstate: InputMode::SelectLog,
            from: tui_input_date::TuiInputDate::new("📅from".to_string()),
            to: tui_input_date::TuiInputDate::new("📅to".to_string()),
            unit: unit,
            log_data: vec![],
            cursor_map: vec![],
            temp_cursor_log: JournalEntry::new(),
            tstate: ListState::default(),
            clipboard: arboard::Clipboard::new().ok(),
        };
        res.run_fetch();
        res
    }
    pub fn reformwidgets(&mut self) -> bool {
        let mut stay_focus = true;

        match self.inputstate {
            InputMode::Unfocused => {
                stay_focus = false;
                self.filter.focused = false;
                self.from.focused = false;
                self.to.focused = false;
            }
            InputMode::InputTo => {
                self.filter.focused = false;
                self.from.focused = false;
                self.to.focused = true;
            }
            InputMode::InputFrom => {
                self.filter.focused = false;
                self.from.focused = true;
                self.to.focused = false;
            }
            InputMode::InputFilter => {
                self.filter.focused = true;
                self.from.focused = false;
                self.to.focused = false;
            }
            InputMode::SelectLog | InputMode::DetailedEntry => {
                self.filter.focused = false;
                self.from.focused = false;
                self.to.focused = false;
            }
        }
        if self.inputstate == InputMode::Unfocused {
            stay_focus = false;
        }
        stay_focus
    }
    fn run_fetch(&mut self) {
        // 1. Snapshot: Clone strings so the future OWNS them
        let u = self.unit.clone();
        let f = self.filter.input.clone();
        let from = self.from.input.clone();
        let to = self.to.input.clone();

        let is_cancelled = Arc::clone(&self.is_cancelled);

        // 3. The closure now moves the CLONES, not 'self'
        let handle = tokio::task::spawn_blocking(move || {
            // Use the clones here. 'self' is not touched!
            SEETui::fetch_log_data(u, f, from, to, is_cancelled)
        });

        // 4. Now 'self' is still available here!
        self.fetch_task = Some(handle);
    }
    fn yank(&mut self) {
        if let Some(idx) = self.tstate.selected() {
            if let Some((key, value)) = self.temp_cursor_log.fields.iter().nth(idx) {
                let text = format!("{}={}", key, value);
                self.to_clipboard(text);
            }
        }
    }

    pub fn to_clipboard(&mut self, text: String) {
        if let Some(ref mut clipctx) = self.clipboard {
            // We use .ok() or handle the result to keep it silent
            let _ = clipctx.set_text(text);
        }
    }
    //returns false if went out of focus
    pub fn run_widget(&mut self, area: Rect, frame: &mut Frame, keye: Option<KeyEvent>) -> bool {
        let mut stay_focus = true;
        let mut next_input = None;

        if let Some(key) = keye {
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

            // One big match on (State, KeyCode, IsControl)
            match (&self.inputstate, key.code, ctrl) {
                // --- NAVIGATION (SelectLog Mode + No Control) ---
                (InputMode::SelectLog, KeyCode::Char('k') | KeyCode::Up, false) => {
                    self.lstate.select_previous()
                }
                (InputMode::SelectLog, KeyCode::Char('j') | KeyCode::Down, false) => {
                    self.lstate.select_next()
                }
                (InputMode::DetailedEntry, KeyCode::Char('k') | KeyCode::Up, false) => {
                    self.tstate.select_previous()
                }
                (InputMode::DetailedEntry, KeyCode::Char('y'), false) => {
                    self.yank();
                }

                (InputMode::DetailedEntry, KeyCode::Char('j') | KeyCode::Down, false) => {
                    self.tstate.select_next()
                }
                (InputMode::DetailedEntry, KeyCode::Char('G'), false) => self.tstate.select_last(),
                (InputMode::DetailedEntry, KeyCode::Char('g'), false) => self.tstate.select_first(),
                (InputMode::SelectLog, KeyCode::Enter, false) => self.select_log(),

                (InputMode::SelectLog, KeyCode::Char('g'), false) => self.lstate.select_first(),
                (InputMode::SelectLog, KeyCode::Char('G'), false) => self.lstate.select_last(),
                (InputMode::SelectLog, KeyCode::PageUp, false) => {
                    (0..10).for_each(|_| self.lstate.select_previous())
                }
                (InputMode::SelectLog, KeyCode::PageDown, false) => {
                    (0..10).for_each(|_| self.lstate.select_next())
                }

                // --- GLOBAL: MOVE TO LOG LIST (Ctrl + Up/K) ---
                (mode, KeyCode::Char('k') | KeyCode::Up, true)
                    if mode != &InputMode::Unfocused && mode != &InputMode::SelectLog =>
                {
                    self.oldinputstate = self.inputstate;
                    self.inputstate = InputMode::SelectLog;
                }
                (InputMode::SelectLog, KeyCode::Char('t'), _) => {
                    self.oldinputstate = InputMode::SelectLog;
                    self.inputstate = InputMode::InputTo
                }
                (InputMode::SelectLog, KeyCode::Char('f'), _) => {
                    self.oldinputstate = InputMode::SelectLog;
                    self.inputstate = InputMode::InputFrom
                }
                // --- GLOBAL: EXIT LOG LIST / ENTER INPUT (Ctrl + Down/J OR I or /) ---
                (InputMode::SelectLog, KeyCode::Char('j') | KeyCode::Down, true)
                | (InputMode::SelectLog, KeyCode::Char('i') | KeyCode::Char('/'), false)
                | (InputMode::DetailedEntry, KeyCode::Char('q') | KeyCode::Esc, false) => {
                    self.inputstate = if self.inputstate == InputMode::DetailedEntry {
                        InputMode::SelectLog
                    } else if self.oldinputstate == InputMode::SelectLog {
                        InputMode::InputFilter
                    } else {
                        self.oldinputstate
                    };
                }

                // --- GLOBAL: MOVE RIGHT (Ctrl + Right/L) ---
                (InputMode::InputFilter, KeyCode::Char('l') | KeyCode::Right, true) => {
                    self.inputstate = InputMode::InputFrom
                }
                (InputMode::InputFrom, KeyCode::Char('l') | KeyCode::Right, true) => {
                    self.inputstate = InputMode::InputTo
                }

                // --- GLOBAL: MOVE LEFT (Ctrl + Left/H) ---
                (InputMode::InputFrom, KeyCode::Char('h') | KeyCode::Left, true) => {
                    self.inputstate = InputMode::InputFilter
                }
                (InputMode::InputTo, KeyCode::Char('h') | KeyCode::Left, true) => {
                    self.inputstate = InputMode::InputFrom
                }
                (_, KeyCode::Char('h') | KeyCode::Left, true) => {
                    self.oldinputstate = self.inputstate;
                    self.inputstate = InputMode::Unfocused;
                }
                // --- FALLTHROUGH ---
                _ => next_input = keye,
            }

            stay_focus = self.reformwidgets();
        }

        self.render(area, frame, next_input);
        stay_focus // STAY HARD
    }
    /// Render the UI with a table.
    fn render(&mut self, area: Rect, frame: &mut Frame, key: Option<KeyEvent>) {
        // --- STEP 1: Main Vertical Stack ---
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // log info footer
                Constraint::Min(3),    // Main Body (List + Table)
                Constraint::Length(3), // Search / Dates Row
            ])
            .split(area);

        // --- STEP 3: Search Controls Row (Horizontal Split) ---
        let controls_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(70), // User free search / regex
                Constraint::Percentage(15), // From date
                Constraint::Percentage(15), // To date
            ])
            .split(main_chunks[2]);

        self.render_logs(frame, main_chunks[1]);
        if self.filter.render_input(controls_chunks[0], frame, key)
            || self.from.render_input(controls_chunks[1], frame, key)
            || self.to.render_input(controls_chunks[2], frame, key)
        {
            self.is_cancelled.store(true, Ordering::SeqCst);

            if let Some(task) = self.fetch_task.take() {
                task.abort();
            }
            self.is_cancelled.store(false, Ordering::SeqCst);
            self.run_fetch();
        };
        self.render_footer(frame, main_chunks[0]);
        if self.inputstate == InputMode::DetailedEntry {
            self.render_detailed_entry(frame);
        }
    }
    fn render_footer(&mut self, frame: &mut Frame, area: Rect) {
        let pos = self.lstate.selected().unwrap_or(0) as f64 + 1_f64;
        let len = self.log_data.len().max(1) as f64;
        let ratio = pos / len;

        let line_gauge = LineGauge::default()
            .filled_style(Style::new().gray().on_gray().bold())
            .unfilled_style(Style::new().black().on_black())
            .ratio(ratio)
            .filled_symbol(" ")
            .unfilled_symbol(" ");
        let span1 = format!("{}", pos).light_magenta();
        let span2 = format!("{}", len).magenta();
        let line = Line::from(vec![span1, "/".into(), span2]);
        let text = Text::from(line);
        let controls_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Min(80),
                Constraint::Length(text.to_string().len() as u16),
            ])
            .split(area);
        frame.render_widget(Paragraph::new(text), controls_chunks[1]);
        frame.render_widget(line_gauge, controls_chunks[0]);
    }

    fn pull_data(&mut self) -> bool {
        if let Some(mut task) = self.fetch_task.take() {
            let mut cx = Context::from_waker(noop_waker_ref());

            match Pin::new(&mut task).poll(&mut cx) {
                Poll::Ready(Ok(new_items)) => {
                    self.log_data = new_items.1;
                    self.cursor_map = new_items.0;
                    return true;
                }
                Poll::Ready(Err(e)) => {
                    // Background thread panicked
                    eprintln!("Task failed: {:?}", e);
                }
                _ => {
                    // Background thread is still reading disk, keep UI moving!
                    self.fetch_task = Some(task);
                }
            }
        }
        false
    }
    fn render_detailed_entry(&mut self, frame: &mut Frame) {
        let screen_area = frame.area();
        let buffer = frame.buffer_mut();
        //dimming all elements
        for x in screen_area.left()..screen_area.right() {
            for y in screen_area.top()..screen_area.bottom() {
                let cell = buffer.cell_mut((x, y));
                if let Some(c) = cell {
                    c.set_style(
                        Style::default()
                            .add_modifier(Modifier::DIM)
                            .fg(Color::DarkGray),
                    );
                }
            }
        }

        let popup_block = Block::default()
            .title("Journal Entry -- y / yank | j / down | k / up")
            .borders(Borders::ALL)
            .bg(Color::Black); // Solid background to cover the dimmed cells
        let lszt = List::new(
            self.temp_cursor_log
                .fields
                .iter()
                .map(|x| {
                    ListItem::new(Line::from(vec![
                        Span::styled(x.0, Style::default().fg(Color::LightYellow)),
                        Span::styled("=", Style::default().fg(Color::Gray)),
                        Span::styled(x.1, Style::default().fg(Color::LightMagenta)),
                    ]))
                })
                .collect::<Vec<ListItem>>(),
        )
        .block(popup_block)
        .highlight_symbol(">>")
        .highlight_style(Style::new().white().bg(Color::DarkGray));
        let area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50 / 2),
                Constraint::Percentage(50),
                Constraint::Percentage(50 / 2),
            ])
            .split(
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Percentage(50 / 2),
                        Constraint::Percentage(50),
                        Constraint::Percentage(50 / 2),
                    ])
                    .split(screen_area)[1],
            )[1];
        frame.render_widget(Clear, area); // This clears the dimmed pixels in the popup area
        frame.render_stateful_widget(lszt, area, &mut self.tstate);
    }
    pub fn render_logs(&mut self, frame: &mut Frame, area: Rect) {
        let selecte_data = self.pull_data();
        let cool_block = Block::default()
            .borders(Borders::ALL)
            .padding(Padding::new(0, 1, 0, 0))
            .bg(Color::Rgb(20, 20, 25))
            .border_type(BorderType::Rounded)
            .border_style(if self.inputstate == InputMode::SelectLog {
                SEETui::FOCUSED_COLOR
            } else {
                SEETui::UNFOCUSED_COLOR
            });

        let list = List::new(self.log_data.clone())
            .block(cool_block)
            .highlight_symbol(">>")
            .bg(Color::Rgb(20, 20, 25))
            .highlight_style(Style::new().white().bg(SEETui::FOCUSED_COLOR));
        if selecte_data {
            let last_idx = list.len().saturating_sub(1);

            let list_height = area.height as usize;

            let total_items = list.len();
            let offset = if total_items > list_height {
                total_items - list_height
            } else {
                0
            };

            self.lstate.select(Some(last_idx));
            *self.lstate.offset_mut() = offset;
        }
        frame.render_stateful_widget(list, area, &mut self.lstate);
    }

    fn format_styled_line(
        entry: &journald::JournalEntry,
        wallclock: i64,
        message: &str,
    ) -> ListItem<'static> {
        let dt = Local.timestamp_nanos(wallclock * 1000);
        let date_str = dt.format("%m/%d/%Y").to_string();
        let time_str = dt.format("%H:%M:%S%.3f").to_string();
        let unit_raw = entry.get_field("_SYSTEMD_UNIT").unwrap_or("");
        let priority = entry.get_field("PRIORITY").unwrap_or("6");
        if wallclock == -1 {
            let line = Line::from(vec![
                // Insert the Emoji at the very beginning
                // Timestamp in Purple
                Span::styled(
                    format!("Started New Instance ➝ {}({})", unit_raw, message),
                    Style::default().fg(Color::Gray),
                ),
            ]);

            return ListItem::new(line.centered());
        }
        let display_message = if let Some(start_idx) = message.find("msg=\"") {
            let content_start = start_idx + 5; // Skip past the 'msg="'
            if let Some(end_offset) = message[content_start..].find('"') {
                &message[content_start..content_start + end_offset]
            } else {
                message // Missing closing quote, fallback to whole message
            }
        } else if let Some(start_idx) = message.find("msg=") {
            let content_start = start_idx + 4;
            if let Some(end_offset) = message[content_start..].find(' ') {
                &message[content_start..content_start + end_offset]
            } else {
                &message[content_start..]
            }
        } else {
            message
        };
        let (emoji, msg_style) = match priority {
            "0" | "1" | "2" | "3" => (
                // Emerg, Alert, Crit, Err
                "❌",
                Style::default().fg(Color::Red), // Error = Red
            ),
            "4" => (
                // Warning
                "⚠️",
                Style::default().fg(Color::Rgb(255, 165, 0)), // Warning = Orange
            ),
            "5" | "6" => (
                // Notice, Info
                "ℹ️",
                Style::default().fg(Color::Cyan), // Info = Cyan
            ),
            "7" => (
                // Debug
                "🐞",
                Style::default().fg(Color::Yellow), // Debug = Yellow
            ),
            _ => (
                // Unknown
                "❓",
                Style::default().fg(Color::White), // Unknown = White
            ),
        };

        // 3. Construct the Styled Line using Ratatui Spans
        let line = Line::from(vec![
            Span::styled(format!("{}", emoji), msg_style),
            Span::styled(date_str, Style::default().fg(Color::Indexed(170))),
            Span::styled(format!(" {}", time_str), Style::default().fg(Color::White)),
            // Offset in Blue/Cyan
            // Spacing
            Span::raw("  "),
            // The Message styled dynamically based on the priority level
            Span::styled(
                display_message
                    .replace("\t", "    ")
                    .replace('\r', "")
                    .to_string(),
                msg_style,
            ),
        ]);

        ListItem::new(line)
    }

    pub fn parse_human_time(s: &str) -> i64 {
        if s.is_empty() {
            return 0;
        }
        // Format: month/day/year hour:minute:second
        let fmt = "%m/%d/%Y %H:%M:%S";

        NaiveDateTime::parse_from_str(s, fmt)
            .ok()
            .and_then(|naive| Local.from_local_datetime(&naive).single())
            .map(|dt| dt.timestamp_micros())
            .unwrap_or(0)
    }
}
