use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

pub const MINIMUM_GHOSTTY_VERSION: &str = "1.4.0";
const OSASCRIPT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct TerminalInfo {
    pub id: String,
    pub name: String,
    pub cwd: String,
    pub pid: Option<u32>,
    pub tty: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SurfaceConfig {
    pub cwd: Option<String>,
    pub command: Option<String>,
    pub input: Option<String>,
    pub wait_after_command: bool,
    pub env: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum SplitDirection {
    Right,
    Left,
    Down,
    Up,
}

impl SplitDirection {
    fn as_applescript(self) -> &'static str {
        match self {
            SplitDirection::Right => "right",
            SplitDirection::Left => "left",
            SplitDirection::Down => "down",
            SplitDirection::Up => "up",
        }
    }
}

fn osascript(script: &str) -> Result<String, String> {
    let mut child = Command::new("osascript")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run osascript: {}", e))?;

    {
        use std::io::Write;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Failed to open osascript stdin".to_string())?;
        stdin
            .write_all(script.as_bytes())
            .map_err(|e| format!("Failed to write osascript stdin: {}", e))?;
    }

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                let mut stderr = String::new();

                if let Some(mut out) = child.stdout.take() {
                    use std::io::Read;
                    let _ = out.read_to_string(&mut stdout);
                }
                if let Some(mut err) = child.stderr.take() {
                    use std::io::Read;
                    let _ = err.read_to_string(&mut stderr);
                }

                if status.success() {
                    return Ok(stdout.trim().to_string());
                }
                return Err(stderr.trim().to_string());
            }
            Ok(None) if start.elapsed() < OSASCRIPT_TIMEOUT => {
                thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "osascript timed out after {}s",
                    OSASCRIPT_TIMEOUT.as_secs()
                ));
            }
            Err(e) => return Err(format!("Failed to wait for osascript: {}", e)),
        }
    }
}

pub fn supports_minimum_version(version: &str) -> bool {
    parse_version_prefix(version)
        .map(|(major, minor)| (major, minor) >= (1, 4))
        .unwrap_or(false)
}

pub fn supports_pid_metadata(terminals: &[TerminalInfo]) -> bool {
    terminals.iter().any(|terminal| terminal.pid.is_some())
}

pub fn supports_identity_detection(version: Option<&str>, terminals: &[TerminalInfo]) -> bool {
    version.is_some_and(supports_minimum_version) || supports_pid_metadata(terminals)
}

pub fn version_requires_capability_fallback(
    version: Option<&str>,
    terminals: &[TerminalInfo],
) -> bool {
    match version {
        Some(version) => !supports_minimum_version(version) && supports_pid_metadata(terminals),
        None => supports_pid_metadata(terminals),
    }
}

fn parse_version_prefix(version: &str) -> Option<(u64, u64)> {
    let mut parts = version.split('.');
    let major = parse_version_component(parts.next()?)?;
    let minor = parse_version_component(parts.next()?)?;
    Some((major, minor))
}

