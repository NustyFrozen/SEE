use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use systemd::journal::{Journal, JournalSeek};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    // --- Core Human Readable Fields ---
    #[serde(rename = "MESSAGE")]
    pub message: String,

    #[serde(rename = "PRIORITY")]
    pub priority: LogLevel,

    // --- Identification Fields ---
    #[serde(rename = "_SYSTEMD_UNIT")]
    pub unit: Option<String>,

    #[serde(rename = "_PID")]
    pub pid: Option<String>,

    #[serde(rename = "_HOSTNAME")]
    pub hostname: Option<String>,

    // --- Timestamps ---
    #[serde(rename = "__REALTIME_TIMESTAMP")]
    pub timestamp_usec: String, // Microseconds since epoch

    // --- Technical/Location Fields ---
    #[serde(rename = "_EXE")]
    pub executable_path: Option<String>,

    #[serde(rename = "_COMM")]
    pub command: Option<String>,

    #[serde(rename = "__CURSOR")]
    pub cursor: String,

    // --- Catch-all for custom fields ---
    #[serde(flatten)]
    pub extra_fields: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(try_from = "String", into = "String")]
pub enum LogLevel {
    Emergency = 0,
    Alert = 1,
    Critical = 2,
    Error = 3,
    Warning = 4,
    Notice = 5,
    Info = 6,
    Debug = 7,
    Unknown = 8,
}

impl From<LogLevel> for String {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Emergency => "0".to_string(),
            LogLevel::Alert => "1".to_string(),
            LogLevel::Critical => "2".to_string(),
            LogLevel::Error => "3".to_string(),
            LogLevel::Warning => "4".to_string(),
            LogLevel::Notice => "5".to_string(),
            LogLevel::Info => "6".to_string(),
            LogLevel::Debug => "7".to_string(),
            LogLevel::Unknown => "8".to_string(),
        }
    }
}
// Convert from the string "3" systemd gives us to our Enum
impl From<String> for LogLevel {
    fn from(s: String) -> Self {
        match s.as_str() {
            "0" => LogLevel::Emergency,
            "1" => LogLevel::Alert,
            "2" => LogLevel::Critical,
            "3" => LogLevel::Error,
            "4" => LogLevel::Warning,
            "5" => LogLevel::Notice,
            "6" => LogLevel::Info,
            "7" => LogLevel::Debug,
            _ => LogLevel::Unknown,
        }
    }
}
pub fn fetch_journal_entries(
    filters: HashMap<&str, &str>,
) -> Result<Vec<JournalEntry>, Box<dyn std::error::Error>> {
    // 1. Open the journal for the system (false, false means system-wide logs)
    let mut journal = Journal::open(false, false)?;

    // 2. Apply Filters (Equivalent to journalctl -u or _PID=x)
    for (key, value) in filters {
        journal.add_match(key, value)?;
    }

    // 3. Seek to the beginning (or end depending on your needs)
    journal.seek(JournalSeek::Head)?;

    let mut entries = Vec::new();

    // 4. Iterate and Hydrate
    // Note: next_entry() moves the cursor and returns the data for that entry
    while let Ok(Some(raw_entry)) = journal.next_entry() {
        let entry = JournalEntry {
            message: raw_entry.get("MESSAGE").unwrap_or_default(),
            priority: LogLevel::from(raw_entry.get("PRIORITY").unwrap_or_else(|| "6".into())),
            unit: raw_entry.get("_SYSTEMD_UNIT"),
            pid: raw_entry.get("_PID"),
            hostname: raw_entry.get("_HOSTNAME"),
            timestamp_usec: raw_entry.get("__REALTIME_TIMESTAMP").unwrap_or_default(),
            executable_path: raw_entry.get("_EXE"),
            command: raw_entry.get("_COMM"),
            cursor: raw_entry.get("__CURSOR").unwrap_or_default(),
            extra_fields: HashMap::new(),
        };
        entries.push(entry);
    }

    // 5. Sort by Timestamp
    // systemd timestamps are strings of microseconds, so we parse them to u64 for sorting
    entries.sort_by(|a, b| {
        let time_a: u64 = a.timestamp_usec.parse().unwrap_or(0);
        let time_b: u64 = b.timestamp_usec.parse().unwrap_or(0);
        time_a.cmp(&time_b)
    });

    Ok(entries)
}
