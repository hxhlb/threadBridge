use std::env;
use std::fs;
use std::io::{self, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use serde::Deserialize;

#[derive(Debug)]
struct Args {
    data_root: PathBuf,
    workspace: PathBuf,
    thread_key: String,
    session_id: String,
    since: String,
}

#[derive(Debug, Deserialize)]
struct MirrorEntry {
    timestamp: String,
    origin: String,
    role: String,
    text: String,
}

fn parse_args() -> Result<Args> {
    let mut data_root = None;
    let mut workspace = None;
    let mut thread_key = None;
    let mut session_id = None;
    let mut since = None;

    let mut iter = env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--repo-root" => {
                let _ = iter.next().context("missing value for --repo-root")?;
            }
            "--data-root" => {
                data_root = Some(PathBuf::from(
                    iter.next().context("missing value for --data-root")?,
                ));
            }
            "--workspace" => {
                workspace = Some(PathBuf::from(
                    iter.next().context("missing value for --workspace")?,
                ));
            }
            "--thread-key" => {
                thread_key = Some(iter.next().context("missing value for --thread-key")?);
            }
            "--session-id" => {
                session_id = Some(iter.next().context("missing value for --session-id")?);
            }
            "--since" => {
                since = Some(iter.next().context("missing value for --since")?);
            }
            other => bail!("unknown argument: {other}"),
        }
    }

    Ok(Args {
        data_root: data_root.context("--data-root is required")?,
        workspace: workspace.context("--workspace is required")?,
        thread_key: thread_key.context("--thread-key is required")?,
        session_id: session_id.context("--session-id is required")?,
        since: since.context("--since is required")?,
    })
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn print_header(args: &Args) -> Result<()> {
    print!("\x1b[2J\x1b[H");
    println!("threadBridge viewer");
    println!("workspace: {}", args.workspace.display());
    println!("thread_key: {}", args.thread_key);
    println!("session_id: {}", args.session_id);
    println!();
    println!("read-only mirror from .attach handoff");
    println!("press r to resume local Codex CLI");
    println!();
    io::stdout().flush()?;
    Ok(())
}

fn render_entry(entry: &MirrorEntry) -> Option<String> {
    let prefix = match (entry.origin.as_str(), entry.role.as_str()) {
        ("telegram", "user") => "Telegram",
        ("telegram", "assistant") => "Codex",
        ("cli", "user") => "CLI",
        ("cli", "assistant") => "Codex",
        _ => return None,
    };
    Some(format!("{prefix}: {}", entry.text.trim()))
}

fn mirror_path(args: &Args) -> PathBuf {
    args.data_root
        .join(&args.thread_key)
        .join("state")
        .join("transcript-mirror.jsonl")
}

fn resume_command(args: &Args) -> Command {
    let snippet = args.workspace.join(".threadbridge/shell/codex-sync.bash");
    let shell_command = format!(
        "source {} && hcodex resume {} --thread-key {}",
        shell_single_quote(&snippet.display().to_string()),
        shell_single_quote(&args.session_id),
        shell_single_quote(&args.thread_key),
    );
    let mut cmd = Command::new("/bin/zsh");
    cmd.arg("-lc").arg(shell_command);
    cmd
}

fn main() -> Result<()> {
    let args = parse_args()?;
    print_header(&args)?;
    enable_raw_mode().context("failed to enable terminal raw mode")?;

    let path = mirror_path(&args);
    let mut seen_lines = 0usize;

    loop {
        if let Ok(content) = fs::read_to_string(&path) {
            let lines = content.lines().collect::<Vec<_>>();
            for line in lines.iter().skip(seen_lines) {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let entry: MirrorEntry = match serde_json::from_str(trimmed) {
                    Ok(entry) => entry,
                    Err(_) => continue,
                };
                if entry.timestamp < args.since {
                    continue;
                }
                if let Some(rendered) = render_entry(&entry) {
                    println!("{rendered}");
                }
            }
            if lines.len() > seen_lines {
                io::stdout().flush()?;
            }
            seen_lines = lines.len();
        }

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()?
                && key.code == KeyCode::Char('r')
            {
                disable_raw_mode().ok();
                println!();
                println!("Resuming local Codex CLI...");
                io::stdout().flush().ok();
                let error = resume_command(&args).exec();
                return Err(error).context("failed to exec hcodex resume");
            }
        }
    }
}
