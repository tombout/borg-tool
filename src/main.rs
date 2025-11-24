use std::{
    env, fs,
    io::{self, Write},
    path::PathBuf,
    process::Command,
};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use rpassword::prompt_password;
use serde::Deserialize;

/// CLI entry point.
#[derive(Parser, Debug)]
#[command(
    name = "borg-tool",
    version,
    about = "Minimal helper to inspect a Borg repository"
)]
struct Cli {
    /// Path to the config file
    #[arg(short, long)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug, Clone)]
enum Commands {
    /// List all archives in the configured repository
    List,
    /// List files inside a chosen archive
    Files {
        /// Archive name; if omitted, you will be prompted to choose
        archive: Option<String>,
    },
    /// Start interactive navigation
    Interactive,
}

#[derive(Debug, Deserialize)]
struct Config {
    /// Path/URL of the Borg repository, same as you would pass to `borg list`
    repo: String,
    /// Borg binary to invoke (optional)
    #[serde(default = "default_borg_bin")]
    borg_bin: String,
}

#[derive(Debug, Deserialize)]
struct BorgListResponse {
    archives: Vec<BorgArchive>,
}

#[derive(Debug, Deserialize)]
struct BorgArchive {
    #[serde(rename = "archive")]
    name: String,
    /// Timestamp string as returned by Borg (RFC3339)
    #[serde(rename = "time")]
    time_utc: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BorgItem {
    path: String,
    #[serde(rename = "type")]
    item_type: Option<String>,
    #[allow(dead_code)]
    size: Option<u64>,
}

fn default_borg_bin() -> String {
    "borg".to_string()
}

fn default_config_path() -> PathBuf {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("borg-tool").join("config.toml");
    }

    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home)
            .join(".config")
            .join("borg-tool")
            .join("config.toml");
    }

    // Fallback to current dir as a last resort.
    PathBuf::from("borg-tool.toml")
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cmd = cli.command;

    let config_path = cli.config.unwrap_or_else(default_config_path);
    let config = load_config(&config_path)
        .with_context(|| format!("Failed to load config from {}", config_path.display()))?;

    // Prompt passphrase once if needed; reused for subsequent borg calls.
    let passphrase = ensure_passphrase(&config)?;

    match cmd {
        None => run_interactive(&config, passphrase)?,
        Some(Commands::Interactive) => run_interactive(&config, passphrase)?,
        Some(Commands::List) => {
            let archives = list_archives(&config, passphrase.as_deref())?;
            print_archives(&archives);
        }
        Some(Commands::Files { archive }) => {
            let archives = list_archives(&config, passphrase.as_deref())?;
            let selected = match archive {
                Some(name) => archives
                    .iter()
                    .find(|a| a.name == name)
                    .ok_or_else(|| anyhow::anyhow!("Archive '{}' not found", name))?,
                None => prompt_archive_selection(&archives)?,
            };
            let items = list_items(&config, &selected.name, passphrase.as_deref())?;
            print_items(&items);
        }
    }

    Ok(())
}

fn load_config(path: &PathBuf) -> Result<Config> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("Cannot read config file {}", path.display()))?;
    let cfg: Config =
        toml::from_str(&raw).with_context(|| format!("Invalid TOML in {}", path.display()))?;
    Ok(cfg)
}

/// Return archives and optionally print them.
fn list_archives(cfg: &Config, passphrase: Option<&str>) -> Result<Vec<BorgArchive>> {
    let mut cmd = Command::new(&cfg.borg_bin);
    cmd.args(["list", "--json", &cfg.repo]);

    if let Some(pass) = passphrase {
        cmd.env("BORG_PASSPHRASE", pass);
    }

    let output = cmd
        .output()
        .with_context(|| format!("Failed to invoke {} binary", cfg.borg_bin))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "borg list failed with status {}: {}",
            output.status,
            stderr.trim()
        );
    }

    let parsed: BorgListResponse =
        serde_json::from_slice(&output.stdout).context("Failed to parse borg JSON output")?;

    Ok(parsed.archives)
}

fn list_items(cfg: &Config, archive: &str, passphrase: Option<&str>) -> Result<Vec<BorgItem>> {
    let mut cmd = Command::new(&cfg.borg_bin);
    cmd.args([
        "list",
        "--json-lines",
        &format!("{}::{}", cfg.repo, archive),
    ]);

    if let Some(pass) = passphrase {
        cmd.env("BORG_PASSPHRASE", pass);
    }

    let output = cmd
        .output()
        .with_context(|| format!("Failed to invoke {} binary", cfg.borg_bin))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "borg list for archive failed with status {}: {}",
            output.status,
            stderr.trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut items = Vec::new();
    for (idx, line) in stdout.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let item: BorgItem = serde_json::from_str(trimmed)
            .with_context(|| format!("Failed to parse JSON line {} from borg output", idx + 1))?;
        items.push(item);
    }
    Ok(items)
}

fn ensure_passphrase(cfg: &Config) -> Result<Option<String>> {
    if env::var("BORG_PASSCOMMAND").is_ok() || env::var("BORG_PASSPHRASE").is_ok() {
        return Ok(None);
    }

    let prompt = format!(
        "Enter passphrase for repo {} (leave empty if none): ",
        cfg.repo
    );
    let pass = prompt_password(prompt).context("Reading passphrase failed")?;
    Ok(Some(pass))
}

fn run_interactive(cfg: &Config, passphrase: Option<String>) -> Result<()> {
    let passphrase = passphrase;
    loop {
        println!();
        println!("borg-tool interactive");
        println!("1) List archives");
        println!("2) List files in archive");
        println!("q) Quit");
        print!("Select option: ");
        io::stdout().flush().ok();

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .context("Failed to read input")?;
        match input.trim() {
            "1" => {
                let archives = list_archives(cfg, passphrase.as_deref())?;
                print_archives(&archives);
            }
            "2" => {
                let archives = list_archives(cfg, passphrase.as_deref())?;
                if archives.is_empty() {
                    println!("No archives found");
                    continue;
                }
                let selected = prompt_archive_selection(&archives)?;
                let items = list_items(cfg, &selected.name, passphrase.as_deref())?;
                print_items(&items);
            }
            "q" | "quit" | "exit" => break,
            _ => println!("Unknown option"),
        }
    }
    Ok(())
}

fn prompt_archive_selection<'a>(archives: &'a [BorgArchive]) -> Result<&'a BorgArchive> {
    if archives.is_empty() {
        anyhow::bail!("No archives found");
    }

    println!("Select archive:");
    for (idx, arch) in archives.iter().enumerate() {
        let time = arch.time_utc.as_deref().unwrap_or("-");
        println!("[{:>2}] {:<40} {}", idx + 1, arch.name, time);
    }
    print!("Enter number: ");
    io::stdout().flush().ok();

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("Failed to read selection")?;
    let choice: usize = input
        .trim()
        .parse()
        .context("Please enter a valid number")?;
    if choice == 0 || choice > archives.len() {
        anyhow::bail!("Choice out of range");
    }
    Ok(&archives[choice - 1])
}

fn print_archives(archives: &[BorgArchive]) {
    if archives.is_empty() {
        println!("No archives found");
        return;
    }
    for arch in archives {
        let time = arch.time_utc.as_deref().unwrap_or("-");
        println!("{:<40} {}", arch.name, time);
    }
}

fn print_items(items: &[BorgItem]) {
    if items.is_empty() {
        println!("No files in archive");
        return;
    }

    for item in items {
        let kind = item.item_type.as_deref().unwrap_or("");
        println!("{:<8} {}", kind, item.path);
    }
}
