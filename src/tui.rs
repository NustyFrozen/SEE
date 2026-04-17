use crate::reader_instance::{self};
use crate::{tui_input, tui_input_date};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use journald::JournalEntry;
use journald::reader::{JournalReader, JournalReaderConfig};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, LineGauge, List, ListItem, ListState, Padding};
use ratatui::widgets::{Clear, Paragraph};
use std::sync::atomic::Ordering;
use tui_slider::{Slider, SliderOrientation};

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
    reader_instance: reader_instance::ReaderInstance,
    filter: tui_input::TuiInput,
    from: tui_input_date::TuiInputDate,
    to: tui_input_date::TuiInputDate,
    pub inputstate: InputMode,
    pub oldinputstate: InputMode,
    lstate: ListState,
    tstate: ListState,
    temp_cursor_log: JournalEntry,
    cursor_map: Vec<String>,
    log_data: Vec<ListItem<'static>>,
    meta_motions: String, // meta kepress for motions
}
impl SEETui {
    pub const FOCUSED_COLOR: Color = Color::Rgb(121, 88, 221);
    pub const UNFOCUSED_COLOR: Color = Color::Rgb(91, 88, 91);
    pub fn refocus(&mut self) {
        self.reformwidgets();
    }
    pub fn dispose(&mut self) {
        self.reader_instance
            .is_cancelled
            .store(true, Ordering::SeqCst);
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

    pub fn new(unit: String) -> Self {
        Self {
            filter: tui_input::TuiInput::new(
                "📌Filter".to_string(),
                "Filter for example \"service (daemon) has not started \" or regex h*o w??rld"
                    .to_string(),
            ),
            lstate: ListState::default(),
            oldinputstate: InputMode::InputFilter,
            inputstate: InputMode::SelectLog,
            from: tui_input_date::TuiInputDate::new("📅from".to_string()),
            to: tui_input_date::TuiInputDate::new("📅to".to_string()),
            unit: unit.clone(),

            temp_cursor_log: JournalEntry::new(),
            tstate: ListState::default(),
            clipboard: arboard::Clipboard::new().ok(),
            reader_instance: reader_instance::ReaderInstance::new(
                unit,
                "".to_string(),
                "".to_string(),
                "".to_string(),
            ),
            cursor_map: vec![],
            log_data: vec![],
            meta_motions: String::new(),
        }
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

    fn yank(&mut self) {
        if let Some(idx) = self.tstate.selected() {
            if let Some((key, value)) = self.temp_cursor_log.fields.iter().nth(idx) {
                let text = format!("{}={}", key, value);
                self.to_clipboard(text);
            }
        }
    }
    fn pull_new_journaldata(&mut self) -> bool {
        if let Ok(mut cursor_guard) = self.reader_instance.cursor_map.try_lock()
            && let Ok(mut logs_guard) = self.reader_instance.log_data.try_lock()
        {
            if logs_guard.is_empty() {
                return false;
            }

            self.cursor_map.append(&mut *cursor_guard);
            self.log_data.append(&mut *logs_guard);

            return true;
        }

        false
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
        let mut keep_meta = false;
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
                (_, KeyCode::Char(c @ '0'..='9'), false) => {
                    keep_meta = true;
                    self.meta_motions.push(c);
                }
                (InputMode::DetailedEntry, KeyCode::Char('j') | KeyCode::Down, false) => {
                    self.tstate.select_next()
                }
                (InputMode::DetailedEntry, KeyCode::Char('G'), false) => self.tstate.select_last(),
                (InputMode::DetailedEntry, KeyCode::Char('g'), false) => self.tstate.select_first(),
                (InputMode::SelectLog, KeyCode::Enter, false) => self.select_log(),

                (InputMode::SelectLog, KeyCode::Char('g'), false) => self.lstate.select_first(),
                (InputMode::SelectLog, KeyCode::Char('G'), false) => {
                    if let Some(line_num) = self.meta_motions.parse::<usize>().ok() {
                        self.lstate.select((line_num - 1_usize).into());
                    } else {
                        self.lstate.select_last()
                    }
                }
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
            if !keep_meta {
                self.meta_motions.clear();
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
            self.reader_instance
                .is_cancelled
                .store(true, Ordering::SeqCst);
            self.cursor_map = vec![];
            self.log_data = vec![];
            self.reader_instance = reader_instance::ReaderInstance::new(
                self.unit.clone(),
                self.filter.input.clone(),
                self.from.input.clone(),
                self.to.input.clone(),
            );
        };
        self.render_footer(frame, main_chunks[0]);
        if self.inputstate == InputMode::DetailedEntry {
            self.render_detailed_entry(frame);
        }
    }
    fn render_footer(&mut self, frame: &mut Frame, area: Rect) {
        let pos = self.lstate.selected().unwrap_or(0) as f64 + 1.0;
        let len = self.log_data.len() as f64;

        let ratio = if len > 0.0 {
            (pos / len).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let slider = Slider::new(pos, 0_f64, len)
            .orientation(SliderOrientation::Horizontal)
            .filled_symbol("─")
            .filled_color(Color::Rgb(86, 117, 125))
            .empty_symbol("─")
            .handle_symbol("│")
            .show_value(false);
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
        frame.render_widget(slider, controls_chunks[0]);
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
        let selecte_data = self.pull_new_journaldata();
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
}
