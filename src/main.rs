mod applescript;
mod labels;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

const LAYOUT_HELP: &str = r#"Examples:
  ghostty-bridge layout validate layouts/ai-trio.toml
  ghostty-bridge layout apply layouts/ai-trio.toml

Template shape:
  name = "optional-name"

  [root]
  type = "split"
  direction = "right" # right | left | down | up

  [root.left]
  type = "pane"
  label = "claude"
  cwd = "~/path/to/project"
  command = "claude"

  [root.right]
  type = "pane"
  label = "codex"
  focus = true

Each pane may set: label, cwd, command, input, env = ["KEY=VALUE"], focus = true.
Use cwd = "." or cwd = "$PWD" to reuse the current working directory.
A layout may mark at most one pane with focus = true."#;

#[derive(Parser)]
#[command(name = "ghostty-bridge", bin_name = "ghostty-bridge")]
#[command(about = "cross-pane communication for AI agents via Ghostty")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Show all terminals (id, name, cwd, label)")]
    List {
        #[arg(long)]
        json: bool,
    },

    #[command(about = "Type text into a terminal without pressing Enter")]
    Type { target: String, text: String },

    #[command(about = "Type text with auto-prepended sender info and reply target")]
    Message { target: String, text: String },

    #[command(about = "Reply to the latest ghostty-bridge message in this terminal")]
    Reply {
        text: String,
        #[arg(long, default_value = "200")]
        lines: usize,
    },

    #[command(about = "Read terminal output")]
    Read {
        target: String,
        #[arg(default_value = "50")]
        lines: usize,
        #[arg(long)]
        since_last_message: bool,
    },

    #[command(about = "Send special keys (Enter, Escape, C-c, etc.)")]
    Keys {
        target: String,
        #[arg(num_args = 1..)]
        keys: Vec<String>,
    },

    #[command(about = "Label a terminal")]
    Name { target: String, label: String },

    #[command(about = "Print terminal id for a label")]
    Resolve { label: String },

    #[command(about = "Print this terminal's Ghostty id")]
    Id,

    #[command(about = "Diagnose Ghostty connectivity")]
    Doctor,

    #[command(about = "Open a new Ghostty window, tab, or split")]
    Open(OpenArgs),

    #[command(about = "Send a command to a terminal and press Enter")]
    Exec { target: String, command: String },

    #[command(about = "Run the same text or keys across multiple targets")]
    Broadcast(BroadcastArgs),

    #[command(about = "Focus a terminal or active Ghostty context")]
    Focus { target: String },

    #[command(about = "Close a terminal or active Ghostty context")]
    Close { target: String },

    #[command(
        about = "Apply or validate a layout template",
        after_help = LAYOUT_HELP
    )]
    Layout {
        #[command(subcommand)]
        command: LayoutCommands,
    },
}

#[derive(Subcommand)]
enum LayoutCommands {
    #[command(about = "Apply a layout template", after_help = LAYOUT_HELP)]
    Apply {
        #[arg(value_name = "FILE", help = "Path to a layout TOML file")]
        file: PathBuf,
    },

    #[command(about = "Validate a layout template", after_help = LAYOUT_HELP)]
    Validate {
        #[arg(value_name = "FILE", help = "Path to a layout TOML file")]
        file: PathBuf,
    },
}

#[derive(Args)]
struct OpenArgs {
    #[arg(value_enum)]
    kind: OpenKind,

    #[arg(long, value_enum)]
    direction: Option<SplitDirectionArg>,

    #[arg(long)]
    target: Option<String>,

    #[arg(long)]
    cwd: Option<String>,

    #[arg(long)]
    command: Option<String>,

    #[arg(long)]
    input: Option<String>,

    #[arg(long)]
    wait: bool,

    #[arg(long = "env")]
    env: Vec<String>,

    #[arg(long)]
    label: Option<String>,
}

#[derive(Args)]
struct BroadcastArgs {
    #[arg(long = "target", required = true, num_args = 1..)]
    targets: Vec<String>,

    #[arg(long)]
    text: Option<String>,

