use crate::{fetch_services, tui_input};
use chrono::{Local, NaiveDateTime, TimeZone};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, poll, read};
use futures::task::noop_waker_ref;
use journald::reader::{JournalReader, JournalReaderConfig, JournalSeek};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::Clear;
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Padding};
use ratatui::{Frame, style};
use regex::Regex;
use std::collections::{BTreeMap, btree_map};
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
}
pub(crate) struct SEETui {
    pub unit: String,
    filter: tui_input::TuiInput,
    from: tui_input::TuiInput,
    to: tui_input::TuiInput,
    autofetch: bool,
    pub inputstate: InputMode,
    pub oldinputstate: InputMode,
    log_data: Vec<ListItem<'static>>,
    fetch_task: Option<JoinHandle<Vec<ListItem<'static>>>>,
    pub is_cancelled: Arc<AtomicBool>,
    lstate: ListState,
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
    fn fetch_log_data(
        unit: String,
        filter: String,
        from: String,
        to: String,
        stop_flag: Arc<AtomicBool>,
    ) -> Vec<ListItem<'static>> {
        let mut reader = match JournalReader::open(&JournalReaderConfig::default()) {
            Ok(r) => r,
            Err(_) => return vec![],
        };
        reader
            .add_filter(format!("_SYSTEMD_UNIT={}", unit).as_str())
            .ok();
        let from_us = parse_human_time(&from);
        let to_us = if to.is_empty() {
            i64::MAX
        } else {
            parse_human_time(&to)
        };
        let mut items = Vec::new();
        let re = if !filter.is_empty() {
            Regex::new(&filter).ok()
        } else {
            None
        };
        while let Ok(Some(entry)) = reader.next_entry() {
            let wallclock = match entry.get_wallclock_time() {
                Some(ts) => ts.timestamp_us,
                None => continue,
            };
            if stop_flag.load(Ordering::SeqCst) {
                return items;
            }
            // Manual Time Window Filtering
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
            } else if (!filter.is_empty()) {
                if (!message.contains(filter.as_str())) {
                    continue;
                }
            }

