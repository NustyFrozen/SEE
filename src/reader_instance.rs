use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use chrono::{Local, NaiveDateTime, TimeZone};
use journald::reader::{JournalReader, JournalReaderConfig};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
    widgets::ListItem,
};
use regex::Regex;
use tokio::task;
struct SendReader(JournalReader);
unsafe impl Send for SendReader {}

pub struct ReaderInstance {
    pub cursor_map: Arc<tokio::sync::Mutex<Vec<String>>>,
    pub log_data: Arc<tokio::sync::Mutex<Vec<ListItem<'static>>>>,
    pub is_cancelled: Arc<AtomicBool>,
}

impl ReaderInstance {
    pub fn new(unit: String, filter: String, from: String, to: String) -> Self {
        let is_cancelled = Arc::new(AtomicBool::new(false));
        let cursor_map = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let log_data = Arc::new(tokio::sync::Mutex::new(Vec::new()));

        let is_cancelled_task = Arc::clone(&is_cancelled);
        let cursor_map_task = Arc::clone(&cursor_map);
        let log_data_task = Arc::clone(&log_data);

        let from_ts = parse_human_time(&from);
        let to_ts = if to.is_empty() {
            i64::MAX
        } else {
            parse_human_time(&to)
        };
        let re = if !filter.is_empty() {
            Regex::new(&filter).ok()
        } else {
            None
        };
        let filter_owned = filter.clone();

        tokio::spawn(async move {
            let reader_raw = match JournalReader::open(&JournalReaderConfig::default()) {
                Ok(mut r) => {
                    let _ = r.add_filter(format!("_SYSTEMD_UNIT={}", unit).as_str());
                    r
                }
                Err(_) => return,
            };

            // 2. Wrap the reader in our Send-safe wrapper and a Mutex
            let reader = Arc::new(std::sync::Mutex::new(SendReader(reader_raw)));
            let mut current_pid = String::new();

            loop {
                if is_cancelled_task.load(Ordering::SeqCst) {
                    break;
                }

                let reader_cloned = Arc::clone(&reader);
                let re_cloned = re.clone();
                let filter_val = filter_owned.clone();
                let pid_val = current_pid.clone();

                let result = task::spawn_blocking(move || {
                    let mut batch_cursors = Vec::new();
                    let mut batch_items = Vec::new();
                    let mut loop_pid = pid_val;

                    let mut guard = reader_cloned.lock().unwrap();
                    let reader = &mut guard.0; // Access the JournalReader inside the wrapper

                    while let Ok(Some(entry)) = reader.next_entry() {
                        let wallclock = entry
                            .get_wallclock_time()
                            .map(|ts| ts.timestamp_us)
                            .unwrap_or(0);

                        if wallclock < from_ts {
                            continue;
                        }
                        if wallclock > to_ts {
                            break;
                        }

                        let message = entry.get_field("MESSAGE").unwrap_or_default();

                        if let Some(ref regex) = re_cloned {
                            if !regex.is_match(&message) {
                                continue;
                            }
                        } else if !filter_val.is_empty() {
                            if !message.contains(&filter_val) {
                                continue;
                            }
                        }

                        if let Some(newpid) = entry.get_field("_PID") {
                            if newpid != loop_pid {
                                batch_cursors.push(String::new());
                                batch_items.push(format_styled_line(&entry, -1, newpid));
                                loop_pid = newpid.to_string();
                            }
                        }

                        let curs = entry.get_field("__CURSOR").unwrap_or_default();
                        batch_cursors.push(curs.to_string());
                        batch_items.push(format_styled_line(&entry, wallclock, &message));
                    }
                    (batch_cursors, batch_items, loop_pid)
                })
                .await;

                if let Ok((new_cursors, new_items, new_pid)) = result {
                    current_pid = new_pid;
                    if !new_items.is_empty() {
                        let mut c_lock = cursor_map_task.lock().await;
                        let mut l_lock = log_data_task.lock().await;
                        c_lock.extend(new_cursors);
                        l_lock.extend(new_items);
                    }
                }

                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        });

        Self {
            cursor_map,
            log_data,
            is_cancelled,
        }
    }
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
        let line = Line::from(vec![Span::styled(
            format!("Started New Instance ➝ {}({})", unit_raw, message),
            Style::default().fg(Color::Gray),
        )]);

        return ListItem::new(line.centered());
    }
    let display_message = if let Some(start_idx) = message.find("msg=\"") {
        let content_start = start_idx + 5; // Skip past the 'msg="'
        if let Some(end_offset) = message[content_start..].find('"') {
            &message[content_start..content_start + end_offset]
        } else {
            message
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
    let (level, msg_style) = match priority {
        "0" | "1" | "2" | "3" => (
            // Emerg, Alert, Crit, Err
            "ERR",
            Style::default().fg(Color::Red), // Error = Red
        ),
        "4" => (
            // Warning
            "WARN",
            Style::default().fg(Color::Rgb(255, 165, 0)), // Warning = Orange
        ),
        "5" | "6" => (
            // Notice, Info
            "INFO",
            Style::default().fg(Color::Cyan), // Info = Cyan
        ),
        "7" => (
            // Debug
            "DEBUG",
            Style::default().fg(Color::Yellow), // Debug = Yellow
        ),
        _ => (
            // Unknown
            "UKNOWN",
            Style::default().fg(Color::White), // Unknown = White
        ),
    };

    // 3. Construct the Styled Line using Ratatui Spans
    let line = Line::from(vec![
        Span::styled(format!("[{}] ", level), msg_style),
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
