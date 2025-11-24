use std::{env, fs, path::PathBuf, process::Command};

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
    #[arg(short, long, default_value = "borg-tool.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug, Clone)]
enum Commands {
    /// List all archives in the configured repository
    List,
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

fn default_borg_bin() -> String {
    "borg".to_string()
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cmd = cli.command.unwrap_or(Commands::List);

    let config = load_config(&cli.config)
        .with_context(|| format!("Failed to load config from {}", cli.config.display()))?;

    match cmd {
        Commands::List => list_archives(&config)?,
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

fn list_archives(cfg: &Config) -> Result<()> {
    let mut cmd = Command::new(&cfg.borg_bin);
    cmd.args(["list", "--json", &cfg.repo]);

    // If no passphrase is provided via env, ask with a clearer prompt.
    if env::var("BORG_PASSCOMMAND").is_err() && env::var("BORG_PASSPHRASE").is_err() {
        let prompt = format!(
            "Enter passphrase for repo {} (leave empty if none): ",
            cfg.repo
        );
        let pass = prompt_password(prompt).context("Reading passphrase failed")?;
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

    if parsed.archives.is_empty() {
        println!("No archives found in {}", cfg.repo);
        return Ok(());
    }

    for arch in parsed.archives {
        let time = arch.time_utc.as_deref().unwrap_or("-");
        println!("{:<40} {}", arch.name, time);
    }

    Ok(())
}
