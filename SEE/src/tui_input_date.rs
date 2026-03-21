use chrono::{Local, NaiveDateTime, TimeZone};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    Frame,
    layout::{Alignment, Position, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
};

use crate::tui::SEETui;
pub(crate) struct TuiInputDate {
    title: String,
    pub input: String,
    character_index: usize,
    pub focused: bool,
}
impl TuiInputDate {
    pub fn new(title: String) -> Self {
        Self {
            title: title,
            input: String::new(),
            character_index: 0,
            focused: false,
        }
    }

    fn move_cursor_left(&mut self) {
        let cursor_moved_left = self.character_index.saturating_sub(1);
        self.character_index = self.clamp_cursor(cursor_moved_left);
    }

    fn move_cursor_right(&mut self) {
        let cursor_moved_right = self.character_index.saturating_add(1);
        self.character_index = self.clamp_cursor(cursor_moved_right);
    }

    fn enter_char(&mut self, new_char: char) {
        let index = self.byte_index();

        // 1. Total length is now 19 characters for yyyy
        if index >= 19 {
            return;
        }

        // 2. Updated Validation Logic
        let is_valid = match index {
            // MM (Month) 00-12
            0 => "01".contains(new_char),
            1 => {
                let first = self.input.chars().nth(0).unwrap_or('0');
                if first == '1' {
                    "012".contains(new_char)
                } else {
                    true
                }
            }
            // DD (Day) 00-31
            3 => "0123".contains(new_char),
            4 => {
                let first = self.input.chars().nth(3).unwrap_or('0');
                if first == '3' {
                    "01".contains(new_char)
                } else {
                    true
                }
            }
            // YYYY - Allow any digits for 6, 7, 8, 9
            6..=9 => true,
            // HH (Hour) - Now starts at index 11
            11 => "012".contains(new_char),
            12 => {
                let first = self.input.chars().nth(11).unwrap_or('0');
                if first == '2' {
                    "0123".contains(new_char)
                } else {
                    true
                }
            }
            // MM/SS (Minutes/Seconds) - Now at 14 and 17
            14 | 17 => "012345".contains(new_char),
            _ => true,
        };

        if !is_valid {
            return;
        }

        self.input.insert(index, new_char);
        self.move_cursor_right();

        // 4. Corrected Auto-Insert Positions
        let next_index = self.byte_index();
        match next_index {
            2 | 5 => self.auto_insert('/'),
            10 => self.auto_insert(' '),      // Space after YYYY
            13 | 16 => self.auto_insert(':'), // Time separators
            _ => {}
        }
    }
    fn auto_insert(&mut self, separator: char) {
        let i = self.byte_index();
        // Only insert if it's not already there (prevents double separators)
        if self.input.chars().nth(i) != Some(separator) {
            self.input.insert(i, separator);
            self.move_cursor_right();
        }
    }

    /// Returns the byte index based on the character position.
    ///
    /// Since each character in a string can contain multiple bytes, it's necessary to calculate
    /// the byte index based on the index of the character.
    fn byte_index(&self) -> usize {
        self.input
            .char_indices()
            .map(|(i, _)| i)
            .nth(self.character_index)
            .unwrap_or(self.input.len())
    }

    fn delete_char(&mut self) {
        let is_not_cursor_leftmost = self.character_index != 0;
        if is_not_cursor_leftmost {
            // Method "remove" is not used on the saved text for deleting the selected char.
            // Reason: Using remove on String works on bytes instead of the chars.
            // Using remove would require special care because of char boundaries.

            let current_index = self.character_index;
            let from_left_to_current_index = current_index - 1;

            // Getting all characters before the selected character.
            let before_char_to_delete = self.input.chars().take(from_left_to_current_index);
            // Getting all characters after selected character.
            let after_char_to_delete = self.input.chars().skip(current_index);

            // Put all characters together except the selected one.
            // By leaving the selected one out, it is forgotten and therefore deleted.
            self.input = before_char_to_delete.chain(after_char_to_delete).collect();
            self.move_cursor_left();
        }
    }

    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.input.chars().count())
    }

    const fn reset_cursor(&mut self) {
        self.character_index = 0;
    }

    fn submit_message(&mut self) {
        self.input.clear();
        self.reset_cursor();
    }

    pub fn render_input(&mut self, area: Rect, frame: &mut Frame, keye: Option<KeyEvent>) -> bool {
        self.render(area, frame);
        let mut key_pressed = false;
        if let Some(key) = keye
            && self.focused
        {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Enter => self.submit_message(),
                    KeyCode::Char(to_insert) if to_insert.is_ascii_digit() => {
                        self.enter_char(to_insert);
                        key_pressed = true;
                    }
                    KeyCode::Backspace => {
                        self.delete_char();

                        return self.input.is_empty();
                    }
                    _ => {}
                }
            }
        }

        if key_pressed {
            let ts = Self::parse_human_time(&self.input);
            return ts != 0;
        }
        false
    }
    pub fn parse_human_time(s: &str) -> i64 {
        // 1. Instant exit for wrong length
        if s.len() != 19 {
            return 0;
        }

        // 2. Fast byte-check for separators (much faster than regex or full parse)
        // mm/dd/yyyy hh:mm:ss
        // 0123456789012345678
        let bytes = s.as_bytes();
        if bytes[2] != b'/'
            || bytes[5] != b'/'
            || bytes[10] != b' '
            || bytes[13] != b':'
            || bytes[16] != b':'
        {
            return 0;
        }

        // 3. Only now do the heavy lifting
        let fmt = "%m/%d/%Y %H:%M:%S";
        NaiveDateTime::parse_from_str(s, fmt)
            .ok()
            .and_then(|naive| Local.from_local_datetime(&naive).single())
            .map(|dt| dt.timestamp_micros())
            .unwrap_or(0)
    }
    fn render(&self, input_area: Rect, frame: &mut Frame) {
        let cool_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(if self.focused {
                SEETui::FOCUSED_COLOR
            } else {
                SEETui::UNFOCUSED_COLOR
            })
            .title(Line::from(vec![Span::styled(
                &self.title,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )]))
            .title_alignment(Alignment::Left);
        let is_empty = str::is_empty(self.input.as_str());
        let input = Paragraph::new(if is_empty {
            "mm/dd/yyyy hh:mm:ss"
        } else {
            self.input.as_str()
        })
        .block(cool_block)
        .fg(if is_empty { Color::Gray } else { Color::White });
        frame.render_widget(input, input_area);
        if self.focused {
            #[expect(clippy::cast_possible_truncation)]
            frame.set_cursor_position(Position::new(
                // Draw the cursor at the current position in the input field.
                // This position can be controlled via the left and right arrow key
                input_area.x + self.character_index as u16 + 1,
                // Move one line down, from the border to the input line
                input_area.y + 1,
            ));
        }
    }
}
