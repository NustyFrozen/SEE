## SEE (Service Event Extraction)
#### Linux Services Log Viewer (like journalctl) TUI with VI alike key movements that allows you to open multiple instances of service logs (like nvim buffers) with custom filtering
![Demo of SEE Viewer](media/demo.gif)
### Installation
`cargo install seetui`


#### Usage
run
`SEE`
or
`SEE -u service1 service2....`
#### Keybinds
- Ctrl + q Exit application
- Alt + [1-9]: Switch between open logs (buffers)
- Ctrl + h / l / j / k: Move between widgets (like in nvim)
- j / k Move up or down | g / G: Jump to top or bottom within widget
- Enter / x Toggle / open
- I input search, F input from date, T input to date
y: Yank selected key=value to clipboard (only in detailed view)
