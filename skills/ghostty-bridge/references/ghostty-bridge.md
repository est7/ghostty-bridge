# ghostty-bridge — Deeper Reference

Extended CLI semantics, framing conventions, and layout template rules. Load this when the short playbook in `SKILL.md` isn't enough — typically when debugging odd framing, designing a non-trivial pane tree, or deciding which of the four "read the latest message" paths to use.

---

## 1. Bridge message framing

The bridge uses a single line format for every agent-to-agent message:

```
[ghostty-bridge:<sender> >>> <recipient>] <body>
```

- `<sender>` — the sender's terminal UUID (or, if labeled, the UUID is still used in the wire format). Receivers can resolve it to a label via `ghostty-bridge resolve` or `ghostty-bridge list`.
- `<recipient>` — the label or UUID the sender passed to `message` / `reply`.
- `<body>` — single line; newlines are rejected by `message` and `reply` so the frame stays parseable.

This line shape is the contract three commands depend on:

1. `message` — emits it.
2. `reply` — finds the latest such line in **your own** terminal and replies to `<sender>`.
3. `read --since-last-message` — finds the latest such line in **the target** terminal's capture and slices to what came after it.

If you ever need to change the framing, everything above has to change together. Treat `src/main.rs` as canonical; docs are derived from it.

### Why bodies must be single-line

`message` and `reply` reject bodies containing newlines. The parser is line-oriented: a multi-line body would be indistinguishable from "framed line + ordinary terminal output" when someone reads the transcript later. For multi-paragraph content, send a link, a gist, or split into multiple `message` calls.

---

## 2. Four ways to "read the latest message" — pick the right one

These look similar and get confused. The distinction is **whose terminal** you're reading and **which cut** you want.

| You are… | You want… | Command |
|---|---|---|
| **receiver** (the frame is addressed to you) | full recent context, see the frame itself | `ghostty-bridge read focused N` |
| **receiver** | only what arrived after the frame, not the frame itself | `ghostty-bridge read focused N --since-last-message` |
| **receiver** | reply back to whoever sent the frame | `ghostty-bridge reply '<body>'` (optionally `--lines N`) |
| **sender** (you messaged a peer, now want their reply block) | pull the peer's output after their framed reply line | `ghostty-bridge read <peer> N --since-last-message` |

Notes:

- `reply` is the usual answer to "someone sent me a message" — it reads your terminal, parses the last frame, resolves the sender, and sends the reply. No manual parsing needed.
- `read --since-last-message` returns the full capture unchanged if no bridge-framed line is found. That's a feature: it stops you from silently losing output if the frame rolled out of scrollback. Bump `N` if that happens.
- Avoid using `read <peer>` as a polling loop. Agent replies already come back to your terminal — see the "DO NOT WAIT OR POLL" note at the top of `SKILL.md`.

---

## 3. Target resolution order

When a command gets a target string, it's resolved in this order:

1. **Built-in selector** — `focused`, `selected-tab`, `front-window`.
   - `focused` → the currently focused terminal.
   - `selected-tab` / `front-window` → multiple terminals. Only commands that can fan out (`broadcast`, `close`, partially `focus`) accept these. `type` / `message` / `read` / `exec` reject them with a clear error.
2. **UUID** — looks like `B7B29D1F-3720-48AC-ADA7-D507B260E1F0`. Used directly.
3. **Label** — resolved against `~/Library/Application Support/ghostty-bridge/labels.json`. If missing, the command fails before touching Ghostty.

Labels are case-sensitive and persist across sessions. Re-labeling replaces the existing mapping; nothing garbage-collects orphan labels whose terminal has closed — run `ghostty-bridge list` occasionally and re-label.

---

## 4. Layout templates

`layout apply` opens a **new tab in Ghostty's front window** and builds a split tree declared in TOML. Leaves are panes; each non-leaf is a binary split with a direction.

### Minimum validate-then-apply flow

```bash
ghostty-bridge layout validate ~/.ghostty-bridge/my-layout.toml
ghostty-bridge layout apply    ~/.ghostty-bridge/my-layout.toml
```

