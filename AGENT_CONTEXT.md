# AGENT_CONTEXT.md

## Purpose
This file is the official agent handoff document and the source of truth for the current state of the project.

## Agent Instructions
- Read this file at the start of every new session.
- Update this file whenever you learn important durable context about the project.
- Update this file whenever the project's behavior, architecture, requirements, or decisions materially change.
- Prefer stable facts and decisions over temporary notes.
- Clearly label planned work, open questions, or assumptions so they are not mistaken for current state.
- If another file is canonical for a specific area, record it here.

## Canonical References
- Overall project state: this file
- Domain-specific source of truth: this file

## Architecture

A Rust CLI (`ghostty-bridge`) that bridges AI agents to Ghostty terminal panes via AppleScript.
Ghostty 1.3.0+ exposes an AppleScript API for window/tab/terminal control. `ghostty-bridge` wraps
this API and adds a label system for friendly terminal naming.

- `src/main.rs` — CLI entry point, clap arg parsing, command dispatch
- `src/applescript.rs` — All AppleScript interaction (list, input_text, send_key, read_terminal, etc.)
- `src/labels.rs` — Label-to-ID mapping stored in `~/Library/Application Support/ghostty-bridge/labels.json`

The binary is invoked as `ghostty-bridge`.

## Current State

**v0.1.0 — All core commands working:**

| Command | Status | Notes |
|---------|--------|-------|
| `ghostty-bridge list` | Working | Shows id, name, cwd, label for all Ghostty terminals |
| `ghostty-bridge type <target> <text>` | Working | Sends text without pressing Enter |
| `ghostty-bridge keys <target> <key>...` | Working | Sends special keys (Enter, C-c, etc.) |
| `ghostty-bridge read <target> [lines]` | Working | Reads screen via select_all + copy_to_clipboard, restores clipboard |
| `ghostty-bridge message <target> <text>` | Working | Types text with sender info and presses Enter |
| `ghostty-bridge name <target> <label>` | Working | Labels a terminal for easy reference |
| `ghostty-bridge resolve <label>` | Working | Resolves a label to a terminal UUID |
| `ghostty-bridge id` | Working | Identifies current terminal by matching cwd |
| `ghostty-bridge doctor` | Working | Diagnoses Ghostty connectivity |

## Important Decisions

1. **Read via clipboard save/restore** — `ghostty-bridge read` uses `select_all` + `copy_to_clipboard` via AppleScript,
   reads `pbpaste`, then restores the original clipboard via `pbcopy`. There is a small (~100ms) window
   where the clipboard is clobbered. Alternatives considered: Accessibility API (needs permissions),
   shell function injection (requires user setup), `write_screen_file` (returns false via AppleScript).

2. **AppleScript via stdin** — Scripts are piped to `osascript` via stdin rather than `-e` flag, because
   `-e` mode doesn't handle certain escape sequences. Field delimiter is `|||`.

3. **Terminal identification** — `ghostty-bridge id` matches by `$TERM_PROGRAM == "ghostty"` + current working directory.
   Ambiguous if multiple terminals share the same cwd. Could be improved with PTY device matching.

4. **Label storage** — JSON file in `~/Library/Application Support/ghostty-bridge/labels.json`. Labels
   are ephemeral and not tied to Ghostty sessions.

## Open Questions / Known Gaps

- **Read includes typed commands** — `ghostty-bridge read` captures the full visible screen, including the `select_all`
  visual highlight and any commands injected by the tool. No way to get clean command output boundaries
  without Ghostty exposing buffer semantics (OSC 133 markers are internal only).
- **`perform action "write_screen_file"`** returns false via AppleScript — may need a Ghostty fix or
  different action string format. If this worked, it would be a cleaner read path than clipboard.
- **Terminal identification is fragile** — matching by cwd alone can be ambiguous. Could be improved
  with tty device matching or a Ghostty feature request for an env var like `GHOSTTY_TERMINAL_ID`.
- **No Linux support** — Ghostty AppleScript is macOS only. Linux would need a different IPC mechanism.
- **No shell setup command yet** — planned `ghostty-bridge shell-setup` to emit shell helper functions.

## Notes For The Next Agent

- Build: `cargo build` in project root. Binary at `target/debug/ghostty-bridge`.
- Install: install via `cargo install --path .` or copy `target/debug/ghostty-bridge` to your PATH.
- Test: run `ghostty-bridge doctor` first to verify Ghostty connectivity.
- The AppleScript scripts must be piped via stdin to `osascript`, not passed with `-e`.
- Ghostty terminal IDs are UUIDs like `B7B29D1F-3720-48AC-ADA7-D507B260E1F0`.