fn parse_version_component(component: &str) -> Option<u64> {
    let digits = component
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn applescript_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn quoted_lines(values: &[String]) -> String {
    if values.is_empty() {
        "{}".to_string()
    } else {
        let joined = values
            .iter()
            .map(|v| applescript_string(v))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{{{}}}", joined)
    }
}

fn config_block(config: &SurfaceConfig) -> String {
    let mut lines = vec!["set cfg to new surface configuration".to_string()];

    if let Some(cwd) = &config.cwd {
        lines.push(format!(
            "set initial working directory of cfg to {}",
            applescript_string(cwd)
        ));
    }
    if let Some(command) = &config.command {
        lines.push(format!(
            "set command of cfg to {}",
            applescript_string(command)
        ));
    }
    if let Some(input) = &config.input {
        lines.push(format!(
            "set initial input of cfg to {}",
            applescript_string(input)
        ));
    }
    if config.wait_after_command {
        lines.push("set wait after command of cfg to true".to_string());
    }
    if !config.env.is_empty() {
        lines.push(format!(
            "set environment variables of cfg to {}",
            quoted_lines(&config.env)
        ));
    }

    lines.join("\n    ")
}

pub fn is_ghostty_running() -> bool {
    osascript("tell application \"System Events\" to exists process \"Ghostty\"").is_ok()
        && osascript("tell application \"Ghostty\" to get version").is_ok()
}

pub fn get_version() -> Option<String> {
    osascript("tell application \"Ghostty\" to get version").ok()
}

pub fn list_terminals() -> Vec<TerminalInfo> {
    let script = r#"
tell application "Ghostty"
    set output to ""
    set allTerms to terminals
    set termCount to count of allTerms
    repeat with i from 1 to termCount
        set t to item i of allTerms
        set tid to id of t
        set tn to name of t
        set twd to working directory of t
        set tpid to ""
        set ttty to ""
        try
            set tpid to (pid of t) as text
        end try
        try
            set ttty to tty of t
        end try
        set output to output & tid & "|||" & tn & "|||" & twd & "|||" & tpid & "|||" & ttty & linefeed
    end repeat
    return output
end tell
"#;
    let raw = match osascript(script) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let mut terminals = Vec::new();
    for line in raw.lines() {
        let parts: Vec<&str> = line.split("|||").collect();
        if parts.len() >= 3 {
            let pid = parts.get(3).and_then(|s| {
                if s.is_empty() {
                    None
                } else {
                    s.parse::<u32>().ok()
                }
            });
            let tty = parts.get(4).and_then(|s| {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            });
            terminals.push(TerminalInfo {
                id: parts[0].to_string(),
                name: parts[1].to_string(),
                cwd: parts[2].to_string(),
                pid,
                tty,
            });
        }
    }
    terminals
}

fn find_terminal_index(id: &str) -> Option<usize> {
    let terminals = list_terminals();
    terminals.iter().position(|t| t.id == id)
}

pub fn input_text(id: &str, text: &str) -> bool {
    let idx = match find_terminal_index(id) {
        Some(i) => i + 1,
        None => return false,
    };

    let script = format!(
        r#"tell application "Ghostty"
    set allTerms to terminals
    set t to item {} of allTerms
    input text {} to t
end tell"#,
        idx,
        applescript_string(text)
    );
    osascript(&script).is_ok()
}

pub fn send_key(id: &str, key: &str) -> bool {
    let idx = match find_terminal_index(id) {
        Some(i) => i + 1,
        None => return false,
    };

    let script = format!(
        r#"tell application "Ghostty"
    set allTerms to terminals
    set t to item {} of allTerms
    send key {} to t
end tell"#,
        idx,
        applescript_string(key)
    );
    osascript(&script).is_ok()
}

pub fn perform_action(id: &str, action: &str) -> bool {
    let idx = match find_terminal_index(id) {
        Some(i) => i + 1,
        None => return false,
    };

    let script = format!(
        r#"tell application "Ghostty"
    set allTerms to terminals
    set t to item {} of allTerms
    perform action {} on t
end tell"#,
        idx,
        applescript_string(action)
    );
    osascript(&script).is_ok()
}

pub fn read_terminal(id: &str, lines: usize) -> Result<String, String> {
    let saved_clipboard = save_clipboard();

    if !perform_action(id, "select_all") {
        return Err("Failed to select all text in terminal".into());
    }
    if !perform_action(id, "copy_to_clipboard") {
        let _ = perform_action(id, "select_none");
        return Err("Failed to copy text from terminal".into());
    }

    let _ = perform_action(id, "select_none");

    let content = read_clipboard();

    restore_clipboard(saved_clipboard.as_deref());

    let content = match content {
        Some(c) => c,
        None => return Err("Clipboard was empty after copy".into()),
    };

    let all_lines: Vec<&str> = content.lines().collect();
    let start = if all_lines.len() > lines {
        all_lines.len() - lines
    } else {
        0
    };
    let result: Vec<&str> = all_lines[start..].to_vec();

    Ok(result.join("\n"))
}

fn save_clipboard() -> Option<String> {
    let output = Command::new("pbpaste").output().ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        None
    }
}

fn read_clipboard() -> Option<String> {
    let output = Command::new("pbpaste").output().ok()?;
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        if text.is_empty() { None } else { Some(text) }
    } else {
        None
    }
}

fn restore_clipboard(content: Option<&str>) {
    if let Some(content) = content {
        use std::io::Write;
        if let Ok(mut child) = Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(content.as_bytes());
            }
            let _ = child.wait();
        }
    }
}

pub fn find_current_terminal_id() -> Option<String> {
    if std::env::var("TERM_PROGRAM").unwrap_or_default() != "ghostty" {
        return None;
    }

    if let Ok(id) = std::env::var("GHOSTTY_BRIDGE_TERMINAL_ID")
        && !id.is_empty()
    {
        return Some(id);
    }

    let terminals = list_terminals();
    let ancestors = ancestor_pids();

    for pid in &ancestors {
        if let Some(t) = terminals.iter().find(|t| t.pid == Some(*pid)) {
            return Some(t.id.clone());
        }
    }

    None
}

pub fn focused_terminal_id() -> Option<String> {
    let script = r#"
tell application "Ghostty"
    return id of focused terminal of selected tab of front window
end tell
"#;
    osascript(script).ok().filter(|s| !s.is_empty())
}

pub fn terminal_ids_in_selected_tab() -> Result<Vec<String>, String> {
    let script = r#"
tell application "Ghostty"
    set output to ""
    set allTerms to terminals of selected tab of front window
    repeat with t in allTerms
        set output to output & (id of t) & linefeed
    end repeat
    return output
end tell
"#;
    let raw = osascript(script)?;
    let ids = raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if ids.is_empty() {
        Err("No terminals found in the selected tab".to_string())
    } else {
        Ok(ids)
    }
}

