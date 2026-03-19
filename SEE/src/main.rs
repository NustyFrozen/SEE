mod tui;
use journald::reader::{JournalReader, JournalReaderConfig};
use tui::SEETui;
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open the journal with a default configuration
    let mut gui = tui::SEETui::new();
    gui.run();
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

    Ok(())
}
