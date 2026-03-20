use crate::tui_input;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListState};
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
}
impl SEETui {
    pub const FOCUSED_COLOR: Color = Color::Rgb(121, 88, 221);
    pub const UNFOCUSED_COLOR: Color = Color::Rgb(91, 88, 91);
    pub fn refocus(&mut self) {
        self.reformwidgets();
    }
    pub fn new(unit: String) -> Self {
        Self {
            filter: tui_input::TuiInput::new(
                "Search".to_string(),
                "Filter for example \"service (daemon) has not started \" or regex h*o w??rld"
                    .to_string(),
            ),

            oldinputstate: InputMode::InputFilter,
            inputstate: InputMode::SelectLog,
            from: tui_input::TuiInput::new("from".to_string(), "mm/dd/yyyy".to_string()),
            to: tui_input::TuiInput::new("To".to_string(), "mm/dd/yyyy".to_string()),
            autofetch: false,
            unit: unit,
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
    //returns false if went out of focus
    pub fn run_widget(&mut self, area: Rect, frame: &mut Frame, keye: Option<KeyEvent>) -> bool {
        //UI
        let mut stay_focus = true;
        let mut next_input = None;
        let mut logstate = ListState::default().with_selected(Some(0));
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

        self.render(area, frame, &mut logstate, next_input);
        stay_focus // STAY HARD  :P
    }
    /// Render the UI with a table.
    fn render(
        &mut self,
        area: Rect,
        frame: &mut Frame,
        logs_state: &mut ListState,
        key: Option<KeyEvent>,
    ) {
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

        self.render_logs(frame, main_chunks[0], logs_state);
        self.filter.render_input(controls_chunks[0], frame, key);
        self.from.render_input(controls_chunks[1], frame, key);
        self.to.render_input(controls_chunks[2], frame, key);
    }

    pub fn render_logs(&self, frame: &mut Frame, area: Rect, list_state: &mut ListState) {
        let items = ["Item 1", "Item 2", "Item 3", "Item 4"];
        let cool_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(if self.inputstate == InputMode::SelectLog {
                SEETui::FOCUSED_COLOR
            } else {
                SEETui::UNFOCUSED_COLOR
            });

        let list = List::new(items)
            .block(cool_block)
            .highlight_style(Modifier::REVERSED)
            .highlight_symbol("✓ ");
        frame.render_stateful_widget(list, area, list_state);
    }
}
