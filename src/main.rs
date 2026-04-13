mod applescript;
mod labels;

use clap::{Parser, Subcommand};
use std::process;

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
    List,

    #[command(about = "Type text into a terminal without pressing Enter")]
    Type { target: String, text: String },

    #[command(about = "Type text with auto-prepended sender info and reply target")]
    Message { target: String, text: String },

    #[command(about = "Read last N lines from terminal output")]
    Read {
        target: String,
        #[arg(default_value = "50")]
        lines: usize,
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
}

fn resolve_target(target: &str) -> String {
    let lowered = target.to_lowercase();
    let looks_like_uuid = lowered.contains('-') && lowered.len() >= 20;
    if !looks_like_uuid {
        if let Some(id) = labels::resolve(target) {
            return id;
        }
    }
    target.to_string()
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::List => {
            let terminals = applescript::list_terminals();
            let store = labels::load();
            for t in &terminals {
                let label = store
                    .iter()
                    .find(|(_, id)| *id == &t.id)
                    .map(|(l, _)| l.as_str())
                    .unwrap_or("-");
                println!(
                    "{:<38} {:<30} {:<40} {}",
                    t.id,
                    truncate(&t.name, 30),
                    t.cwd,
                    label
                );
            }
        }

        Commands::Type { target, text } => {
            let id = resolve_target(&target);
            if !applescript::input_text(&id, &text) {
                eprintln!("Failed to type text into terminal {}", id);
                process::exit(1);
            }
        }

        Commands::Message { target, text } => {
            let id = resolve_target(&target);
            let sender =
                applescript::find_current_terminal_id().unwrap_or_else(|| "unknown".to_string());
            let msg = format!("[ghostty-bridge:{} >>> {}] {}", sender, target, text);
            if !applescript::input_text(&id, &msg) {
                eprintln!("Failed to send message to terminal {}", id);
                process::exit(1);
            }
            applescript::send_key(&id, "enter");
        }

        Commands::Read { target, lines } => {
            let id = resolve_target(&target);
            let output = match applescript::read_terminal(&id, lines) {
                Ok(text) => text,
                Err(e) => {
                    eprintln!("Failed to read terminal {}: {}", id, e);
                    process::exit(1);
                }
            };
            print!("{}", output);
        }

        Commands::Keys { target, keys } => {
            let id = resolve_target(&target);
            for key in &keys {
                let mapped = map_key(key);
                if !applescript::send_key(&id, &mapped) {
                    eprintln!("Failed to send key {} to terminal {}", key, id);
                    process::exit(1);
                }
            }
        }

        Commands::Name { target, label } => {
            let id = resolve_target(&target);
            labels::set(&label, &id);
        }

        Commands::Resolve { label } => match labels::resolve(&label) {
            Some(id) => println!("{}", id),
            None => {
                eprintln!("No terminal labeled '{}'", label);
                process::exit(1);
            }
        },

        Commands::Id => match applescript::find_current_terminal_id() {
            Some(id) => println!("{}", id),
            None => {
                eprintln!("Could not identify current Ghostty terminal");
                eprintln!("Hint: ensure you are running inside a Ghostty terminal");
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
                version.unwrap_or_else(|| "unknown".into())
            );

            let terminals = applescript::list_terminals();
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
