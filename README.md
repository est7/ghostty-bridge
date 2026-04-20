# ghostty-bridge

A CLI that bridges AI agents to Ghostty terminal panes via AppleScript. Send text, read output, create surfaces, and coordinate across multiple terminal sessions programmatically.

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
| `ghostty-bridge list [--json]` | Show all terminals (id, name, cwd, label). `--json` emits a stable array for scripts |
| `ghostty-bridge type <target> <text>` | Type text into a terminal without pressing Enter |
| `ghostty-bridge message <target> <text>` | Type text with sender framing and press Enter |
| `ghostty-bridge reply <text>` | Read the current terminal, find the last bridge message, and message the sender back |
| `ghostty-bridge read <target> [lines] [--since-last-message]` | Read terminal output; `--since-last-message` returns only output after the last bridge-framed line |
| `ghostty-bridge keys <target> <key>...` | Send special keys (Enter, Escape, C-c, etc.) |
| `ghostty-bridge exec <target> <command>` | Type a command into a terminal and press Enter |
| `ghostty-bridge broadcast --target <target>... --text <text>` | Send the same command to multiple terminals |
| `ghostty-bridge broadcast --target <target>... --keys <key>...` | Send the same key sequence to multiple terminals |
| `ghostty-bridge open <window\|tab\|split> [...]` | Create a new Ghostty window, tab, or split pane |
| `ghostty-bridge layout validate <file>` | Validate a TOML layout template |
| `ghostty-bridge layout apply <file>` | Open and populate a full Ghostty layout from TOML |
| `ghostty-bridge focus <target>` | Focus a terminal or active Ghostty context |
| `ghostty-bridge close <target>` | Close a terminal, selected tab, or front window |
| `ghostty-bridge name <target> <label>` | Label a terminal for easy reference |
| `ghostty-bridge resolve <label>` | Resolve a label to a terminal UUID |
| `ghostty-bridge id` | Print this terminal's Ghostty ID |
| `ghostty-bridge doctor` | Diagnose Ghostty connectivity |

## Targets

Most commands accept a terminal UUID or a previously assigned label.

Built-in selectors are also supported:

- `focused` — the focused terminal of the selected tab in the front window
- `selected-tab` — all terminals in the selected tab of the front window
- `front-window` — all terminals in the front Ghostty window

`selected-tab` and `front-window` expand to multiple terminals, so they are mainly useful with `broadcast` and `close`.

## Usage

### Identify terminals

```sh
# List all Ghostty terminals
ghostty-bridge list

# Or get the same data in a stable JSON shape for scripts
ghostty-bridge list --json

# Find the current terminal's ID
ghostty-bridge id
```

### Label terminals for easy reference

```sh
# Label a terminal by its UUID
ghostty-bridge name B7B29D1F-3720-48AC-ADA7-D507B260E1F0 build-server

# Then use the label instead of the UUID
ghostty-bridge read build-server
ghostty-bridge type build-server "cargo build"
```

### Send commands and read output

```sh
# Type a command (no Enter)
ghostty-bridge type build-server "cargo build"

# Send Enter to run it
ghostty-bridge keys build-server Enter

# Or do both at once with exec
ghostty-bridge exec build-server "cargo build"

# Or include sender framing for agent-to-agent messages
ghostty-bridge message build-server "cargo build"

# Read the output
ghostty-bridge read build-server 20
```

### Agent-to-agent messaging

```sh
# Send a framed message to another labeled terminal
ghostty-bridge message codex "Please review src/auth.ts"

# Reply from the current terminal to the latest bridge message in scrollback
ghostty-bridge reply "I checked it; auth.ts:142 needs a nil guard"
```

`message` and `reply` emit lines like:

```text
[ghostty-bridge:CLAUDE-UUID >>> codex] Please review src/auth.ts
```

`reply` reads the current terminal, finds the most recent bridge-framed line, resolves the sender through the existing label-or-UUID pipeline, and sends the response there.

### Read only the output after the last agent message

```sh
# Read the visible transcript after the most recent bridge message
ghostty-bridge read codex 80 --since-last-message
```

This is still based on visible-screen capture, not exact Ghostty buffer boundaries. If no bridge-framed line is present in the captured output, the full read result is returned unchanged.

### Create work surfaces

```sh
# Open a new tab in the front window and run a command
ghostty-bridge open tab --cwd ~/src/project --command "cargo test" --label test

# Split the focused terminal and seed the new pane
ghostty-bridge open split --direction right --cwd ~/src/project --input "nvim ." --label editor

# Open a fresh window with environment variables
ghostty-bridge open window --cwd ~/src/project --env RUST_LOG=debug --env APP_ENV=dev
```

`open` returns the created terminal UUID on stdout so scripts can chain follow-up commands.

### Broadcast to active Ghostty contexts

```sh
# Run the same command in every terminal in the selected tab
ghostty-bridge broadcast --target selected-tab --text "git status -sb"

# Send Ctrl-C to every terminal in the front window
ghostty-bridge broadcast --target front-window --keys C-c
```

### Layout templates

Layout templates let you declare a full pane tree in TOML and apply it in one command.

```sh
# Check that a layout file is structurally valid
ghostty-bridge layout validate layouts/ai-trio.toml

# Open a new window and build the declared layout
ghostty-bridge layout apply layouts/ai-trio.toml
```

Example `layouts/ai-trio.toml`:

```toml
name = "ai-trio"

[root]
type = "split"
direction = "right"

[root.left]
type = "pane"
label = "claude"
cwd = "~/Est7Projects"
command = "claude"

[root.right]
type = "split"
direction = "down"

[root.right.top]
type = "pane"
label = "codex"
cwd = "~/Est7Projects"
command = "codex"

[root.right.bottom]
type = "pane"
label = "gemini"
cwd = "~/Est7Projects"
command = "gemini"
focus = true
```

Each `pane` can set `label`, `cwd`, `command`, `input`, `env = ["KEY=VALUE"]`, and `focus = true`. A layout may mark at most one pane with `focus = true`.

### Focus and close

```sh
# Jump back to a labeled pane
ghostty-bridge focus editor

# Close the currently focused terminal
ghostty-bridge close focused

# Close the selected tab
ghostty-bridge close selected-tab
```

### Special keys

Keys supports names like `Enter`, `Escape`, `Tab`, `Space`, `Up`, `Down`, `Left`, `Right`, `Home`, `End`, `PageUp`, `PageDown`, `Backspace`, `Delete`, and modifier combos like `C-c`, `M-a`.

```sh
ghostty-bridge keys build-server C-c
ghostty-bridge keys build-server Escape Enter
```

## How it works

ghostty-bridge uses Ghostty's AppleScript API to control windows, tabs, and terminals. It pipes AppleScript to `osascript` via stdin and parses the structured output.

Layout templates are parsed from TOML and expanded into a split tree by recursively opening panes from a fresh Ghostty window. Pane setup is injected as shell input (`cd`, `export`, then the requested command) so each leaf can be configured independently.

Terminal labels are stored in `~/Library/Application Support/ghostty-bridge/labels.json` and are not tied to Ghostty sessions.

Agent-to-agent messages use a parseable framing line:
`[ghostty-bridge:<sender> >>> <recipient>] <body>`.
`reply` and `read --since-last-message` both operate by parsing that visible transcript format.

The `read` command works by using `select_all` + `copy_to_clipboard` via AppleScript, reading the clipboard via `pbpaste`, then restoring the original clipboard. There is a brief window (~100ms) where the clipboard is in use.

## Diagnostics

Run `ghostty-bridge doctor` to verify that Ghostty is running, check the version, count terminals, and test terminal identification.

## License

MIT