pub fn terminal_ids_in_front_window() -> Result<Vec<String>, String> {
    let script = r#"
tell application "Ghostty"
    set output to ""
    set allTerms to terminals of front window
    repeat with t in allTerms
        set output to output & (id of t) & linefeed
    end repeat
    return output
end tell
"#;
    let raw = osascript(script)?;
    let ids = raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if ids.is_empty() {
        Err("No terminals found in the front window".to_string())
    } else {
        Ok(ids)
    }
}

pub fn open_window(config: &SurfaceConfig) -> Result<String, String> {
    let config_block = config_block(config);
    let script = format!(
        r#"tell application "Ghostty"
    {}
    set win to new window with configuration cfg
    return id of focused terminal of selected tab of win
end tell"#,
        config_block
    );
    osascript(&script)
}

pub fn open_tab(config: &SurfaceConfig) -> Result<String, String> {
    let config_block = config_block(config);
    let script = format!(
        r#"tell application "Ghostty"
    {}
    set win to front window
    set newTab to new tab in win with configuration cfg
    return id of focused terminal of newTab
end tell"#,
        config_block
    );
    osascript(&script)
}

pub fn open_split(
    target_id: &str,
    direction: SplitDirection,
    config: &SurfaceConfig,
) -> Result<String, String> {
    let idx = find_terminal_index(target_id)
        .map(|i| i + 1)
        .ok_or_else(|| format!("Could not find terminal {}", target_id))?;
    let config_block = config_block(config);
    let script = format!(
        r#"tell application "Ghostty"
    set allTerms to terminals
    set baseTerm to item {} of allTerms
    {}
    set newTerm to split baseTerm direction {} with configuration cfg
    return id of newTerm
end tell"#,
        idx,
        config_block,
        direction.as_applescript()
    );
    osascript(&script)
}

pub fn focus_terminal(id: &str) -> bool {
    let idx = match find_terminal_index(id) {
        Some(i) => i + 1,
        None => return false,
    };
    let script = format!(
        r#"tell application "Ghostty"
    set allTerms to terminals
    focus item {} of allTerms
end tell"#,
        idx
    );
    osascript(&script).is_ok()
}

pub fn focus_focused_terminal() -> bool {
    let script = r#"
tell application "Ghostty"
    focus focused terminal of selected tab of front window
end tell
"#;
    osascript(script).is_ok()
}

pub fn activate_front_window() -> bool {
    let script = r#"
tell application "Ghostty"
    activate window (front window)
end tell
"#;
    osascript(script).is_ok()
}

pub fn close_terminal(id: &str) -> bool {
    let idx = match find_terminal_index(id) {
        Some(i) => i + 1,
        None => return false,
    };
    let script = format!(
        r#"tell application "Ghostty"
    set allTerms to terminals
    close item {} of allTerms
end tell"#,
        idx
    );
    osascript(&script).is_ok()
}

pub fn close_selected_tab() -> bool {
    let script = r#"
tell application "Ghostty"
    close tab (selected tab of front window)
end tell
"#;
    osascript(script).is_ok()
}

pub fn close_front_window() -> bool {
    let script = r#"
tell application "Ghostty"
    close window (front window)
end tell
"#;
    osascript(script).is_ok()
}

fn ancestor_pids() -> Vec<u32> {
    let mut out = Vec::new();
    let mut pid = std::process::id();
    for _ in 0..32 {
        out.push(pid);
        match parent_pid(pid) {
            Some(parent) if parent != 0 && parent != 1 && parent != pid => pid = parent,
            _ => break,
        }
    }
    out
}

fn parent_pid(pid: u32) -> Option<u32> {
    let output = Command::new("ps")
        .args(["-o", "ppid=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u32>()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_supported_version_prefixes() {
        assert!(supports_minimum_version("1.4.0"));
        assert!(supports_minimum_version("1.4.2-dev"));
        assert!(supports_minimum_version("1.5"));
        assert!(supports_minimum_version("2.0.0"));
    }

    #[test]
    fn rejects_unsupported_or_invalid_versions() {
        assert!(!supports_minimum_version("1.3.9"));
        assert!(!supports_minimum_version("0.9.0"));
        assert!(!supports_minimum_version("dev-build"));
    }

    #[test]
    fn pid_metadata_can_satisfy_identity_detection() {
        let terminals = vec![TerminalInfo {
            id: "ID-1".to_string(),
            name: "term".to_string(),
            cwd: "/tmp".to_string(),
            pid: Some(42),
            tty: Some("/dev/ttys001".to_string()),
        }];

        assert!(supports_identity_detection(Some("afdae7293"), &terminals));
        assert!(version_requires_capability_fallback(
            Some("afdae7293"),
            &terminals
        ));
        assert!(supports_identity_detection(None, &terminals));
    }
}