    #[arg(long, num_args = 1..)]
    keys: Vec<String>,
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
enum OpenKind {
    Window,
    Tab,
    Split,
}

#[derive(Clone, Copy, Debug, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
enum SplitDirectionArg {
    Right,
    Left,
    Down,
    Up,
}

impl From<SplitDirectionArg> for applescript::SplitDirection {
    fn from(value: SplitDirectionArg) -> Self {
        match value {
            SplitDirectionArg::Right => applescript::SplitDirection::Right,
            SplitDirectionArg::Left => applescript::SplitDirection::Left,
            SplitDirectionArg::Down => applescript::SplitDirection::Down,
            SplitDirectionArg::Up => applescript::SplitDirection::Up,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayoutFile {
    name: Option<String>,
    root: LayoutNode,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
enum LayoutNode {
    Pane(LayoutPane),
    Split(LayoutSplit),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayoutPane {
    label: Option<String>,
    cwd: Option<String>,
    command: Option<String>,
    input: Option<String>,
    #[serde(default)]
    env: Vec<String>,
    #[serde(default)]
    focus: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayoutSplit {
    direction: SplitDirectionArg,
    left: Option<Box<LayoutNode>>,
    right: Option<Box<LayoutNode>>,
    top: Option<Box<LayoutNode>>,
    bottom: Option<Box<LayoutNode>>,
}

enum ParsedTarget {
    Focused,
    SelectedTab,
    FrontWindow,
    Named(String),
}

fn parse_target(target: &str) -> ParsedTarget {
    match target.to_ascii_lowercase().as_str() {
        "focused" => ParsedTarget::Focused,
        "selected-tab" => ParsedTarget::SelectedTab,
        "front-window" => ParsedTarget::FrontWindow,
        _ => ParsedTarget::Named(target.to_string()),
    }
}

fn resolve_label_or_uuid(target: &str) -> String {
    if !looks_like_uuid(target)
        && let Some(id) = labels::resolve(target)
    {
        return id;
    }
    target.to_string()
}

fn looks_like_uuid(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (i, b) in bytes.iter().enumerate() {
        let expect_hyphen = matches!(i, 8 | 13 | 18 | 23);
        if expect_hyphen {
            if *b != b'-' {
                return false;
            }
        } else if !b.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

fn resolve_terminal_target(target: &str) -> Result<String, String> {
    match parse_target(target) {
        ParsedTarget::Focused => applescript::focused_terminal_id()
            .ok_or_else(|| "Could not resolve the focused terminal".to_string()),
        ParsedTarget::SelectedTab => Err(
            "Target 'selected-tab' expands to multiple terminals; use broadcast, focus, or close"
                .to_string(),
        ),
        ParsedTarget::FrontWindow => Err(
            "Target 'front-window' expands to multiple terminals; use broadcast, focus, or close"
                .to_string(),
        ),
        ParsedTarget::Named(name) => Ok(resolve_label_or_uuid(&name)),
    }
}

fn expand_terminal_targets(targets: &[String]) -> Result<Vec<String>, String> {
    let known: HashSet<String> = applescript::list_terminals()
        .into_iter()
        .map(|t| t.id)
        .collect();
    let mut seen = HashSet::new();
    let mut resolved = Vec::new();

    for target in targets {
        let ids = match parse_target(target) {
            ParsedTarget::Focused => vec![
                applescript::focused_terminal_id()
                    .ok_or_else(|| "Could not resolve the focused terminal".to_string())?,
            ],
            ParsedTarget::SelectedTab => applescript::terminal_ids_in_selected_tab()?,
            ParsedTarget::FrontWindow => applescript::terminal_ids_in_front_window()?,
            ParsedTarget::Named(name) => {
                let id = resolve_label_or_uuid(&name);
                if !known.contains(&id) {
                    return Err(format!("Unknown terminal target '{}'", target));
                }
                vec![id]
            }
        };

        for id in ids {
            if seen.insert(id.clone()) {
                resolved.push(id);
            }
        }
    }

    if resolved.is_empty() {
        Err("No terminals matched the requested targets".to_string())
    } else {
        Ok(resolved)
    }
}

fn load_layout(file: &Path) -> Result<LayoutFile, String> {
    let raw = fs::read_to_string(file)
        .map_err(|e| format!("Failed to read layout file {}: {}", file.display(), e))?;
    toml::from_str(&raw)
        .map_err(|e| format!("Failed to parse layout file {}: {}", file.display(), e))
}

fn validate_layout(layout: &LayoutFile) -> Result<(), String> {
    if let Some(name) = &layout.name
        && name.trim().is_empty()
    {
        return Err("layout name cannot be empty".to_string());
    }

    let mut focus_count = 0;
    validate_layout_node(&layout.root, "root", &mut focus_count)?;

    if focus_count > 1 {
        return Err("layout may mark at most one pane with focus = true".to_string());
    }

    Ok(())
}

fn validate_layout_node(
    node: &LayoutNode,
    path: &str,
    focus_count: &mut usize,
) -> Result<(), String> {
    match node {
        LayoutNode::Pane(pane) => {
            if pane.command.is_some() && pane.input.is_some() {
                return Err(format!("{}: pane cannot set both command and input", path));
            }

            for entry in &pane.env {
                validate_env_assignment(entry).map_err(|e| format!("{}: {}", path, e))?;
            }

            if pane.focus {
                *focus_count += 1;
            }

            Ok(())
        }
        LayoutNode::Split(split) => {
            let (first_name, first, second_name, second) = split_children(split)?;

            if split.left.is_some()
                && !matches!(
                    split.direction,
                    SplitDirectionArg::Right | SplitDirectionArg::Left
                )
            {
                return Err(format!(
                    "{}: left child is only valid for left/right splits",
                    path
                ));
            }
            if split.right.is_some()
                && !matches!(
                    split.direction,
                    SplitDirectionArg::Right | SplitDirectionArg::Left
                )
            {
                return Err(format!(
                    "{}: right child is only valid for left/right splits",
                    path
                ));
            }
            if split.top.is_some()
                && !matches!(
                    split.direction,
                    SplitDirectionArg::Down | SplitDirectionArg::Up
                )
            {
                return Err(format!(
                    "{}: top child is only valid for up/down splits",
                    path
                ));
            }
            if split.bottom.is_some()
                && !matches!(
                    split.direction,
                    SplitDirectionArg::Down | SplitDirectionArg::Up
                )
            {
                return Err(format!(
                    "{}: bottom child is only valid for up/down splits",
                    path
                ));
            }

            validate_layout_node(first, &format!("{}.{}", path, first_name), focus_count)?;
            validate_layout_node(second, &format!("{}.{}", path, second_name), focus_count)?;
            Ok(())
        }
    }
}

fn split_children(split: &LayoutSplit) -> Result<(&str, &LayoutNode, &str, &LayoutNode), String> {
    match split.direction {
        SplitDirectionArg::Right => Ok((
            "left",
            split.left.as_deref().ok_or_else(|| {
                "split with direction = right must define [root.left]".to_string()
            })?,
            "right",
            split.right.as_deref().ok_or_else(|| {
                "split with direction = right must define [root.right]".to_string()
            })?,
        )),
        SplitDirectionArg::Left => Ok((
            "right",
            split.right.as_deref().ok_or_else(|| {
                "split with direction = left must define [root.right]".to_string()
            })?,
            "left",
            split
                .left
                .as_deref()
                .ok_or_else(|| "split with direction = left must define [root.left]".to_string())?,
        )),
        SplitDirectionArg::Down => Ok((
            "top",
            split
                .top
                .as_deref()
                .ok_or_else(|| "split with direction = down must define [root.top]".to_string())?,
            "bottom",
            split.bottom.as_deref().ok_or_else(|| {
                "split with direction = down must define [root.bottom]".to_string()
            })?,
        )),
        SplitDirectionArg::Up => {
            Ok((
                "bottom",
                split.bottom.as_deref().ok_or_else(|| {
                    "split with direction = up must define [root.bottom]".to_string()
                })?,
                "top",
                split.top.as_deref().ok_or_else(|| {
                    "split with direction = up must define [root.top]".to_string()
                })?,
            ))
        }
    }
}

fn empty_surface_config() -> applescript::SurfaceConfig {
    applescript::SurfaceConfig {
        cwd: None,
        command: None,
        input: None,
        wait_after_command: false,
        env: Vec::new(),
    }
}

fn apply_layout(layout: &LayoutFile) -> Result<String, String> {
    validate_layout(layout)?;

    let root_id = applescript::open_tab(&empty_surface_config())?;
    let mut focus_target = None;
    build_layout_node(&layout.root, &root_id, &mut focus_target)?;

    if let Some(target) = focus_target
        && !applescript::focus_terminal(&target)
    {
        return Err(format!("Failed to focus terminal {}", target));
    }

    Ok(root_id)
}

fn build_layout_node(
    node: &LayoutNode,
    terminal_id: &str,
    focus_target: &mut Option<String>,
) -> Result<(), String> {
    match node {
        LayoutNode::Pane(pane) => apply_layout_pane(pane, terminal_id, focus_target),
        LayoutNode::Split(split) => {
            let (current_name, current_node, new_name, new_node) =
                split_children(split).map_err(|e| e.replace("root", "layout root"))?;
            let new_terminal_id = applescript::open_split(
                terminal_id,
                split.direction.into(),
                &empty_surface_config(),
            )?;

            build_layout_node(current_node, terminal_id, focus_target)
                .map_err(|e| format!("{} subtree: {}", current_name, e))?;
            build_layout_node(new_node, &new_terminal_id, focus_target)
                .map_err(|e| format!("{} subtree: {}", new_name, e))?;
            Ok(())
        }
    }
}

fn apply_layout_pane(
    pane: &LayoutPane,
    terminal_id: &str,
    focus_target: &mut Option<String>,
) -> Result<(), String> {
    if let Some(label) = &pane.label {
        labels::set(label, terminal_id);
    }

    if let Some(command) = build_layout_shell_command(pane, terminal_id)? {
        if !applescript::input_text(terminal_id, &command) {
            return Err(format!(
                "Failed to type layout command into terminal {}",
                terminal_id
            ));
        }
        if !applescript::send_key(terminal_id, "enter") {
            return Err(format!("Failed to send Enter to terminal {}", terminal_id));
        }
    }

    if let Some(input) = &pane.input
        && !applescript::input_text(terminal_id, input)
    {
        return Err(format!(
            "Failed to type layout input into terminal {}",
            terminal_id
        ));
    }

    if pane.focus {
        *focus_target = Some(terminal_id.to_string());
    }

    Ok(())
}

fn build_layout_shell_command(
    pane: &LayoutPane,
    terminal_id: &str,
) -> Result<Option<String>, String> {
    let mut inner_parts = Vec::new();

    let mut bridge_exports = vec![format!(
        "GHOSTTY_BRIDGE_TERMINAL_ID={}",
        shell_quote(terminal_id)
    )];
    if let Some(label) = &pane.label {
        bridge_exports.push(format!("GHOSTTY_BRIDGE_LABEL={}", shell_quote(label)));
    }
    inner_parts.push(format!("export {}", bridge_exports.join(" ")));

    if let Some(cwd) = &pane.cwd {
        let cwd = resolve_cwd(cwd)?;
        inner_parts.push(format!("cd {}", shell_quote(&cwd)));
    }

    if !pane.env.is_empty() {
        let exports = pane
            .env
            .iter()
            .map(|entry| parse_env_assignment(entry))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|(key, value)| format!("{}={}", key, shell_quote(&value)))
            .collect::<Vec<_>>()
            .join(" ");
        inner_parts.push(format!("export {}", exports));
    }

    if let Some(command) = &pane.command {
        inner_parts.push(format!("exec {}", command));
    }

    let script = inner_parts.join(" && ");
    Ok(Some(format!("exec sh -c {}", shell_quote(&script))))
}

fn validate_env_assignment(entry: &str) -> Result<(), String> {
    let (key, _) = parse_env_assignment(entry)?;
    if key.is_empty() {
        return Err(format!("invalid env assignment '{}': empty key", entry));
    }
    if !is_valid_env_key(&key) {
        return Err(format!(
            "invalid env assignment '{}': key must match [A-Za-z_][A-Za-z0-9_]*",
            entry
        ));
    }
    Ok(())
}

fn parse_env_assignment(entry: &str) -> Result<(String, String), String> {
    let (key, value) = entry
        .split_once('=')
        .ok_or_else(|| format!("invalid env assignment '{}': expected KEY=VALUE", entry))?;
    Ok((key.to_string(), value.to_string()))
}

fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(ch) if ch == '_' || ch.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn expand_home(path: &str) -> String {
    if path == "~" {
        return dirs::home_dir()
            .map(|home| home.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
    }
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest).to_string_lossy().to_string();
    }
    path.to_string()
}

fn resolve_cwd(path: &str) -> Result<String, String> {
    let (anchor, rest) = if path == "." || path == "$PWD" {
        (Some(CwdAnchor::CurrentDir), "")
    } else if let Some(rest) = path.strip_prefix("./") {
        (Some(CwdAnchor::CurrentDir), rest)
    } else if let Some(rest) = path.strip_prefix("$PWD/") {
        (Some(CwdAnchor::CurrentDir), rest)
    } else {
        (None, "")
    };

    match anchor {
        Some(CwdAnchor::CurrentDir) => {
            let cwd = std::env::current_dir()
                .map_err(|e| format!("failed to read current directory: {}", e))?;
            let full = if rest.is_empty() { cwd } else { cwd.join(rest) };
            Ok(full.to_string_lossy().to_string())
        }
        None => Ok(expand_home(path)),
    }
}

enum CwdAnchor {
    CurrentDir,
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn exit_with_error<T>(message: impl AsRef<str>) -> T {
    eprintln!("{}", message.as_ref());
    process::exit(1);
}

fn ensure_ok(success: bool, message: impl AsRef<str>) {
    if !success {
        let _: () = exit_with_error(message);
    }
}

fn map_key(key: &str) -> String {
    match key.to_lowercase().as_str() {
        "enter" => "enter".to_string(),
        "return" => "enter".to_string(),
        "escape" | "esc" => "escape".to_string(),
        "tab" => "tab".to_string(),
        "space" => "space".to_string(),
        "backspace" | "bs" => "delete".to_string(),
        "delete" | "del" => "forward delete".to_string(),
        "up" => "up arrow".to_string(),
        "down" => "down arrow".to_string(),
        "left" => "left arrow".to_string(),
        "right" => "right arrow".to_string(),
        "home" => "home".to_string(),
        "end" => "end".to_string(),
        "pageup" | "page-up" => "page up".to_string(),
        "pagedown" | "page-down" => "page down".to_string(),
        s if s.starts_with("c-") => {
            let ch = s.strip_prefix("c-").unwrap_or(s);
            format!("{} control", ch)
        }
        s if s.starts_with("m-") || s.starts_with("a-") => {
            let ch = s
                .strip_prefix("m-")
                .or_else(|| s.strip_prefix("a-"))
                .unwrap_or(s);
            format!("{} option", ch)
        }
        s => s.to_string(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

const MESSAGE_PREFIX: &str = "[ghostty-bridge:";

#[derive(Debug, PartialEq, Eq)]
struct ParsedMessage {
    sender: String,
    recipient: String,
    body: String,
}

fn parse_bridge_message(line: &str) -> Option<ParsedMessage> {
    let rest = line.trim_start().strip_prefix(MESSAGE_PREFIX)?;
    let (header_rest, body) = rest.split_once("] ")?;
    let (sender, recipient) = header_rest.split_once(" >>> ")?;
    let sender = sender.trim();
    let recipient = recipient.trim();
    if sender.is_empty() || recipient.is_empty() {
        return None;
    }
    Some(ParsedMessage {
        sender: sender.to_string(),
        recipient: recipient.to_string(),
        body: body.to_string(),
    })
}

fn find_last_bridge_message(text: &str) -> Option<ParsedMessage> {
    text.lines().rev().find_map(parse_bridge_message)
}

fn slice_since_last_message(text: &str) -> &str {
    let mut cursor = 0;
    let mut last_end = None;
    for line in text.split_inclusive('\n') {
        let line_no_newline = line.strip_suffix('\n').unwrap_or(line);
        if parse_bridge_message(line_no_newline).is_some() {
            last_end = Some(cursor + line.len());
        }
        cursor += line.len();
    }
    match last_end {
        Some(end) if end <= text.len() => &text[end..],
        _ => text,
    }
}

fn format_bridge_message(sender: &str, recipient: &str, body: &str) -> String {
    format!("[ghostty-bridge:{} >>> {}] {}", sender, recipient, body)
}

fn ensure_single_line_body(body: &str) -> Result<(), String> {
    if body.contains('\n') || body.contains('\r') {
        Err("bridge message body must be a single line (no newlines)".to_string())
    } else {
        Ok(())
    }
}

#[derive(Serialize)]
struct ListEntry<'a> {
    id: &'a str,
    name: &'a str,
    cwd: &'a str,
    pid: Option<u32>,
    tty: Option<&'a str>,
    label: Option<&'a str>,
}

fn build_list_entries<'a>(
    terminals: &'a [applescript::TerminalInfo],
    labels: &'a HashMap<String, String>,
) -> Vec<ListEntry<'a>> {
    terminals
        .iter()
        .map(|t| ListEntry {
            id: &t.id,
            name: &t.name,
            cwd: &t.cwd,
            pid: t.pid,
            tty: t.tty.as_deref(),
            label: labels
                .iter()
                .find(|(_, id)| *id == &t.id)
                .map(|(l, _)| l.as_str()),
        })
        .collect()
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::List { json } => {
            let terminals = applescript::list_terminals();
            let store = labels::load();
            let entries = build_list_entries(&terminals, &store);
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&entries).unwrap_or_else(|e| {
                        exit_with_error(format!("Failed to serialize terminal list as JSON: {}", e))
                    })
                );
            } else {
                for entry in entries {
                    let label = entry.label.unwrap_or("-");
                    let tty = entry.tty.unwrap_or("-");
                    println!(
                        "{:<38} {:<30} {:<40} {:<14} {}",
                        entry.id,
                        truncate(entry.name, 30),
                        entry.cwd,
                        tty,
                        label
                    );
                }
            }
        }

        Commands::Type { target, text } => {
            let id = resolve_terminal_target(&target).unwrap_or_else(exit_with_error);
            ensure_ok(
                applescript::input_text(&id, &text),
                format!("Failed to type text into terminal {}", id),
            );
        }

        Commands::Message { target, text } => {
            ensure_single_line_body(&text).unwrap_or_else(exit_with_error);
            let id = resolve_terminal_target(&target).unwrap_or_else(exit_with_error);
            let sender = applescript::find_current_terminal_id().unwrap_or_else(|| {
                exit_with_error(
                    "Could not identify current Ghostty terminal; \
                     'message' must run inside Ghostty so replies can route back",
                )
            });
            let msg = format_bridge_message(&sender, &target, &text);
            ensure_ok(
                applescript::input_text(&id, &msg),
                format!("Failed to send message to terminal {}", id),
            );
            ensure_ok(
                applescript::send_key(&id, "enter"),
                format!("Failed to send Enter to terminal {}", id),
            );
        }

        Commands::Reply { text, lines } => {
            ensure_single_line_body(&text).unwrap_or_else(exit_with_error);
            let current_id = applescript::find_current_terminal_id()
                .unwrap_or_else(|| exit_with_error("Could not identify current Ghostty terminal"));
            let output = applescript::read_terminal(&current_id, lines).unwrap_or_else(|e| {
                exit_with_error(format!("Failed to read terminal {}: {}", current_id, e))
            });
            let message = find_last_bridge_message(&output).unwrap_or_else(|| {
                exit_with_error("No ghostty-bridge message found in current terminal")
            });
            let target_id =
                resolve_terminal_target(&message.sender).unwrap_or_else(exit_with_error);
            let reply = format_bridge_message(&current_id, &message.sender, &text);
            ensure_ok(
                applescript::input_text(&target_id, &reply),
                format!("Failed to send reply to terminal {}", target_id),
            );
            ensure_ok(
                applescript::send_key(&target_id, "enter"),
                format!("Failed to send Enter to terminal {}", target_id),
            );
        }

        Commands::Read {
            target,
            lines,
            since_last_message,
        } => {
            let id = resolve_terminal_target(&target).unwrap_or_else(exit_with_error);
            let output = applescript::read_terminal(&id, lines).unwrap_or_else(|e| {
                exit_with_error(format!("Failed to read terminal {}: {}", id, e))
            });
            if since_last_message {
                print!("{}", slice_since_last_message(&output));
            } else {
                print!("{}", output);
            }
        }

        Commands::Keys { target, keys } => {
            let id = resolve_terminal_target(&target).unwrap_or_else(exit_with_error);
            for key in &keys {
                let mapped = map_key(key);
                ensure_ok(
                    applescript::send_key(&id, &mapped),
                    format!("Failed to send key {} to terminal {}", key, id),
                );
            }
        }

        Commands::Name { target, label } => {
            let id = resolve_terminal_target(&target).unwrap_or_else(exit_with_error);
            labels::set(&label, &id);
        }

        Commands::Resolve { label } => match labels::resolve(&label) {
            Some(id) => println!("{}", id),
            None => exit_with_error(format!("No terminal labeled '{}'", label)),
        },

        Commands::Id => match applescript::find_current_terminal_id() {
            Some(id) => println!("{}", id),
            None => {
                eprintln!("Could not identify current Ghostty terminal");
                eprintln!(
                    "Hint: ensure you are running inside a Ghostty {}+ terminal",
                    applescript::MINIMUM_GHOSTTY_VERSION
                );
                process::exit(1);
            }
        },

        Commands::Doctor => {
            println!("ghostty-bridge doctor v0.1.0");
            println!("---");

            let ghostty_running = applescript::is_ghostty_running();
            println!(
                "Ghostty running:    {}",
                if ghostty_running { "yes" } else { "no" }
            );

            if !ghostty_running {
                println!("---");
                println!("Status: FAIL - Ghostty is not running");
                process::exit(1);
            }

            let version = applescript::get_version();
            println!(
                "Ghostty version:    {}",
                version.as_deref().unwrap_or("unknown")
            );

            let terminals = applescript::list_terminals();
            let pid_metadata = applescript::supports_pid_metadata(&terminals);
            println!(
                "PID metadata:       {}",
                if pid_metadata { "yes" } else { "no" }
            );

            if !applescript::supports_identity_detection(version.as_deref(), &terminals) {
                println!("---");
                println!(
                    "Status: FAIL - ghostty-bridge requires Ghostty {}+ or a build exposing Ghostty pid metadata",
                    applescript::MINIMUM_GHOSTTY_VERSION
                );
                process::exit(1);
            }

            if applescript::version_requires_capability_fallback(version.as_deref(), &terminals) {
                println!("Version gate:       fallback via pid metadata");
            }

            println!("Total terminals:    {}", terminals.len());

            let store = labels::load();
            let labeled = store.len();
            println!("Labeled terminals:  {}", labeled);

            let in_ghostty = std::env::var("TERM_PROGRAM").unwrap_or_default() == "ghostty";
            println!(
                "In Ghostty:         {}",
                if in_ghostty { "yes" } else { "no" }
            );

            if in_ghostty {
                match applescript::find_current_terminal_id() {
                    Some(id) => println!("Current terminal:   {}", id),
                    None => println!("Current terminal:   <detection failed>"),
                }
            }

            println!("---");
            println!("Status: OK");
        }

        Commands::Open(args) => {
            if args.kind != OpenKind::Split && args.direction.is_some() {
                let _: () = exit_with_error("--direction is only valid for 'open split'");
            }
            if args.kind != OpenKind::Split && args.target.is_some() {
                let _: () = exit_with_error("--target is only valid for 'open split'");
            }

            let user_command = args.command.clone();
            let config = applescript::SurfaceConfig {
                cwd: args.cwd,
                command: None,
                input: args.input,
                wait_after_command: args.wait,
                env: args.env,
            };

            let id = match args.kind {
                OpenKind::Window => applescript::open_window(&config),
                OpenKind::Tab => applescript::open_tab(&config),
                OpenKind::Split => {
                    let direction = args
                        .direction
                        .map(Into::into)
                        .unwrap_or(applescript::SplitDirection::Right);
                    let split_target = args.target.unwrap_or_else(|| "focused".to_string());
                    let target_id =
                        resolve_terminal_target(&split_target).unwrap_or_else(exit_with_error);
                    applescript::open_split(&target_id, direction, &config)
                }
            }
            .unwrap_or_else(exit_with_error);

            if let Some(label) = &args.label {
                labels::set(label, &id);
            }

            let mut exports = vec![format!("GHOSTTY_BRIDGE_TERMINAL_ID={}", shell_quote(&id))];
            if let Some(label) = &args.label {
                exports.push(format!("GHOSTTY_BRIDGE_LABEL={}", shell_quote(label)));
            }
            let mut inner = vec![format!("export {}", exports.join(" "))];
            if let Some(cmd) = user_command {
                inner.push(format!("exec {}", cmd));
            }
            let script = inner.join(" && ");
            let line = format!("exec sh -c {}", shell_quote(&script));
            ensure_ok(
                applescript::input_text(&id, &line),
                format!("Failed to type bootstrap into terminal {}", id),
            );
            ensure_ok(
                applescript::send_key(&id, "enter"),
                format!("Failed to send Enter to terminal {}", id),
            );

            println!("{}", id);
        }

        Commands::Exec { target, command } => {
            let id = resolve_terminal_target(&target).unwrap_or_else(exit_with_error);
            ensure_ok(
                applescript::input_text(&id, &command),
                format!("Failed to type command into terminal {}", id),
            );
            ensure_ok(
                applescript::send_key(&id, "enter"),
                format!("Failed to send Enter to terminal {}", id),
            );
        }

        Commands::Broadcast(args) => {
            let has_text = args.text.is_some();
            let has_keys = !args.keys.is_empty();
            if has_text == has_keys {
                let _: () =
                    exit_with_error("Provide exactly one of --text or --keys for broadcast");
            }

            let ids = expand_terminal_targets(&args.targets).unwrap_or_else(exit_with_error);

            if let Some(text) = args.text {
                for id in &ids {
                    ensure_ok(
                        applescript::input_text(id, &text),
                        format!("Failed to type text into terminal {}", id),
                    );
                    ensure_ok(
                        applescript::send_key(id, "enter"),
                        format!("Failed to send Enter to terminal {}", id),
                    );
                }
            } else {
                for id in &ids {
                    for key in &args.keys {
                        let mapped = map_key(key);
                        ensure_ok(
                            applescript::send_key(id, &mapped),
                            format!("Failed to send key {} to terminal {}", key, id),
                        );
                    }
                }
            }
        }

        Commands::Focus { target } => match parse_target(&target) {
            ParsedTarget::Focused | ParsedTarget::SelectedTab => {
                ensure_ok(
                    applescript::focus_focused_terminal(),
                    "Failed to focus the selected tab",
                );
            }
            ParsedTarget::FrontWindow => {
                ensure_ok(
                    applescript::activate_front_window(),
                    "Failed to activate the front window",
                );
            }
            ParsedTarget::Named(name) => {
                let id = resolve_label_or_uuid(&name);
                ensure_ok(
                    applescript::focus_terminal(&id),
                    format!("Failed to focus terminal {}", id),
                );
            }
        },

        Commands::Close { target } => match parse_target(&target) {
            ParsedTarget::Focused => {
                let id = applescript::focused_terminal_id()
                    .unwrap_or_else(|| exit_with_error("Could not resolve the focused terminal"));
                ensure_ok(
                    applescript::close_terminal(&id),
                    format!("Failed to close terminal {}", id),
                );
            }
            ParsedTarget::SelectedTab => {
                ensure_ok(
                    applescript::close_selected_tab(),
                    "Failed to close the selected tab",
                );
            }
            ParsedTarget::FrontWindow => {
                ensure_ok(
                    applescript::close_front_window(),
                    "Failed to close the front window",
                );
            }
            ParsedTarget::Named(name) => {
                let id = resolve_label_or_uuid(&name);
                ensure_ok(
                    applescript::close_terminal(&id),
                    format!("Failed to close terminal {}", id),
                );
            }
        },

        Commands::Layout { command } => match command {
            LayoutCommands::Apply { file } => {
                let layout = load_layout(&file).unwrap_or_else(exit_with_error);
                validate_layout(&layout).unwrap_or_else(exit_with_error);
                let root_id = apply_layout(&layout).unwrap_or_else(exit_with_error);
                println!("{}", root_id);
            }
            LayoutCommands::Validate { file } => {
                let layout = load_layout(&file).unwrap_or_else(exit_with_error);
                validate_layout(&layout).unwrap_or_else(exit_with_error);
                println!("OK");
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_well_formed_message() {
        let parsed =
            parse_bridge_message("[ghostty-bridge:AAA >>> BBB] hello world").expect("should parse");
        assert_eq!(parsed.sender, "AAA");
        assert_eq!(parsed.recipient, "BBB");
        assert_eq!(parsed.body, "hello world");
    }

    #[test]
    fn parser_preserves_inner_brackets_and_arrows() {
        let parsed =
            parse_bridge_message("[ghostty-bridge:claude >>> codex] reply [1]: a >>> b").unwrap();
        assert_eq!(parsed.sender, "claude");
        assert_eq!(parsed.recipient, "codex");
        assert_eq!(parsed.body, "reply [1]: a >>> b");
    }

    #[test]
    fn parser_rejects_unrelated_lines() {
        assert!(parse_bridge_message("$ echo hi").is_none());
        assert!(parse_bridge_message("[ghostty-bridge-x: a >>> b] y").is_none());
        assert!(parse_bridge_message("[ghostty-bridge: >>> codex] missing sender").is_none());
        assert!(parse_bridge_message("[ghostty-bridge:claude] missing recipient").is_none());
    }

    #[test]
    fn find_last_bridge_message_picks_the_most_recent() {
        let transcript = "\
boot log
[ghostty-bridge:claude >>> codex] first
random output
[ghostty-bridge:gemini >>> claude] second
tail
";
        let parsed = find_last_bridge_message(transcript).unwrap();
        assert_eq!(parsed.sender, "gemini");
        assert_eq!(parsed.recipient, "claude");
        assert_eq!(parsed.body, "second");
    }

    #[test]
    fn find_last_returns_none_when_absent() {
        let transcript = "boot\nother output\n";
        assert!(find_last_bridge_message(transcript).is_none());
    }

    #[test]
    fn slice_since_last_message_returns_trailing_output() {
        let transcript = "\
[ghostty-bridge:claude >>> codex] hi
running command
done
";
        assert_eq!(
            slice_since_last_message(transcript),
            "running command\ndone\n"
        );
    }

    #[test]
    fn slice_since_last_message_returns_empty_when_message_is_last_line() {
        let transcript = "prelude\n[ghostty-bridge:claude >>> codex] ping\n";
        assert_eq!(slice_since_last_message(transcript), "");
    }

    #[test]
    fn slice_since_last_message_returns_full_when_no_message() {
        let transcript = "just terminal output\nwith no framing\n";
        assert_eq!(slice_since_last_message(transcript), transcript);
    }

    #[test]
    fn format_bridge_message_roundtrips_through_parser() {
        let line = format_bridge_message("claude", "codex", "please review auth.ts");
        let parsed = parse_bridge_message(&line).unwrap();
        assert_eq!(parsed.sender, "claude");
        assert_eq!(parsed.recipient, "codex");
        assert_eq!(parsed.body, "please review auth.ts");
    }

    #[test]
    fn list_entries_attach_labels_by_id() {
        let terminals = vec![
            applescript::TerminalInfo {
                id: "ID-1".to_string(),
                name: "claude".to_string(),
                cwd: "/a".to_string(),
                pid: Some(1234),
                tty: Some("/dev/ttys001".to_string()),
            },
            applescript::TerminalInfo {
                id: "ID-2".to_string(),
                name: "codex".to_string(),
                cwd: "/b".to_string(),
                pid: None,
                tty: None,
            },
        ];
        let mut labels: HashMap<String, String> = HashMap::new();
        labels.insert("claude".to_string(), "ID-1".to_string());

        let entries = build_list_entries(&terminals, &labels);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, "ID-1");
        assert_eq!(entries[0].label, Some("claude"));
        assert_eq!(entries[0].pid, Some(1234));
        assert_eq!(entries[0].tty, Some("/dev/ttys001"));
        assert_eq!(entries[1].id, "ID-2");
        assert_eq!(entries[1].label, None);
        assert_eq!(entries[1].pid, None);
        assert_eq!(entries[1].tty, None);
    }

    #[test]
    fn list_entries_serialize_to_stable_json_shape() {
        let terminals = vec![applescript::TerminalInfo {
            id: "ID-1".to_string(),
            name: "claude".to_string(),
            cwd: "/a".to_string(),
            pid: Some(42),
            tty: Some("/dev/ttys009".to_string()),
        }];
        let mut labels: HashMap<String, String> = HashMap::new();
        labels.insert("claude".to_string(), "ID-1".to_string());

        let entries = build_list_entries(&terminals, &labels);
        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&entries).unwrap()).unwrap();
        let arr = value.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["id"], "ID-1");
        assert_eq!(arr[0]["name"], "claude");
        assert_eq!(arr[0]["cwd"], "/a");
        assert_eq!(arr[0]["pid"], 42);
        assert_eq!(arr[0]["tty"], "/dev/ttys009");
        assert_eq!(arr[0]["label"], "claude");
    }

    #[test]
    fn looks_like_uuid_requires_canonical_shape() {
        assert!(looks_like_uuid("123e4567-e89b-12d3-a456-426614174000"));
        assert!(!looks_like_uuid("codex-e2e-1740000000000"));
        assert!(!looks_like_uuid("123e4567e89b12d3a456426614174000"));
    }

    #[test]
    fn bundled_layout_template_validates() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("layouts/ai-trio.toml");
        let layout = load_layout(&path).expect("layout should parse");
        validate_layout(&layout).expect("layout should validate");
    }
}
