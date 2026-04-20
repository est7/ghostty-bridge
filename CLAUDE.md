# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

- Build: `cargo build`
- Run the CLI from source: `cargo run -- <subcommand>`
- Install the local binary: `cargo install --path .`
- Run tests: `cargo test`
- Run a single test: `cargo test <test_name> -- --exact`
- Format: `cargo fmt`
- Lint: `cargo clippy --all-targets --all-features`

For end-to-end verification, run inside Ghostty on macOS:

- Connectivity check: `cargo run -- doctor`
- List visible terminals: `cargo run -- list`
- Read the current terminal ID: `cargo run -- id`

There is no dedicated integration test harness yet; manual verification against a live Ghostty instance is part of normal development for AppleScript-facing changes.

## Project context

Read `AGENT_CONTEXT.md` at the start of a session. It is the repository's handoff document and the canonical place for durable project context, architecture notes, and known gaps.

## Architecture

`ghostty-bridge` is a small Rust CLI that wraps Ghostty's AppleScript API for agent-to-agent terminal control. The codebase is intentionally split into three layers:

- `src/main.rs`: clap-based CLI surface and command dispatch.
- `src/applescript.rs`: all Ghostty and macOS process interaction.
- `src/labels.rs`: persistent label-to-terminal-ID mapping in `~/Library/Application Support/ghostty-bridge/labels.json`.

Keep new work inside those boundaries unless the feature truly needs a new module.

## Key implementation details

- Target resolution happens in two steps: user input is first resolved from label to terminal UUID, then AppleScript functions re-scan Ghostty's current terminal list and act by index. If a command cannot find a terminal by UUID, it fails before issuing AppleScript input.
- Built-in selectors `focused`, `selected-tab`, and `front-window` are preferred over cwd-based `id` detection when a command can target active Ghostty context directly.
- AppleScript is always piped to `osascript` through stdin. Do not switch these scripts to `osascript -e`; the repository context documents escaping problems with that approach.
- `read` is clipboard-based: it saves the current clipboard, triggers Ghostty `select_all` and `copy_to_clipboard`, reads via `pbpaste`, then restores the clipboard. Any change here needs to preserve clipboard restoration behavior.
- `id` detection is layered (first match wins): (1) `GHOSTTY_BRIDGE_TERMINAL_ID` env, which `layout apply` and `open` inject into the pane's bootstrap line so every descendant process inherits its pane identity; (2) PID-ancestor match against Ghostty's `pid` property (Ghostty 1.4+). Requires `TERM_PROGRAM=ghostty` at every layer. Ghostty versions older than 1.4.0 are out of support.
- `layout apply` and `open` wrap the user command in `exec sh -c 'export GHOSTTY_BRIDGE_TERMINAL_ID=<uuid> [GHOSTTY_BRIDGE_LABEL=<label>] && [cd <cwd> &&] exec <user-cmd>'`. `sh` is used (not the user's login shell) so `export` keeps its POSIX meaning even when the user runs fish; `exec` chains keep the AI process as the pane's top-level PID.
- Layout templates live in TOML files and are applied by recursively opening splits from a fresh Ghostty window, then typing synthesized shell setup into each leaf pane.
- Do not add new automation or tests on top of `reply` / `read --since-last-message` transcript parsing. Future agent routing should move to plugin hooks; see `docs/hook-based-messaging.md`.

## Development notes that matter

- The project is macOS-only today because the IPC path is AppleScript-based.
- `doctor` is the quickest sanity check before debugging failures; it verifies Ghostty presence, version access, terminal enumeration, and current-terminal detection.
- `README.md` is accurate for the public command surface, but `AGENT_CONTEXT.md` captures the more important engineering constraints and open gaps.
- `layouts/ai-trio.toml` is the canonical example layout for the declarative split-tree feature.
