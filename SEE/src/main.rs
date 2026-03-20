mod tui;
mod tui_input;
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use color_eyre::Result;
use color_eyre::owo_colors::colors::White;
use crossterm::event::{self, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::enable_raw_mode;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Offset, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Masked, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Tabs};
use ratatui::{Frame, symbols};
use serde::ser;
use tokio::{io, process::Command};
use tui::{InputMode, SEETui};
static SERVICES_POST_PROCESSING: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
static SERVICES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
static BUFFERS: OnceLock<Mutex<Vec<SEETui>>> = OnceLock::new();
static SELECTED_BUFFER: AtomicUsize = AtomicUsize::new(0);
static INPUT_OWNER: Mutex<InputOwner> = Mutex::new(InputOwner::SERVICEList);
#[derive(PartialEq, Eq)]
enum InputOwner {
    BUFFERS,
    SERVICESearch,
    SERVICEList,
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
fn get_buffers() -> &'static Mutex<Vec<SEETui>> {
    BUFFERS.get_or_init(|| Mutex::new(vec![]))
}
fn get_services() -> &'static Mutex<Vec<String>> {
    SERVICES.get_or_init(|| Mutex::new(vec![]))
}
fn get_services_post_processing() -> &'static Mutex<Vec<String>> {
    SERVICES_POST_PROCESSING.get_or_init(|| Mutex::new(vec![]))
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open the journal with a default configuration

    color_eyre::install()?;
    //ActiveServiceFetcher
    let (tx, mut rx) = tokio::sync::mpsc::channel(10);

    // Background Task
    tokio::spawn(async move {
        loop {
            if let Ok(new_items) = fetch_services().await {
                let _ = tx.send(new_items).await;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
    let mut search_service = tui_input::TuiInput::new(
        "🔎︎Search Service".to_string(),
        "ex: Docker.service".to_string(),
    );
    let mut service_state = ListState::default().with_selected(Some(0));
    //UI
    let mut pass_key = None;
    enable_raw_mode()?;
    ratatui::run(|terminal| -> std::io::Result<()> {
        loop {
            //check buffers do not take focus
            terminal
                .draw(|frame| render(frame, &mut service_state, &mut search_service, pass_key))?;

            pass_key = None;
            if event::poll(Duration::from_millis(10))? {
                if let Some(key) = event::read()?.as_key_press_event() {
                    if key.code == KeyCode::Char('q')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        return Ok(());
                    }
                    if (key.modifiers.contains(KeyModifiers::ALT)) {
                        match key.code {
                            KeyCode::Char(c) if c.is_ascii_digit() => {
                                let mut pos = c.to_digit(10).unwrap() - 1;
                                let mut buffers = get_buffers().lock().unwrap();
                                if (pos >= 0 && (pos as usize) < buffers.len()) {
                                    SELECTED_BUFFER.store(pos as usize, Ordering::SeqCst);
                                }
                            }
                            _ => {}
                        }
                    } else {
                        let mut owner = INPUT_OWNER.lock().unwrap();
                        match *owner {
                            InputOwner::SERVICESearch => {
                                match (key.code, key.modifiers.contains(KeyModifiers::CONTROL)) {
                                    (KeyCode::Char('k') | KeyCode::Up, true) => {
                                        *owner = InputOwner::SERVICEList;
                                        search_service.focused = false;
                                    }
                                    (KeyCode::Char('l') | KeyCode::Down, true) => {
                                        *owner = InputOwner::BUFFERS;

                                        search_service.focused = false;
                                        let mut buffers = get_buffers().lock().unwrap();
                                        if let Some(buffer) =
                                            buffers.get_mut(SELECTED_BUFFER.load(Ordering::SeqCst))
                                        {
                                            let buffer: &mut SEETui = buffer;
                                            buffer.inputstate = InputMode::InputFilter;
                                            buffer.refocus();
                                        }
                                    }
                                    (KeyCode::Enter, false) => {
                                        let index: Option<usize> = service_state.selected();

                                        if let Some(mut i) = index {
                                            let services = SERVICES_POST_PROCESSING
                                                .get()
                                                .unwrap()
                                                .lock()
                                                .unwrap();

                                            let mut buffers = get_buffers().lock().unwrap();
                                            if let Some(matching_buffer) =
                                                buffers.iter().position(|b| b.unit == services[i])
                                            {
                                                buffers.remove(matching_buffer);
                                            } else {
                                                buffers.push(SEETui::new(services[i].clone()));
                                            }
                                        }
                                    }

                                    (_, _) => {}
                                }

                                pass_key = Some(key);
                            }

                            InputOwner::SERVICEList => {
                                match (key.code, key.modifiers.contains(KeyModifiers::CONTROL)) {
                                    (KeyCode::Char('j'), false) => service_state.select_next(),
                                    (KeyCode::Char('k'), false) => service_state.select_previous(),
                                    (KeyCode::Enter | KeyCode::Char('x'), false) => {
                                        let index: Option<usize> = service_state.selected();

                                        if let Some(mut i) = index {
                                            let services = SERVICES_POST_PROCESSING
                                                .get()
                                                .unwrap()
                                                .lock()
                                                .unwrap();

                                            let mut buffers = get_buffers().lock().unwrap();
                                            if let Some(matching_buffer) =
                                                buffers.iter().position(|b| b.unit == services[i])
                                            {
                                                buffers.remove(matching_buffer);
                                            } else {
                                                buffers.push(SEETui::new(services[i].clone()));
                                            }
                                        }
                                    }

                                    (KeyCode::Char('j'), true) => {
                                        *owner = InputOwner::SERVICESearch;
                                        search_service.focused = true;
                                    }
                                    (KeyCode::Char('l'), true) => {
                                        *owner = InputOwner::BUFFERS;

                                        search_service.focused = false;
                                        let mut buffers = get_buffers().lock().unwrap();
                                        if let Some(buffer) =
                                            buffers.get_mut(SELECTED_BUFFER.load(Ordering::SeqCst))
                                        {
                                            let buffer: &mut SEETui = buffer;
                                            buffer.inputstate = InputMode::SelectLog;
                                        }
                                    }
                                    (_, _) => {}
                                }
                            }
                            _ => {
                                pass_key = Some(key);
                            }
                        }
                    }
                }
            }
            //sync service list
            if let Ok(new_services) = rx.try_recv() {
                if let Some(mutex) = SERVICES.get() {
                    let mut services = mutex.lock().expect("Failed to lock SERVICES");
                    *services = new_services;
                }
            }
        }

        Ok(())
    })?;
    /*
        let mut reader = JournalReader::open(&JournalReaderConfig::default())?;
        reader.add_filter("PRIORITY=3");
        // Iterate over available entries
        while let Some(entry) = reader.next_entry()? {
            println!(
                ": p={} m={}",
                entry.get_field("PRIORITY").unwrap_or("unknown"),
                entry.get_field("MESSAGE").unwrap_or("unknown")
            );
            // You can also access other fields like "PRIORITY", "_SYSTEMD_UNIT", etc.
            // entry.get("PRIORITY"), entry.get("_SYSTEMD_UNIT")
        }
    */
    Ok(())
}
fn render(
    frame: &mut Frame,
    list_state: &mut ListState,
    filter_input: &mut tui_input::TuiInput,
    nextkey: Option<KeyEvent>,
) {
    // --- STEP 1: Main Vertical Stack ---
    let root_block = Block::default().style(Style::default().bg(Color::Rgb(20, 20, 25))); // Deep dark blue background
    frame.render_widget(root_block, frame.size());
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Header text
            Constraint::Min(3),    // Main Body (List + Table)
            Constraint::Length(2), // Application metadata footer
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
    let servicechunk = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),         // Table content
            Constraint::Percentage(10), //search bar
        ])
        .split(body_chunks[0]);

    render_services(
        frame,
        &mut filter_input.input.to_string(),
        servicechunk[0],
        list_state,
    );
    render_info_paragraph(frame, main_chunks[2], nextkey);
    render_buffer_tabs(frame, body_chunks[1], nextkey);
    if (filter_input.render_input(servicechunk[1], frame, nextkey)) {
        list_state.select(Some(0 as usize));
    }
}
fn render_info_paragraph(frame: &mut Frame, area: Rect, nextkey: Option<KeyEvent>) {
    let short_line = "Slice, layer, and bake the vegetables. ";
    let long_line = short_line.repeat((area.width as usize) / short_line.len() + 2);
    let parag = vec![
        Line::from_iter([
            "Move between Widgets".bold().light_yellow(),
            " Cntrl + h/← | j/↑ | k/⬇| l/→".italic().gray().slow_blink(),
        ]),
        Line::from_iter([
            "Switch Between open Logs".bold().light_cyan(),
            " ALT + 1..9".italic().gray().slow_blink(),
        ]),
    ];
    frame.render_widget(parag[0].clone().left_aligned(), area + Offset::new(1, 0));
    frame.render_widget(parag[1].clone().right_aligned(), area + Offset::new(-1, 0));
}
fn render_buffer_tabs(frame: &mut Frame, area: Rect, nextkey: Option<KeyEvent>) {
    let mut buffers = get_buffers().lock().unwrap();
    if let Some(buffer) = buffers.get_mut(SELECTED_BUFFER.load(Ordering::SeqCst)) {
        let buffer: &mut SEETui = buffer;
        let mut owner = INPUT_OWNER.lock().unwrap();
        if !buffer.run_widget(area + Offset::new(1, 0), frame, nextkey)
            && *owner == InputOwner::BUFFERS
        {
            if (buffer.oldinputstate == InputMode::InputFrom) {
                *owner = InputOwner::SERVICESearch;
            } else {
                *owner = InputOwner::SERVICEList;
            }
        }
        let tab_titles: Vec<String> = buffers
            .iter()
            .map(|b| format!("📝{}", b.unit.clone()))
            .collect();
        let tabs = Tabs::new(tab_titles)
            .highlight_style(Style::default().fg(SEETui::FOCUSED_COLOR).bold())
            .select(SELECTED_BUFFER.load(Ordering::SeqCst))
            .divider("|")
            .padding(" ", " ");
        frame.render_widget(tabs, area);
    }
}
fn render_services(frame: &mut Frame, filter: &mut String, area: Rect, list_state: &mut ListState) {
    let mut items: Vec<ListItem> = vec![];

    let mut items_for_key_processing: Vec<String> = vec![];
    // 1. Lock both at the start
    let buffers = get_buffers().lock().unwrap();
    let services = get_services().lock().unwrap();

    let existing_units: HashSet<&str> = buffers.iter().map(|b| b.unit.as_str()).collect();

    // 3. Iterate through all services
    for item in services.iter() {
        // Check the set using .as_str()
        let is_focused = existing_units.contains(item.as_str());
        if (!filter.is_empty()) {
            if (!item.to_lowercase().contains(&filter.to_lowercase())) {
                continue;
            }
        }
        let style = if is_focused {
            Style::default().fg(Color::LightYellow).bold()
        } else {
            Style::default().fg(SEETui::UNFOCUSED_COLOR).bold()
        };
        items_for_key_processing.push(item.clone());
        items.push(ListItem::new(item.clone()).style(style));
    }
    *get_services_post_processing().lock().unwrap() = items_for_key_processing;
    let cool_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Color::LightYellow)
        .title(Line::from(vec![Span::styled(
            "Services🌐",
            Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD),
        )]))
        .title_alignment(Alignment::Left);

    let list = List::new(items)
        .block(cool_block)
        .highlight_style(Modifier::REVERSED)
        .highlight_symbol("✓ ");
    frame.render_stateful_widget(list, area, list_state);
}
