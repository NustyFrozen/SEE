use color_eyre::Result;
use crossterm::event::{self, KeyCode};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, List, ListState, Paragraph, Row, Table, TableState,
};
use tokio::process::Command;
use tokio::{self, io};

#[derive(PartialEq)]
enum InputMode {
    InputFilter,
    InputFrom,
    InputTo,
    SelectService,
    SelectLog,
}
pub(crate) struct SEETui {
    services: Vec<String>,
    filter: String,
    from: String,
    to: String,
    autofetch: bool,
    service: String,
    character_index: usize,
    inputstate: InputMode,
}
impl SEETui {
    const FOCUSED_COLOR: Color = Color::Rgb(121, 88, 221);
    const UNFOCUSED_COLOR: Color = Color::Rgb(91, 88, 91);
    pub const fn new() -> Self {
        Self {
            filter: String::new(),
            inputstate: InputMode::SelectLog,
            service: String::new(),
            character_index: 0,
            from: String::new(),
            to: String::new(),
            autofetch: false,
            services: vec![],
        }
    }
    async fn fetch_services() -> io::Result<Vec<String>> {
        let output = Command::new("journalctl")
            .args(["-F", "_SYSTEMD_UNIT"])
            .output()
            .await?; // The '?' returns Error if journalctl fails

        let list = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect();

        Ok(list)
    }
    pub fn run(&mut self) -> Result<()> {
        color_eyre::install()?;
        //ActiveServiceFetcher
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);

