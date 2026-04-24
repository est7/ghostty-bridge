use std::process::Command;

#[derive(Debug, Clone)]
pub struct TerminalInfo {
    pub id: String,
    pub name: String,
    pub cwd: String,
    pub tty: String,
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
    let output = Command::new("osascript")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(script.as_bytes())?;
            }
            child.wait_with_output()
        })
        .map_err(|e| format!("Failed to run osascript: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(stderr)
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
        set ttty to tty of t
        set output to output & tid & "|||" & tn & "|||" & twd & "|||" & ttty & linefeed
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
        if parts.len() >= 4 {
            terminals.push(TerminalInfo {
                id: parts[0].to_string(),
                name: parts[1].to_string(),
                cwd: parts[2].to_string(),
                tty: parts[3].to_string(),
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

    let cwd = std::env::current_dir().ok()?;
    let cwd_str = cwd.to_str()?;

    let terminals = list_terminals();

    let matches: Vec<&TerminalInfo> = terminals.iter().filter(|t| t.cwd == cwd_str).collect();

    if matches.len() == 1 {
        return Some(matches[0].id.clone());
    }

    if !matches.is_empty()
        && let Some(tty) = current_tty()
    {
        for t in &matches {
            if t.tty == tty {
                return Some(t.id.clone());
            }
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

fn current_tty() -> Option<String> {
    let mut pid = std::process::id();
    loop {
        let output = Command::new("ps")
            .args(["-o", "ppid=,tty=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let line = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let mut parts = line.split_whitespace();
        let ppid: u32 = parts.next()?.parse().ok()?;
        let tty = parts.next().unwrap_or("??");
        if tty != "??" {
            return Some(format!("/dev/{}", tty));
        }
        if ppid <= 1 {
            return None;
        }
        pid = ppid;
    }
}
