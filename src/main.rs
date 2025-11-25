use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use dialoguer::{Confirm, Input, Select, theme::ColorfulTheme};
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
    /// Which configured repo to use (by name)
    #[arg(short, long)]
    repo: Option<String>,

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
    /// Mount an archive to a target path
    Mount {
        /// Archive name
        archive: String,
        /// Target mountpoint
        #[arg(short, long)]
        target: Option<PathBuf>,
    },
    /// Unmount a mounted archive (by mountpoint)
    Umount {
        /// Mountpoint to unmount
        mountpoint: PathBuf,
    },
}

#[derive(Debug, Deserialize)]
struct Config {
    /// Repositories (preferred, supports multiple)
    #[serde(default)]
    repos: Vec<RepoConfig>,
    /// Legacy single repo (fallback)
    repo: Option<String>,
    /// Global borg binary default
    #[serde(default = "default_borg_bin")]
    borg_bin: String,
    /// Global mount root default
    #[serde(default = "default_mount_root")]
    mount_root: PathBuf,
}

#[derive(Debug, Deserialize)]
struct BorgListResponse {
    archives: Vec<BorgArchive>,
}

#[derive(Debug, Deserialize, Clone)]
struct BorgArchive {
    #[serde(rename = "archive")]
    name: String,
    /// Timestamp string as returned by Borg (RFC3339)
    #[serde(rename = "time")]
    time_utc: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct BorgItem {
    path: String,
    #[serde(rename = "type")]
    item_type: Option<String>,
    #[allow(dead_code)]
    size: Option<u64>,
}

#[derive(Debug, Deserialize, Clone)]
struct RepoConfig {
    name: String,
    /// Path/URL of the Borg repository
    repo: String,
    /// Optional repo-specific borg binary
    borg_bin: Option<String>,
    /// Optional repo-specific mount root
    mount_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct RepoCtx {
    name: String,
    repo: String,
    borg_bin: String,
    mount_root: PathBuf,
}

#[derive(Debug, Clone)]
struct MountInfo {
    archive: String,
    mountpoint: PathBuf,
}

fn default_borg_bin() -> String {
    "borg".to_string()
}

fn default_mount_root() -> PathBuf {
    env::temp_dir().join("borg-tool-mounts")
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

fn build_repo_list(cfg: &Config) -> Vec<RepoCtx> {
    if !cfg.repos.is_empty() {
        cfg.repos
            .iter()
            .map(|r| RepoCtx {
                name: r.name.clone(),
                repo: r.repo.clone(),
                borg_bin: r.borg_bin.clone().unwrap_or_else(|| cfg.borg_bin.clone()),
                mount_root: r
                    .mount_root
                    .clone()
                    .unwrap_or_else(|| cfg.mount_root.clone()),
            })
            .collect()
    } else if let Some(repo) = &cfg.repo {
        vec![RepoCtx {
            name: "default".to_string(),
            repo: repo.clone(),
            borg_bin: cfg.borg_bin.clone(),
            mount_root: cfg.mount_root.clone(),
        }]
    } else {
        Vec::new()
    }
}

fn select_repo_ctx(
    cfg: &Config,
    cli_repo: Option<&str>,
    cmd: Option<&Commands>,
    theme: &ColorfulTheme,
) -> Result<RepoCtx> {
    let repos = build_repo_list(cfg);
    if repos.is_empty() {
        anyhow::bail!("No repositories configured in config file");
    }

    // Single repo fast path
    if repos.len() == 1 {
        let ctx = repos.into_iter().next().unwrap();
        if let Some(req) = cli_repo {
            if req != ctx.name {
                anyhow::bail!(
                    "Repo '{}' not found. Only available repo: {}",
                    req,
                    ctx.name
                );
            }
        }
        return Ok(ctx);
    }

    // multiple repos
    if let Some(req) = cli_repo {
        if let Some(found) = repos.iter().find(|r| r.name == req) {
            return Ok(found.clone());
        }
        let names = repos.iter().map(|r| r.name.as_str()).collect::<Vec<_>>();
        anyhow::bail!("Repo '{}' not found. Available: {}", req, names.join(", "));
    }

    // interactive selection allowed only for interactive commands
    match cmd {
        None | Some(Commands::Interactive) => {
            let labels: Vec<String> = repos
                .iter()
                .map(|r| format!("{}  ({})", r.name, r.repo))
                .collect();
            let choice = Select::with_theme(theme)
                .with_prompt("Choose repository")
                .items(&labels)
                .default(0)
                .interact_opt()?;
            match choice {
                Some(idx) => Ok(repos[idx].clone()),
                None => anyhow::bail!("No repository selected"),
            }
        }
        _ => {
            let names = repos.iter().map(|r| r.name.as_str()).collect::<Vec<_>>();
            anyhow::bail!(
                "Multiple repos configured. Please choose with --repo <name>. Available: {}",
                names.join(", ")
            );
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cmd = cli.command;

    let config_path = cli.config.unwrap_or_else(default_config_path);
    let config = load_config(&config_path)
        .with_context(|| format!("Failed to load config from {}", config_path.display()))?;

    let theme = ColorfulTheme::default();
    let repo_ctx = select_repo_ctx(&config, cli.repo.as_deref(), cmd.as_ref(), &theme)?;

    // Prompt passphrase once if needed; reused for subsequent borg calls.
    let passphrase = ensure_passphrase(&repo_ctx)?;

    match cmd {
        None => run_interactive(&repo_ctx, passphrase)?,
        Some(Commands::Interactive) => run_interactive(&repo_ctx, passphrase)?,
        Some(Commands::List) => {
            let archives = list_archives(&repo_ctx, passphrase.as_deref())?;
            print_archives(&archives);
        }
        Some(Commands::Files { archive }) => {
            let archives = list_archives(&repo_ctx, passphrase.as_deref())?;
            let selected = match archive {
                Some(name) => archives
                    .iter()
                    .find(|a| a.name == name)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("Archive '{}' not found", name))?,
                None => match select_archive(&archives, &theme)? {
                    Some(a) => a,
                    None => return Ok(()),
                },
            };
            let items = list_items(&repo_ctx, &selected.name, passphrase.as_deref())?;
            print_items(&items);
        }
        Some(Commands::Mount { archive, target }) => {
            ensure_mount_available(&repo_ctx)?;
            let mountpoint = target.unwrap_or_else(|| default_mountpoint(&repo_ctx, &archive));
            mount_archive(&repo_ctx, &archive, &mountpoint, passphrase.as_deref())?;
            println!("Mounted {} at {}", archive, mountpoint.display());
        }
        Some(Commands::Umount { mountpoint }) => {
            umount_archive(&repo_ctx, &mountpoint, passphrase.as_deref())?;
            println!("Unmounted {}", mountpoint.display());
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
fn list_archives(ctx: &RepoCtx, passphrase: Option<&str>) -> Result<Vec<BorgArchive>> {
    let mut cmd = Command::new(&ctx.borg_bin);
    cmd.args(["list", "--json", &ctx.repo]);

    if let Some(pass) = passphrase {
        cmd.env("BORG_PASSPHRASE", pass);
    }

    let output = cmd
        .output()
        .with_context(|| format!("Failed to invoke {} binary", ctx.borg_bin))?;

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

fn list_items(ctx: &RepoCtx, archive: &str, passphrase: Option<&str>) -> Result<Vec<BorgItem>> {
    let mut cmd = Command::new(&ctx.borg_bin);
    cmd.args([
        "list",
        "--json-lines",
        &format!("{}::{}", ctx.repo, archive),
    ]);

    if let Some(pass) = passphrase {
        cmd.env("BORG_PASSPHRASE", pass);
    }

    let output = cmd
        .output()
        .with_context(|| format!("Failed to invoke {} binary", ctx.borg_bin))?;

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

fn select_archive(archives: &[BorgArchive], theme: &ColorfulTheme) -> Result<Option<BorgArchive>> {
    let items: Vec<String> = archives
        .iter()
        .map(|a| {
            let time = a.time_utc.as_deref().unwrap_or("-");
            format!("{}  [{}]", a.name, time)
        })
        .collect();

    let selection = Select::with_theme(theme)
        .with_prompt("Choose archive (Esc/CTRL+C to quit)")
        .items(&items)
        .default(0)
        .interact_opt()?;

    Ok(selection.map(|idx| archives[idx].clone()))
}

fn select_item(items: &[BorgItem], theme: &ColorfulTheme) -> Result<Option<BorgItem>> {
    let display: Vec<String> = items
        .iter()
        .map(|i| format!("{:<6} {}", i.item_type.as_deref().unwrap_or(""), i.path))
        .collect();

    let selection = Select::with_theme(theme)
        .with_prompt("Choose file (Esc to go back)")
        .items(&display)
        .default(0)
        .interact_opt()?;

    Ok(selection.map(|idx| items[idx].clone()))
}

#[derive(Debug, Clone, Copy)]
enum ArchiveAction {
    Browse,
    Mount,
    UnmountCurrent,
    Back,
}

fn select_archive_action(
    theme: &ColorfulTheme,
    has_mount: bool,
    mount_available: bool,
) -> Result<ArchiveAction> {
    let mut options = vec!["Browse files"];
    if mount_available {
        options.push("Mount");
    }
    if has_mount {
        options.push("Unmount current");
    }
    options.push("Back");

    let choice = Select::with_theme(theme)
        .with_prompt("Action (Enter)")
        .items(&options)
        .default(0)
        .interact_opt()?;

    let action = match choice {
        Some(idx) => {
            let label = options[idx];
            match label {
                "Browse files" => ArchiveAction::Browse,
                "Mount" => ArchiveAction::Mount,
                "Unmount current" => ArchiveAction::UnmountCurrent,
                _ => ArchiveAction::Back,
            }
        }
        None => ArchiveAction::Back,
    };
    Ok(action)
}

fn extract_file(
    ctx: &RepoCtx,
    archive: &str,
    path_in_archive: &str,
    dest_dir: &str,
    passphrase: Option<&str>,
) -> Result<()> {
    fs::create_dir_all(dest_dir).with_context(|| format!("Create destination {}", dest_dir))?;

    let mut cmd = Command::new(&ctx.borg_bin);
    cmd.current_dir(dest_dir);
    cmd.arg("extract");

    // Strip leading path components so only the selected entry is written.
    let strip_components = std::path::Path::new(path_in_archive)
        .components()
        .count()
        .saturating_sub(1);
    if strip_components > 0 {
        cmd.args(["--strip-components", &strip_components.to_string()]);
    }

    cmd.args([&format!("{}::{}", ctx.repo, archive), path_in_archive]);

    if let Some(pass) = passphrase {
        cmd.env("BORG_PASSPHRASE", pass);
    }

    let output = cmd
        .output()
        .with_context(|| format!("Failed to invoke {} binary", ctx.borg_bin))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "borg extract failed with status {}: {}",
            output.status,
            stderr.trim()
        );
    }

    Ok(())
}

fn mount_archive(
    ctx: &RepoCtx,
    archive: &str,
    mountpoint: &Path,
    passphrase: Option<&str>,
) -> Result<()> {
    fs::create_dir_all(mountpoint)
        .with_context(|| format!("Create mountpoint {}", mountpoint.display()))?;

    let mut cmd = Command::new(&ctx.borg_bin);
    cmd.args([
        "mount",
        &format!("{}::{}", ctx.repo, archive),
        &mountpoint.display().to_string(),
    ]);

    if let Some(pass) = passphrase {
        cmd.env("BORG_PASSPHRASE", pass);
    }

    let output = cmd
        .output()
        .with_context(|| format!("Failed to invoke {} binary", ctx.borg_bin))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "borg mount failed with status {}: {}",
            output.status,
            stderr.trim()
        );
    }

    Ok(())
}

fn umount_archive(ctx: &RepoCtx, mountpoint: &Path, passphrase: Option<&str>) -> Result<()> {
    let mut cmd = Command::new(&ctx.borg_bin);
    cmd.args(["umount", &mountpoint.display().to_string()]);

    if let Some(pass) = passphrase {
        cmd.env("BORG_PASSPHRASE", pass);
    }

    let output = cmd
        .output()
        .with_context(|| format!("Failed to invoke {} binary", ctx.borg_bin))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "borg umount failed with status {}: {}",
            output.status,
            stderr.trim()
        );
    }

    Ok(())
}

fn default_mountpoint(ctx: &RepoCtx, archive: &str) -> PathBuf {
    ctx.mount_root.join(archive)
}

fn ensure_mount_available(ctx: &RepoCtx) -> Result<bool> {
    let output = Command::new(&ctx.borg_bin)
        .args(["mount", "--help"])
        .output()
        .with_context(|| format!("Failed to invoke {} binary", ctx.borg_bin))?;

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .to_lowercase();

    if combined.contains("no fuse support") {
        return Ok(false);
    }

    if output.status.success() {
        return Ok(true);
    }

    // fallback: assume available to avoid false negatives
    Ok(true)
}

fn ensure_passphrase(ctx: &RepoCtx) -> Result<Option<String>> {
    if env::var("BORG_PASSCOMMAND").is_ok() || env::var("BORG_PASSPHRASE").is_ok() {
        return Ok(None);
    }

    let prompt = format!(
        "Enter passphrase for repo {} (leave empty if none): ",
        ctx.repo
    );
    let pass = prompt_password(prompt).context("Reading passphrase failed")?;
    Ok(Some(pass))
}

fn run_interactive(repo: &RepoCtx, passphrase: Option<String>) -> Result<()> {
    let theme = ColorfulTheme::default();
    let current_pass = passphrase;
    let mut mount_state: Option<MountInfo> = None;
    let mount_available = ensure_mount_available(repo).unwrap_or(false);

    loop {
        let archives = list_archives(repo, current_pass.as_deref())?;
        if archives.is_empty() {
            println!("No archives found");
            return Ok(());
        }

        if !mount_available {
            println!("(Mount unavailable: no FUSE support detected)");
        } else if let Some(m) = &mount_state {
            println!("Mounted: {} @ {}", m.archive, m.mountpoint.display());
        }

        let archive = match select_archive(&archives, &theme)? {
            Some(a) => a,
            None => return Ok(()),
        };

        match select_archive_action(&theme, mount_state.is_some(), mount_available)? {
            ArchiveAction::Browse => {
                browse_files(repo, &archive, current_pass.as_deref(), &theme)?;
            }
            ArchiveAction::Mount => {
                if let Some(active) = &mount_state {
                    if Confirm::with_theme(&theme)
                        .with_prompt(format!(
                            "Unmount current ({}) before mounting new one?",
                            active.mountpoint.display()
                        ))
                        .default(true)
                        .interact()?
                    {
                        umount_archive(repo, &active.mountpoint, current_pass.as_deref())?;
                        println!("Unmounted {}", active.mountpoint.display());
                    } else {
                        continue;
                    }
                }

                let default_mp = default_mountpoint(repo, &archive.name);
                let target: String = Input::with_theme(&theme)
                    .with_prompt("Mountpoint")
                    .default(default_mp.display().to_string())
                    .interact_text()?;
                let target_path = PathBuf::from(target);
                mount_archive(repo, &archive.name, &target_path, current_pass.as_deref())?;
                println!("Mounted {} at {}", archive.name, target_path.display());
                mount_state = Some(MountInfo {
                    archive: archive.name.clone(),
                    mountpoint: target_path,
                });
            }
            ArchiveAction::Back => {}
            ArchiveAction::UnmountCurrent => {
                if let Some(active) = mount_state.take() {
                    umount_archive(repo, &active.mountpoint, current_pass.as_deref())?;
                    println!("Unmounted {}", active.mountpoint.display());
                }
            }
        }
    }
}

fn browse_files(
    repo: &RepoCtx,
    archive: &BorgArchive,
    passphrase: Option<&str>,
    theme: &ColorfulTheme,
) -> Result<()> {
    loop {
        let items = list_items(repo, &archive.name, passphrase)?;
        if items.is_empty() {
            println!("No files in archive {}", archive.name);
            return Ok(());
        }

        let item = match select_item(&items, theme)? {
            Some(i) => i,
            None => return Ok(()), // back to archive list
        };

        if Confirm::with_theme(theme)
            .with_prompt(format!(
                "Extract '{}' from '{}' to current directory?",
                item.path, archive.name
            ))
            .default(false)
            .interact()?
        {
            let dest: String = Input::with_theme(theme)
                .with_prompt("Destination directory")
                .default(".".to_string())
                .interact_text()?;
            extract_file(repo, &archive.name, &item.path, &dest, passphrase)?;
            println!("Extracted to {}", dest);
        }
    }
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