        // Background Task
        tokio::spawn(async move {
            loop {
                if let Ok(new_items) = SEETui::fetch_services().await {
                    let _ = tx.send(new_items).await;
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        });

        //UI
        let mut logstate = ListState::default().with_selected(Some(0));
        let mut servicestate = ListState::default().with_selected(Some(0));
        ratatui::run(|terminal| {
            loop {
                if let Ok(new_services) = rx.try_recv() {
                    self.services = new_services; // Update the local state
                }
                terminal.draw(|frame| self.render(frame, &mut servicestate, &mut logstate))?;

                if let Some(key) = event::read()?.as_key_press_event() {
                    //global key options
                    match key.code {
                        KeyCode::Esc => {
                            if (self.inputstate == InputMode::InputFilter) {
                                self.inputstate = InputMode::SelectLog;
                            } else {
                                return Ok(());
                            }
                        }
                        KeyCode::Char('i') if self.inputstate != InputMode::InputFilter => {
                            self.inputstate = InputMode::InputFilter;
                        }
                        _ => {}
                    }
                    //per menu input
                    match self.inputstate {
                        InputMode::InputFilter | InputMode::InputFrom | InputMode::InputTo => {
                            match key.code {
                                KeyCode::Char(to_insert) => self.enter_char(to_insert),
                                KeyCode::Backspace => self.delete_char(),
                                KeyCode::Left => self.move_cursor_left(),
                                KeyCode::Right => self.move_cursor_right(),
                                _ => {}
                            }
                        }
                        InputMode::SelectService => match key.code {
                            KeyCode::Char('j') | KeyCode::Down => servicestate.select_next(),
                            KeyCode::Char('k') | KeyCode::Up => servicestate.select_previous(),
                            _ => {}
                        },
                        InputMode::SelectLog => match key.code {
                            KeyCode::Char('j') | KeyCode::Down => logstate.select_next(),
                            KeyCode::Char('k') | KeyCode::Up => logstate.select_previous(),
                            _ => {}
                        },
                    }
                }
            }
        })
    }

    fn get_current_input_ref(&mut self) -> &mut String {
        match self.inputstate {
            InputMode::InputTo => &mut self.to,
            InputMode::InputFrom => &mut self.from,
            _ => &mut self.filter,
        }
    }
    fn delete_char(&mut self) {
        let is_not_cursor_leftmost = self.character_index != 0;
        if is_not_cursor_leftmost {
            let current_index = self.character_index;
            let from_left_to_current_index = current_index - 1;

            // 1. Get the iterator of characters we want to keep
            let new_content: String = {
                let input = self.get_current_input_ref();
                let before = input.chars().take(from_left_to_current_index);
                let after = input.chars().skip(current_index);
                before.chain(after).collect() // This creates a new String
            };

            // 2. Dereference the &mut String and replace its content
            *self.get_current_input_ref() = new_content.to_string();

            self.move_cursor_left();
        }
    }
    const fn reset_cursor(&mut self) {
        self.character_index = 0;
    }
    fn move_cursor_left(&mut self) {
        let cursor_moved_left = self.character_index.saturating_sub(1);
        self.character_index = self.clamp_cursor(cursor_moved_left);
    }

    fn move_cursor_right(&mut self) {
        let cursor_moved_right = self.character_index.saturating_add(1);
        self.character_index = self.clamp_cursor(cursor_moved_right);
    }
    fn clamp_cursor(&mut self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.get_current_input_ref().chars().count())
    }
    fn enter_char(&mut self, new_char: char) {
        let index = self.byte_index();

        let input = self.get_current_input_ref();

        input.insert(index, new_char);

        // Now move the cursor
        self.move_cursor_right();
    }
    fn byte_index(&mut self) -> usize {
        // 1. Copy the index out first to release 'self'
        let char_idx = self.character_index;

        // 2. Now borrow the string
        self.get_current_input_ref()
            .char_indices()
            .nth(char_idx) // Use the local copy
            .map(|(i, _)| i)
            .unwrap_or_else(|| self.get_current_input_ref().len())
    }
    /// Render the UI with a table.
    fn render(&self, frame: &mut Frame, service_state: &mut ListState, logs_state: &mut ListState) {
        // --- STEP 1: Main Vertical Stack ---
        let root_block = Block::default().style(Style::default().bg(Color::Rgb(20, 20, 25))); // Deep dark blue background
        frame.render_widget(root_block, frame.size());
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Header text
                Constraint::Min(3),    // Main Body (List + Table)
                Constraint::Length(3), // Search / Dates Row
                Constraint::Length(1), // Auto-fetch checkbox
                Constraint::Length(1), // Application metadata footer
            ])
            .split(frame.size());

        // --- STEP 2: Body (Horizontal Split) ---
        let body_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(20), // Services List
                Constraint::Percentage(80), // Logs Table (and its headers)
            ])
            .split(main_chunks[1]);

        // Split the right side of the body for Headers vs Table
        let logs_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0), // Table content
            ])
            .split(body_chunks[1]);

        // --- STEP 3: Search Controls Row (Horizontal Split) ---
        let controls_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(70), // User free search / regex
                Constraint::Percentage(15), // From date
                Constraint::Percentage(15), // To date
            ])
            .split(main_chunks[2]);

        // --- STEP 4: Render Placeholders (To verify layout) ---
        frame.render_widget(Paragraph::new("Selected Service Info"), main_chunks[0]);
        frame.render_widget(
            Block::default().borders(Borders::ALL).title("From"),
            controls_chunks[1],
        );
        frame.render_widget(
            Block::default().borders(Borders::ALL).title("To"),
            controls_chunks[2],
        );

        // Checkbox & Footer
        frame.render_widget(Paragraph::new("[x] auto fetch"), main_chunks[3]);
        frame.render_widget(
            Paragraph::new("App Metadata...").on_white().black(),
            main_chunks[4],
        );
        self.render_services(frame, body_chunks[0], service_state);
        self.render_logs(frame, logs_chunks[0], logs_state);
        self.render_filter_input(frame, controls_chunks[0]);
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
            })
            .title(Line::from(vec![Span::styled(
                "LOGS",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )]))
            .title_alignment(Alignment::Center);

        let list = List::new(items)
            .block(cool_block)
            .highlight_style(Modifier::REVERSED)
            .highlight_symbol("✓ ");
        frame.render_stateful_widget(list, area, list_state);
    }
    pub fn render_services(&self, frame: &mut Frame, area: Rect, list_state: &mut ListState) {
        let items = self.services.clone();
        let cool_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(if self.inputstate == InputMode::SelectService {
                SEETui::FOCUSED_COLOR
            } else {
                SEETui::UNFOCUSED_COLOR
            })
            .title(Line::from(vec![Span::styled(
                "SERVICES",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )]))
            .title_alignment(Alignment::Center);

        let list = List::new(items)
            .block(cool_block)
            .highlight_style(Modifier::REVERSED)
            .highlight_symbol("✓ ");
        frame.render_stateful_widget(list, area, list_state);
    }
    pub fn render_filter_input(&self, frame: &mut Frame, area: Rect) {
        let cool_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(if self.inputstate == InputMode::InputFilter {
                SEETui::FOCUSED_COLOR
            } else {
                SEETui::UNFOCUSED_COLOR
            })
            .title(Line::from(vec![Span::styled(
                "FILTER",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )]))
            .title_alignment(Alignment::Left);
        let is_empty = str::is_empty(self.filter.as_str());
        match self.inputstate {
            // Make the cursor visible and ask ratatui to put it at the specified coordinates after
            // rendering
            #[expect(clippy::cast_possible_truncation)]
            InputMode::InputFilter => frame.set_cursor_position(Position::new(
                // Draw the cursor at the current position in the input field.
                // This position can be controlled via the left and right arrow key
                area.x + self.character_index as u16 + 1,
                // Move one line down, from the border to the input line
                area.y + 1,
            )),
            // Hide the cursor. `Frame` does this by default, so we don't need to do anything here
            _ => {}
        }
        let input = Paragraph::new(if is_empty {
            "Filter for example \"The Docker background service (daemon)\" or regex h*o w??rld"
        } else {
            self.filter.as_str()
        })
        .block(cool_block)
        .fg(if is_empty { Color::Gray } else { Color::White });
        frame.render_widget(input, area);
    }
}