`validate` parses the file, checks the tree shape, rejects unknown fields, and enforces the "at most one focused pane" rule. It opens nothing. Always validate before applying — templates fail fast instead of building half a layout.

### Grammar

```
LayoutFile := { name?: string; root: LayoutNode }
LayoutNode := Pane | Split

Pane  := { type = "pane",
           label?: string,
           cwd?: string,
           command?: string,          # mutually exclusive with `input`
           input?: string,            # mutually exclusive with `command`
           env?: ["KEY=VALUE", …],
           focus?: bool }

Split := { type = "split",
           direction = "right" | "left" | "down" | "up",
           # exactly two children, determined by direction:
           left?: LayoutNode, right?: LayoutNode,    # for right/left
           top?: LayoutNode,  bottom?: LayoutNode }  # for down/up
```

Rules:

- Unknown TOML keys are rejected (`deny_unknown_fields`). A typo like `commnad = "foo"` will fail validation with the offending path.
- `direction = "right"` means the split opens a new pane to the right of the current one. The "current" child is on the left. Symmetric for `left` / `down` / `up`.
- A split with `direction = "right"` or `"left"` must define both `left` and `right`. `direction = "down"` or `"up"` must define both `top` and `bottom`.
- A split may not mix `left`/`right` with `top`/`bottom`.
- At most one pane in the whole tree may set `focus = true`.
- A pane may not set both `command` and `input`.
- `env` entries must match `KEY=VALUE` where `KEY` is `[A-Za-z_][A-Za-z0-9_]*`. Malformed entries abort validation.

### `cwd` semantics

`cwd` is applied **after** the pane is created, by typing `cd <cwd> && <rest>` as shell input. Accepted forms:

- `"."` or `"$PWD"` — the directory you ran `ghostty-bridge layout apply` from.
- `"./sub"` or `"$PWD/sub"` — relative to that directory.
- `"~"` or `"~/path"` — home-expanded.
- Absolute paths — used as-is.

Caveat: the root tab still starts in Ghostty's default directory (there's no way to set it at tab-creation time from AppleScript today). `cwd = "."` only takes effect once the shell prompt has appeared and accepts input. In practice this is fine; in slow-startup shells you may see the first prompt briefly in the default dir before the `cd` lands.

### Per-pane startup sequence

For each leaf pane, the bridge types one line joined with `&&`:

```
cd <cwd> && export KEY=VALUE … && <command-or-input>
```

Missing pieces are skipped. Example:

- `cwd = "~/work"`, `command = "cargo run"` → `cd '/Users/you/work' && cargo run`
- `cwd = "."`, `env = ["API_KEY=xyz"]`, `command = "pnpm dev"` → `cd '/current' && export API_KEY='xyz' && pnpm dev`
- No `cwd`, no `env`, `command = "claude"` → `claude`
- `input = "some literal text"` → `some literal text` (typed without prepended `cd`/`export` only if cwd/env are unset; otherwise chained the same way)

The whole line is typed, then Enter is sent.

### Four-pane example

```toml
name = "ai-quad"

[root]
type = "split"
direction = "down"

[root.top]
type = "split"
direction = "right"

[root.top.left]
type = "pane"
label = "main"
cwd = "."
command = "ccd"

[root.top.right]
type = "pane"
label = "codex"
cwd = "."
command = "cxd"

[root.bottom]
type = "split"
direction = "right"

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
focus = true
```

### Three-pane, one large + two stacked

```toml
name = "ai-trio"

[root]
type = "split"
direction = "right"

[root.left]
type = "pane"
label = "claude"
cwd = "."
command = "claude"

[root.right]
type = "split"
direction = "down"

[root.right.top]
type = "pane"
label = "codex"
cwd = "."
command = "codex"

[root.right.bottom]
type = "pane"
label = "gemini"
cwd = "."
command = "gemini"
focus = true
```

### Common validation errors

