# Hook-Based Agent Messaging

## Status

`ghostty-bridge message` remains the delivery primitive. `reply` and `read --since-last-message`
remain available for manual use, but they are not the foundation for future agent automation.

Do not add more fallback logic, transcript parsing tricks, or end-to-end coverage on top of the
current read-and-reply path.

## Problem

The current agent loop mixes transport semantics with terminal rendering:

- send: `message` types a framed line into a target pane with `input_text`
- receive: `reply` reads the visible screen through clipboard capture and parses
  `[ghostty-bridge:<sender> >>> <recipient>] ...`

That receive path is brittle because Claude Code and Codex render inside TUIs that are free to:

- prefix lines
- wrap or reflow content
- redraw composer state
- hide or transform the exact text that was typed

Every extra fallback added here increases complexity without changing the underlying failure mode.

## Decision

Future agent routing should move to plugin-provided hooks for Claude Code and Codex.

The hook should observe the agent's structured end-of-turn output, then call
`ghostty-bridge message` back to the orchestrator pane. Ghostty stays responsible for pane
creation, labeling, focus, and message delivery. The agent plugin owns the extraction of the final
assistant reply.

## Why Hooks

Both tools now expose a `Stop` hook with `last_assistant_message`:

- Claude Code: `Stop` hook includes `last_assistant_message` and `stop_hook_active`
- Codex: `Stop` hook includes `last_assistant_message` and `stop_hook_active`

That is the correct boundary. The plugin can inspect the agent's final response directly instead of
trying to reconstruct it from terminal pixels.

## Proposed Flow

1. The orchestrator opens or targets a pane as it does today.
2. The orchestrator prompt tells the agent to end with one machine-readable bridge line.
3. The local Claude Code / Codex plugin installs a `Stop` hook.
4. The hook reads `last_assistant_message`, extracts the bridge payload, and validates it.
5. The hook invokes `ghostty-bridge message <orchestrator-target> <body>`.

## Suggested Payload Shape

The final assistant message should end with one line like:

```text
GHOSTTY_BRIDGE_REPLY {"to":"orchestrator","conversation_id":"abc123","body":"done"}
```

Constraints:

- exactly one line
- JSON payload only after the marker
- `body` is the text to send back through `ghostty-bridge message`
- `conversation_id` lets the orchestrator correlate retries or parallel work

If the marker is absent, the hook should no-op instead of guessing.

## Non-Goals

- no new screen-scraping logic
- no new `reply` fallbacks
- no transcript-based e2e suite for Claude/Codex TUIs
- no side-channel inbox/socket work unless hooks prove insufficient

## Migration Notes

- Keep `reply` and `read --since-last-message` documented as transcript-based best-effort tools.
- Prefer plugin hooks for any Claude/Codex orchestration that expects reliable callbacks.
- If a future test suite is added, test the hook contract and the `ghostty-bridge message` call,
  not the TUI's visible rendering.