            // 3. Apply the Styling (Colors and Truncation)
            items.push(format_styled_line(&entry, wallclock, &message));
        }
        items
    }
    pub fn new(unit: String) -> Self {
        let mut res = Self {
            filter: tui_input::TuiInput::new(
                "Filter".to_string(),
                "Filter for example \"service (daemon) has not started \" or regex h*o w??rld"
                    .to_string(),
            ),
            lstate: ListState::default(),
            is_cancelled: Arc::new(AtomicBool::new(false)),
            fetch_task: None,
            oldinputstate: InputMode::InputFilter,
            inputstate: InputMode::SelectLog,
            from: tui_input::TuiInput::new("from".to_string(), "mm/dd/yyyy".to_string()),
            to: tui_input::TuiInput::new("To".to_string(), "mm/dd/yyyy".to_string()),
            autofetch: false,
            unit: unit,
            log_data: vec![],
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
            InputMode::SelectLog => {
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
    //returns false if went out of focus
    pub fn run_widget(&mut self, area: Rect, frame: &mut Frame, keye: Option<KeyEvent>) -> bool {
        //UI
        let mut stay_focus = true;
        let mut next_input = None;

        if let Some(key) = keye {
            //global key options
            match (key.code, key.modifiers.contains(KeyModifiers::CONTROL)) {
                // Move Up (k) - row index decreases
                (KeyCode::Char('k') | KeyCode::Up, true) => {
                    if self.inputstate != InputMode::Unfocused
                        && self.inputstate != InputMode::SelectLog
                    {
                        self.oldinputstate = self.inputstate;
                        self.inputstate = InputMode::SelectLog;
                    }
                }
                (KeyCode::Char('j') | KeyCode::Down, true) => {
                    if self.inputstate == InputMode::SelectLog {
                        self.inputstate = if self.oldinputstate == InputMode::SelectLog {
                            InputMode::InputFilter
                        } else {
                            self.oldinputstate
                        };
                    }
                }
                (KeyCode::Char('k') | KeyCode::Up, false) => {
                    self.lstate.select_previous();
                }
                (KeyCode::Char('j') | KeyCode::Down, false) => {
                    self.lstate.select_next();
                }
                (KeyCode::Char('g'), false) => {
                    self.lstate.select_first();
                }
                (KeyCode::Char('G'), false) => {
                    self.lstate.select_last();
                }
                (KeyCode::Char('l') | KeyCode::Right, true) => {
                    if self.inputstate == InputMode::InputFilter {
                        self.inputstate = InputMode::InputFrom;
                    } else if self.inputstate == InputMode::InputFrom {
                        self.inputstate = InputMode::InputTo
                    }
                }
                (KeyCode::Char('h') | KeyCode::Left, true) => {
                    if self.inputstate == InputMode::InputFrom {
                        self.inputstate = InputMode::InputFilter;
                    } else if self.inputstate == InputMode::InputTo {
                        self.inputstate = InputMode::InputFrom
                    } else {
                        self.oldinputstate = self.inputstate;
                        self.inputstate = InputMode::Unfocused;
                    }
                }
                (_, _) => {}
            }
            if !key.modifiers.contains(KeyModifiers::CONTROL) {
                next_input = keye;
            }
            stay_focus = self.reformwidgets();
        }

        self.render(area, frame, next_input);
        stay_focus // STAY HARD  :P
    }
    /// Render the UI with a table.
    fn render(&mut self, area: Rect, frame: &mut Frame, key: Option<KeyEvent>) {
        // --- STEP 1: Main Vertical Stack ---
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
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
            .split(main_chunks[1]);

        self.render_logs(frame, main_chunks[0]);
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
    }
    fn pull_data(&mut self) -> bool {
        if let Some(mut task) = self.fetch_task.take() {
            let mut cx = Context::from_waker(noop_waker_ref());

            match Pin::new(&mut task).poll(&mut cx) {
                Poll::Ready(Ok(new_items)) => {
                    self.log_data = new_items;
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
    pub fn render_logs(&mut self, frame: &mut Frame, area: Rect) {
        if self.pull_data() {
            self.lstate.select_last();
        }
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
            .highlight_style(Style::new().white().bg(SEETui::FOCUSED_COLOR));
        frame.render_widget(Clear, area);
        frame.render_stateful_widget(list, area, &mut self.lstate);
    }
}

fn format_styled_line(
    entry: &journald::JournalEntry,
    wallclock: i64,
    message: &str,
) -> ListItem<'static> {
    let dt = Local.timestamp_nanos(wallclock * 1000);
    let ts_str = dt.format("%Y-%m-%dT%H:%M:%S%.3f").to_string();
    let offset_str = dt.format("%:z").to_string();
    let offset_str = dt.format("%:z").to_string(); // 2. Metadata (Unit, PID, and Priority)
    let unit_raw = entry.get_field("_SYSTEMD_UNIT").unwrap_or("");
    let pid = entry.get_field("_PID").unwrap_or_else(|| "0".into());
    let priority = entry.get_field("PRIORITY").unwrap_or("6");
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
    let (emoji, emoji_style, msg_style) = match priority {
        "0" | "1" | "2" | "3" => (
            // Emerg, Alert, Crit, Err
            "❌",
            Style::default().fg(Color::LightRed),
            Style::default().fg(Color::Red), // Error = Red
        ),
        "4" => (
            // Warning
            "⚠️ ",
            Style::default().fg(Color::Yellow),
            Style::default().fg(Color::Rgb(255, 165, 0)), // Warning = Orange
        ),
        "5" | "6" => (
            // Notice, Info
            "ℹ️ ",
            Style::default().fg(Color::Green),
            Style::default().fg(Color::Cyan), // Info = Cyan
        ),
        "7" => (
            // Debug
            "🐞",
            Style::default().fg(Color::DarkGray),
            Style::default().fg(Color::Yellow), // Debug = Yellow
        ),
        _ => (
            // Unknown
            "❓",
            Style::default().fg(Color::Gray),
            Style::default().fg(Color::White), // Unknown = White
        ),
    };

    // Middle Truncation Logic (e.g., "service-name" -> "ser..ame")
    let unit_disp = if unit_raw.len() > 10 {
        format!("{}..{} ", &unit_raw[..4], &unit_raw[unit_raw.len() - 4..])
    } else {
        format!("{: <11}", unit_raw)
    };

    // 3. Construct the Styled Line using Ratatui Spans
    let line = Line::from(vec![
        // Insert the Emoji at the very beginning
        Span::styled(format!("{} ", emoji), emoji_style),
        // Timestamp in Purple
        Span::styled(ts_str, Style::default().fg(Color::Indexed(170))),
        // Offset in Blue/Cyan
        Span::styled(offset_str, Style::default().fg(Color::Indexed(39))),
        // Spacing
        Span::raw("  "),
        // Unit and [PID] in Green
        Span::styled(
            format!("{}[{}]", unit_disp, pid),
            Style::default().fg(Color::Indexed(76)),
        ),
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

fn parse_human_time(s: &str) -> i64 {
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