| Message | Cause |
|---|---|
| `split with direction = right must define [root.left]` | Missing the paired child on a horizontal split. |
| `left child is only valid for left/right splits` | You gave a horizontal child to a vertical split. |
| `layout may mark at most one pane with focus = true` | Two or more panes set `focus = true`. |
| `pane cannot set both command and input` | Both fields present. Pick one. |
| `invalid env assignment '<entry>': …` | `env = [...]` entry is malformed. |
| `unknown field \`commnad\`` | TOML key typo; `deny_unknown_fields` rejects it. |

---

## 5. `read` mechanics and clipboard cost

`read` works by driving Ghostty's AppleScript:

1. Save the current macOS clipboard (`pbpaste` into a buffer).
2. Tell Ghostty to `select_all` then `copy_to_clipboard` on the target terminal.
3. `pbpaste` the capture.
4. Restore the original clipboard via `pbcopy`.

Consequences:

- The clipboard is briefly clobbered (~100 ms). Don't run two `read`s in parallel, and don't start a `read` while another process is writing to the clipboard.
- `read` returns whatever is currently **visible or scrollback-accessible** in that terminal. It isn't a ring buffer the bridge maintains — lines that scrolled out of Ghostty's scrollback are gone.
- `--since-last-message` is a pure string slice on the captured text. It can't recover content that was already lost.

If `perform action "write_screen_file"` ever works reliably via AppleScript, the bridge should switch to it — that would avoid the clipboard detour entirely. Today it returns false, so the clipboard path is the one in production.

---

## 6. `broadcast` fan-out

`broadcast` accepts `--target <t>` repeatedly plus one of `--text <text>` or `--keys <key>...`. It resolves every target through the normal label/UUID pipeline, deduplicates, and then reuses the single-terminal `type` / `keys` primitives per target. There is no new AppleScript path — it's N sequential single-target calls, validated up front so a stale label aborts the whole broadcast instead of producing a partial fan-out.

`selected-tab` and `front-window` selectors fan out naturally through `broadcast`.

---

## 7. `id` is heuristic

`ghostty-bridge id` matches the current terminal by `$TERM_PROGRAM == "ghostty"` + current working directory. If two terminals share a cwd, the match is ambiguous and `id` may return either.

Prefer built-in selectors when you're scripting from a terminal whose identity matters:

- `focused` — whatever is focused right now.
- `selected-tab` / `front-window` — the current tab / window group.

Use `id` only when you really need "the UUID of the process running this shell" and you've accepted the cwd-collision risk.

---

## 8. `open` surface configuration

`open window | tab | split` builds a Ghostty `surface configuration` record and passes it to the corresponding AppleScript verb. Flags:

- `--cwd <path>` — initial working directory (applied at surface creation, so unlike layout `cwd` it affects the very first prompt).
- `--command <cmd>` — the initial command.
- `--input <text>` — initial typed input.
- `--wait` — keeps the surface open after the command exits.
- `--env KEY=VALUE` — repeatable; sets environment variables on the surface.
- `--label <name>` — labels the new surface immediately, no second `name` call.

`open split` defaults to splitting the focused terminal when `--target` is omitted. Use `--direction right|left|down|up` to override the split orientation.

`open` prints the new terminal's UUID on success, so callers can chain:

```bash
id=$(ghostty-bridge open tab --cwd ~/work --command 'claude')
ghostty-bridge message "$id" 'Ready to review.'
```

---

## 9. Known gaps

- **Root tab cwd**: `layout apply` can't set the root tab's starting directory at creation time — see the `cwd` caveat above.
- **Read includes the `select_all` highlight**: The visible capture includes whatever is on screen, which for a brief moment includes the selection highlight rendered by `select_all`. It's cosmetic in the text output, not a correctness issue.
- **`write_screen_file` is broken**: Returns false via AppleScript. If that's ever fixed upstream, `read` should migrate.
- **No Linux**: AppleScript is macOS-only. Linux parity needs a different IPC path.
- **No `shell-setup`**: Planned helper that emits shell functions (aliases for `message`/`reply`, prompt hooks to render the framing line nicely). Not yet implemented.
- **`find_terminal_index` is chatty**: Every targeted call runs a full Ghostty list. Batching layer is future work.
- **`layout apply --target`**: Doesn't exist yet — layouts always open a new tab. If you need to rebuild into an existing tab, close it and reapply.
