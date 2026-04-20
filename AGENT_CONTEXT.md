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
- Future agent-messaging direction: `docs/hook-based-messaging.md`

## Architecture

A Rust CLI (`ghostty-bridge`) that bridges AI agents to Ghostty terminal panes via AppleScript.
Ghostty 1.4.0+ exposes the AppleScript properties this project now relies on for terminal identity.
`ghostty-bridge` wraps
this API and adds a label system for friendly terminal naming.

- `src/main.rs` — CLI entry point, clap arg parsing, command dispatch, target resolution
- `src/applescript.rs` — All AppleScript interaction (list, input, keys, perform action, surface creation, selectors, lifecycle)
- `src/labels.rs` — Label-to-ID mapping stored in `~/Library/Application Support/ghostty-bridge/labels.json`

The binary is invoked as `ghostty-bridge`.

## Current State

**Core command surface:**

| Command | Status | Notes |
|---------|--------|-------|
| `ghostty-bridge list [--json]` | Working | Shows id, name, cwd, label. `--json` emits a stable JSON array for scripts |
| `ghostty-bridge type <target> <text>` | Working | Sends text without pressing Enter |
| `ghostty-bridge keys <target> <key>...` | Working | Sends special keys (Enter, C-c, etc.) |
| `ghostty-bridge read <target> [lines] [--since-last-message]` | Working | Reads screen via select_all + copy_to_clipboard, restores clipboard. `--since-last-message` slices output after the last bridge-framed line |
| `ghostty-bridge message <target> <text>` | Working | Types `[ghostty-bridge:<sender> >>> <recipient>] <text>` and presses Enter |
| `ghostty-bridge reply <text> [--lines N]` | Working | Reads the current terminal, finds the last bridge message, and messages the sender back |
| `ghostty-bridge exec <target> <command>` | Working | Types a command and presses Enter in one call |
| `ghostty-bridge broadcast --target <t>... --text\|--keys` | Working | Fans out text or keys across multiple resolved terminals |
| `ghostty-bridge open window\|tab\|split` | Working | Creates a new Ghostty surface via `new surface configuration` |
| `ghostty-bridge layout validate <file>` | Working | Validates a TOML layout template before opening anything |
| `ghostty-bridge layout apply <file>` | Working | Opens a new tab in the front window, builds a split tree, labels panes, and seeds commands |
| `ghostty-bridge focus <target>` | Working | Focuses a terminal or the front window |
| `ghostty-bridge close <target>` | Working | Closes a terminal, selected tab, or front window |
| `ghostty-bridge name <target> <label>` | Working | Labels a terminal for easy reference |
| `ghostty-bridge resolve <label>` | Working | Resolves a label to a terminal UUID |
| `ghostty-bridge id` | Working | Identifies current terminal via injected env var or Ghostty PID metadata |
| `ghostty-bridge doctor` | Working | Diagnoses Ghostty connectivity |

**Target resolution:**

- Plain UUIDs and labels stored in the label store.
- Built-in selectors: `focused`, `selected-tab`, `front-window`. `selected-tab` and `front-window` expand to multiple terminals and are only accepted by commands that can fan out (`broadcast`, `close`, and partially `focus`).

## Important Decisions

1. **Read via clipboard save/restore** — `ghostty-bridge read` uses `select_all` + `copy_to_clipboard` via AppleScript,
   reads `pbpaste`, then restores the original clipboard via `pbcopy`. There is a small (~100ms) window
   where the clipboard is clobbered. Alternatives considered: Accessibility API (needs permissions),
   shell function injection (requires user setup), `write_screen_file` (returns false via AppleScript).

2. **AppleScript via stdin** — Scripts are piped to `osascript` via stdin rather than `-e` flag, because
   `-e` mode doesn't handle certain escape sequences. Field delimiter for structured output is `|||`.

