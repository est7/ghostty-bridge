---
name: ghostty-bridge
description: Drive Ghostty terminals from bash via the ghostty-bridge CLI — send keystrokes, read pane output, and coordinate agent-to-agent messaging across Ghostty windows. Use this skill whenever the user mentions Ghostty, cross-terminal communication, talking to another agent in another Ghostty window, sending a message/command to another terminal, reading another Ghostty terminal's output, labeling Ghostty terminals, or anything involving ghostty-bridge / osascript-driven terminal automation on macOS.
metadata:
  { "openclaw": { "emoji": "🖥️", "os": ["darwin"], "requires": { "bins": ["ghostty-bridge"] } } }
---

# ghostty-bridge

Cross-terminal control and agent-to-agent messaging for Ghostty. All interaction goes through the `ghostty-bridge` CLI, which drives Ghostty via AppleScript. Plain bash — any agent that can run shell commands can use it.

Every command is **atomic**:
- `type` — types text, no Enter
- `keys` — sends special keys (Enter, Escape, C-c, …)
- `read` — captures terminal output
- `message` — types a framed bridge line and presses Enter in one shot
- `reply` — reads the current terminal, finds the latest bridge message, and answers the sender in one shot

## Requirements

- macOS
- Ghostty 1.3.0+ with AppleScript support enabled
- `ghostty-bridge` on PATH (`cargo install ghostty-bridge`)

Run `ghostty-bridge doctor` first if the CLI seems unresponsive — it checks that Ghostty is running, reports the version, counts terminals, and verifies terminal identification.

## DO NOT WAIT OR POLL

When you `message` another agent, their reply arrives directly in YOUR terminal as a `[ghostty-bridge:<sender> >>> <recipient>] <body>` line. Do NOT:

- sleep or wait after sending
- poll / re-read the target terminal to check for a response
- loop "just in case"

**Send and move on.** The only times you read a target terminal are:
1. Before interacting — to see what's on screen
2. After `type` — to verify the text landed before you press Enter
3. When the target is a **non-agent terminal** (plain shell or process) and there's nobody to reply back

## Command Reference

| Command | Description |
|---|---|
| `ghostty-bridge list [--json]` | Show all terminals (id, name, cwd, label). Add `--json` for a stable scriptable shape |
| `ghostty-bridge id` | Print this terminal's Ghostty UUID |
| `ghostty-bridge name <target> <label>` | Label a terminal for easy reference |
| `ghostty-bridge resolve <label>` | Resolve a label to a terminal UUID |
| `ghostty-bridge read <target> [lines] [--since-last-message]` | Read last N lines (default 50). `--since-last-message` returns only output after the latest bridge-framed line |
| `ghostty-bridge type <target> <text>` | Type text (no Enter) |
| `ghostty-bridge keys <target> <key>…` | Send special keys |
| `ghostty-bridge message <target> <text>` | Type a framed bridge line and press Enter |
| `ghostty-bridge reply <text>` | Read the current terminal, find the last bridge message, and message the sender back |
| `ghostty-bridge layout validate <file>` | Validate a TOML layout template without opening anything |
| `ghostty-bridge layout apply <file>` | Open a new tab in the front window and build the declared split tree |
| `ghostty-bridge doctor` | Diagnose Ghostty connectivity |

Targets can be a Ghostty UUID (e.g. `B7B29D1F-3720-48AC-ADA7-D507B260E1F0`) or any label previously set with `ghostty-bridge name`. Labels persist at `~/Library/Application Support/ghostty-bridge/labels.json` and survive session restarts.

### Special Keys

`keys` accepts `Enter`, `Escape`, `Tab`, `Space`, `Up`, `Down`, `Left`, `Right`, `Home`, `End`, `PageUp`, `PageDown`, `Backspace`, `Delete`, and modifier combos like `C-c`, `C-d`, `M-a`. Multiple keys can be chained: `ghostty-bridge keys worker Escape Enter`.

## Playbook

### 1. Bootstrap — label yourself, discover peers

```bash
ghostty-bridge name "$(ghostty-bridge id)" claude     # label your own terminal
ghostty-bridge list                                    # see all Ghostty terminals
```

### 2. Message another agent (one shot — preferred)

```bash
ghostty-bridge read codex 20                           # see what's on codex's screen
ghostty-bridge message codex 'Please review src/auth.ts'
# Done. Do NOT poll codex. Their reply arrives in YOUR terminal.
```

`message` auto-prepends sender framing before pressing Enter:

```
[ghostty-bridge:B7B2...E1F0 >>> codex] Please review src/auth.ts
```

### 3. Receive a message → reply via ghostty-bridge

When you see a line starting with `[ghostty-bridge:<sender> >>> <recipient>] ...` in your own terminal, reply back with `ghostty-bridge reply` from that terminal:

```bash
ghostty-bridge reply '87% line coverage; OAuth refresh path uncovered (auth.ts:142-168).'
```

`reply` reads the current terminal, finds the latest bridge-framed line, resolves the sender through the existing label-or-UUID pipeline, and sends the response there. Use `--lines N` if the relevant message may be farther up scrollback.

### 4. Read only the output since the last agent message

```bash
ghostty-bridge read claude 80 --since-last-message
```

