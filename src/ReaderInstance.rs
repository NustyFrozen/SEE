pub struct reander_instance {
    pub reader: JournalReader,
}
pub(crate) impl reander_instance {
    pub fn new(value: u64, name: String) -> Self {
        Self { value, name }
    }
    fn fetch_log_data(&mut self) -> (Vec<String>, Vec<ListItem<'static>>) {
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
}
