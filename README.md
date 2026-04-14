# ghostty-bridge

A CLI that bridges AI agents to Ghostty terminal panes via AppleScript. Send text, read output, and coordinate across multiple terminal sessions programmatically.

## Requirements

- macOS (uses AppleScript for Ghostty automation)
- [Ghostty](https://ghostty.org) 1.3.0+ with AppleScript support

## Install

```sh
cargo install ghostty-bridge
```

## Commands

| Command | Description |
|---------|-------------|
| `ghostty-bridge list` | Show all terminals (id, name, cwd, label) |
| `ghostty-bridge type <target> <text>` | Type text into a terminal without pressing Enter |
| `ghostty-bridge message <target> <text>` | Type text with sender info and press Enter |
| `ghostty-bridge read <target> [lines]` | Read last N lines from terminal output (default: 50) |
| `ghostty-bridge keys <target> <key>...` | Send special keys (Enter, Escape, C-c, etc.) |
| `ghostty-bridge name <target> <label>` | Label a terminal for easy reference |
| `ghostty-bridge resolve <label>` | Resolve a label to a terminal UUID |
| `ghostty-bridge id` | Print this terminal's Ghostty ID |
| `ghostty-bridge doctor` | Diagnose Ghostty connectivity |

## Usage

### Identify terminals

```sh
# List all Ghostty terminals
ghostty-bridge list

# Find the current terminal's ID
ghostty-bridge id
```

### Label terminals for easy reference

```sh
# Label a terminal by its UUID
ghostty-bridge name B7B29D1F-3720-48AC-ADA7-D507B260E1F0 build-server

# Then use the label instead of the UUID
ghostty-bridge read build-server
ghostty-bridge type build-server "cargo test"
```

### Send commands and read output

```sh
# Type a command (no Enter)
ghostty-bridge type build-server "cargo build"

# Send Enter to run it
ghostty-bridge keys build-server Enter

# Or do both at once with message
ghostty-bridge message build-server "cargo build"

# Read the output
ghostty-bridge read build-server 20
```

### Special keys

Keys supports names like `Enter`, `Escape`, `Tab`, `Space`, `Up`, `Down`, `Left`, `Right`, `Home`, `End`, `PageUp`, `PageDown`, `Backspace`, `Delete`, and modifier combos like `C-c`, `M-a`.

```sh
ghostty-bridge keys build-server C-c
ghostty-bridge keys build-server Escape Enter
```

## How it works

ghostty-bridge uses Ghostty's AppleScript API to control windows, tabs, and terminals. It pipes AppleScript to `osascript` via stdin and parses the structured output.

Terminal labels are stored in `~/Library/Application Support/ghostty-bridge/labels.json` and are not tied to Ghostty sessions.

The `read` command works by using `select_all` + `copy_to_clipboard` via AppleScript, reading the clipboard via `pbpaste`, then restoring the original clipboard. There is a brief window (~100ms) where the clipboard is in use.

## Diagnostics

Run `ghostty-bridge doctor` to verify that Ghostty is running, check the version, count terminals, and test terminal identification.

## License

MIT