3. **Terminal identification** — `ghostty-bridge id` resolves in two layers, first match wins: (a) the
   `GHOSTTY_BRIDGE_TERMINAL_ID` environment variable, which `layout apply` and `open` inject into the pane's
   bootstrap line so every descendant process (including nested agents, tmux, or Claude Code) inherits the
   pane's identity; (b) the PID-ancestor chain walked against Ghostty's `pid` property (Ghostty 1.4+, PR
   #11922). Every layer requires `$TERM_PROGRAM == "ghostty"`. The env layer is the most reliable and handles
   nested tools and tmux; the PID layer is the only supported fallback for foreign panes. Ghostty versions older
   than 1.4.0 are now out of support.

4. **Label storage** — JSON file in `~/Library/Application Support/ghostty-bridge/labels.json`. Labels
   are ephemeral and not tied to Ghostty sessions.

5. **Surface creation uses `new surface configuration`** — `open window|tab|split` builds a Ghostty surface
   configuration record and passes it to the matching AppleScript verb. Supported flags: `--cwd`, `--command`,
   `--input`, `--wait`, `--env KEY=VALUE` (repeatable). `open` prints the new terminal UUID so callers can chain.
   `open split` defaults to splitting the focused terminal when `--target` is omitted.

6. **Broadcast fans out over resolved targets** — `broadcast` accepts repeated `--target` values, resolves each
   through the selector/label pipeline, deduplicates UUIDs, and then delegates to `input_text`/`send_key`. It
   does not introduce a new AppleScript path; the existing single-terminal primitives are reused.

7. **Layouts are TOML split trees applied from a new tab** — `layout apply` opens a new tab in Ghostty's front window,
   then recursively builds the declared split tree. Leaf panes are configured by wrapping the synthesized setup in
   `exec sh -c 'export GHOSTTY_BRIDGE_TERMINAL_ID=<uuid> [GHOSTTY_BRIDGE_LABEL=<label>] && [cd <cwd> &&] [export user env &&] exec <user command>'`.
   Using `sh` keeps `export` working regardless of the user's login shell (fish/zsh/bash), and the `exec` chain
   makes the user command the pane's top-level process. `open window|tab|split` uses the same pattern so that
   manually opened panes also carry `GHOSTTY_BRIDGE_TERMINAL_ID`. A layout may mark at most one pane with
   `focus = true`.

8. **Agent messages use a parseable framing line** — `message` and `reply` emit
   `[ghostty-bridge:<sender> >>> <recipient>] <body>`. `reply` and `read --since-last-message` both parse the
   visible terminal transcript using that exact framing, so docs and automation should treat `src/main.rs` as canonical.

9. **Structured terminal discovery is first-class** — `list --json` emits a stable array of `{ id, name, cwd, pid, tty, label }`
   objects so scripts do not need to scrape the human-readable table output. `pid` and `tty` come from Ghostty 1.4+.

## Open Questions / Known Gaps

- **Read includes typed commands** — `ghostty-bridge read` captures the full visible screen, including the `select_all`
  visual highlight and any commands injected by the tool. No way to get clean command output boundaries
  without Ghostty exposing buffer semantics (OSC 133 markers are internal only).
- **`perform action "write_screen_file"`** returns false via AppleScript — may need a Ghostty fix or
  different action string format. If this worked, it would be a cleaner read path than clipboard.
- **Terminal identification is reliable in panes seeded by this tool** — `GHOSTTY_BRIDGE_TERMINAL_ID` makes
  identity deterministic for every process descending from a pane opened by `layout apply` or `open`. For
  foreign panes (manually opened, or started before the tool existed) we now only rely on the Ghostty 1.4+
  `pid` property. There is no longer any `tty` or cwd fallback.
- **Transcript-based messaging remains the weak point** — `reply` and `read --since-last-message` still parse
  visible terminal text captured through the clipboard path. Do not extend that path with more fallbacks or new
  e2e coverage. The intended replacement is a plugin hook flow documented in `docs/hook-based-messaging.md`.
- **No Linux support** — Ghostty AppleScript is macOS only. Linux would need a different IPC mechanism.
- **No shell setup command yet** — planned `ghostty-bridge shell-setup` to emit shell helper functions.
- **`find_terminal_index` runs a full AppleScript list on every targeted call** — sequential commands are chatty.
  No batching layer yet.
- **Layout application currently opens a fresh tab only** — there is no `layout apply --target ...` variant for
  reusing an existing tab or pane tree yet.

## Notes For The Next Agent

- Build: `cargo build` in project root. Binary at `target/debug/ghostty-bridge`.
- Install: install via `cargo install --path .` or copy `target/debug/ghostty-bridge` to your PATH.
- Test: run `ghostty-bridge doctor` first to verify Ghostty connectivity.
- Example layout: `layouts/ai-trio.toml` demonstrates a left-pane + stacked-right-pane setup for Claude/Codex/Gemini.
- The AppleScript scripts must be piped via stdin to `osascript`, not passed with `-e`.
- Ghostty terminal IDs are UUIDs like `B7B29D1F-3720-48AC-ADA7-D507B260E1F0`.
- For new surface work, extend `SurfaceConfig` + `config_block` in `src/applescript.rs` rather than inlining
  AppleScript snippets in `main.rs`.
