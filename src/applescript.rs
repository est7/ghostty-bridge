use std::process::Command;

#[derive(Debug, Clone)]
pub struct TerminalInfo {
    pub id: String,
    pub name: String,
    pub cwd: String,
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
        set output to output & tid & "|||" & tn & "|||" & twd & linefeed
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
            terminals.push(TerminalInfo {
                id: parts[0].to_string(),
                name: parts[1].to_string(),
                cwd: parts[2].to_string(),
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

    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!(
        r#"tell application "Ghostty"
    set allTerms to terminals
    set t to item {} of allTerms
    input text "{}" to t
end tell"#,
        idx, escaped
    );
    osascript(&script).is_ok()
}

pub fn send_key(id: &str, key: &str) -> bool {
    let idx = match find_terminal_index(id) {
        Some(i) => i + 1,
        None => return false,
    };

    let escaped = key.replace('"', "\\\"");
    let script = format!(
        r#"tell application "Ghostty"
    set allTerms to terminals
    set t to item {} of allTerms
    send key "{}" to t
end tell"#,
        idx, escaped
    );
    osascript(&script).is_ok()
}

fn perform_action(id: &str, action: &str) -> bool {
    let idx = match find_terminal_index(id) {
        Some(i) => i + 1,
        None => return false,
    };

    let escaped = action.replace('"', "\\\"");
    let script = format!(
        r#"tell application "Ghostty"
    set allTerms to terminals
    set t to item {} of allTerms
    perform action "{}" on t
end tell"#,
        idx, escaped
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
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
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

    if !matches.is_empty() {
        if let Some(tty) = current_tty() {
            for t in &matches {
                if let Some(child_tty) = get_terminal_tty_for_cwd(&t.cwd) {
                    if tty == child_tty {
                        return Some(t.id.clone());
                    }
                }
            }
        }

        if let Some(_shell_pid) = find_shell_pid_for_cwd(cwd_str) {
            return Some(matches[0].id.clone());
        }
    }

    None
}

fn current_tty() -> Option<String> {
    let output = Command::new("tty").output().ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

fn get_terminal_tty_for_cwd(_cwd: &str) -> Option<String> {
    None
}

fn find_shell_pid_for_cwd(_cwd: &str) -> Option<u32> {
    None
}