This slices the visible transcript after the last recognized bridge-framed line. If there is no bridge message in the captured output, it returns the full read unchanged.

### 4b. Grab only the peer's latest reply after you messaged them

`reply --lines N` slices on **your own** terminal — it finds the last bridge line you received. To pull the last chunk from the peer's terminal (their reply block after the framing line they sent you), target that peer and use `--since-last-message`:

```bash
# You sent a message to codex, now pull only what codex emitted after the framing line.
ghostty-bridge read codex 200 --since-last-message
```

Use this sparingly. If the peer is an agent, their reply should arrive in your terminal on its own — polling the peer is the anti-pattern the top of this file warns about. This path is for non-agent peers or for explicit forensic reads.

### 5. Type a command manually (verify before Enter)

Use `type` + `keys Enter` when you want to see the text land before submitting. Follow **read → type → read → keys**:

```bash
ghostty-bridge read build-server 20
ghostty-bridge type build-server "cargo build --release"
ghostty-bridge read build-server 20       # verify
ghostty-bridge keys build-server Enter
```

### 6. Drive a non-agent terminal (prompt, REPL, TUI)

No agent on the other side, so you MUST read after submitting to see the result:

```bash
ghostty-bridge read worker 10             # see the prompt
ghostty-bridge type worker "y"
ghostty-bridge read worker 10             # verify
ghostty-bridge keys worker Enter
ghostty-bridge read worker 20             # see the outcome
```

### 7. Interrupt or escape a running process

```bash
ghostty-bridge keys build-server C-c                  # Ctrl-C
ghostty-bridge keys worker Escape Enter               # dismiss + submit
```

### 8. Open a pane layout from a TOML template

`layout apply` opens a **new tab in the front Ghostty window** and builds the declared split tree. Each leaf pane is seeded by typing `cd ... && export ... && <command>` after the pane opens, so `cwd` is applied after creation (the root tab itself still starts in Ghostty's default directory).

Put templates wherever you like; `~/.ghostty-bridge/` is a reasonable default. Validate first, then apply:

```bash
ghostty-bridge layout validate ~/.ghostty-bridge/ai-quad.toml
ghostty-bridge layout apply    ~/.ghostty-bridge/ai-quad.toml
```

Template shape — each `split` has a `direction` and exactly two children (`left`+`right` for `right`/`left`, `top`+`bottom` for `down`/`up`). Leaves are `pane` nodes.

Four-pane example (`ai-quad.toml`), top-left / top-right / bottom-left / bottom-right, all inheriting the current directory:

```toml
name = "ai-quad"

[root]
type = "split"
direction = "down"        # split the tab top / bottom first

[root.top]
type = "split"
direction = "right"       # then left / right inside the top half

[root.top.left]
type = "pane"
label = "main"
cwd = "."                 # "." or "$PWD" resolves to the cwd of `layout apply`
command = "ccd"

[root.top.right]
type = "pane"
label = "codex"
cwd = "."
command = "cxd"

[root.bottom]
type = "split"
direction = "right"       # and left / right inside the bottom half

[root.bottom.left]
type = "pane"
label = "yazi"
cwd = "."
command = "yazi"

[root.bottom.right]
type = "pane"
label = "claude"
cwd = "."
command = "ccd"
focus = true              # at most one pane may set focus = true
```

Pane fields: `label`, `cwd`, `command`, `input`, `env = ["KEY=VALUE"]`, `focus = true`.

`cwd` accepts:
- `"."` or `"$PWD"` — the directory you ran `layout apply` from
- `"./sub"` or `"$PWD/sub"` — relative to that directory
- `"~"`, `"~/path"` — home-expanded
- absolute paths

Gotchas:
- Root tab starts in Ghostty's default directory; `cwd = "."` only takes effect after the first shell prompt, because it's applied via `cd` inside each pane.
- `command` and `input` are mutually exclusive on a pane.
- At most one pane in the whole tree may set `focus = true`.
- Unknown fields (typos like `commnad = ...`) are rejected — validation will tell you which path failed.

## How `read` works (and why it costs the clipboard briefly)

`read` uses Ghostty's AppleScript API: `select_all` + `copy_to_clipboard` → `pbpaste` → restore the original clipboard. The clipboard is in use for roughly 100 ms. Avoid concurrent clipboard writes during `read`.

## Choosing `message` vs `type` + `keys Enter`

- **`message`** — default for agent-to-agent. Uses the framing `[ghostty-bridge:<sender> >>> <recipient>] <body>`, presses Enter, one round trip.
- **`type` + `keys Enter`** — when you need to verify the text landed exactly, or when framing would pollute the input (e.g. typing into a search prompt, a REPL, or a non-agent tool).

## Deeper Reference

For the full CLI semantics, framing conventions, and extended examples, see `references/ghostty-bridge.md`.

## Tips

- **Never poll.** Agent replies come to you; non-agent terminals need a final `read`.
- **Label early.** `ghostty-bridge name <uuid> <label>` once, then use the label everywhere.
- **`type` is literal** (`-l` semantics) — special characters are typed as-is, and it does NOT press Enter.
- **`read` default = 50 lines**; pass a larger N for more scrollback.
- **Clipboard is briefly taken during `read`** — don't stack simultaneous `read`s.
- **`ghostty-bridge doctor`** is your first move when something feels off.
