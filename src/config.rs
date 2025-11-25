use std::{env, fs, io::ErrorKind, path::PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    /// Repositories (preferred, supports multiple)
    #[serde(default)]
    pub repos: Vec<RepoConfig>,
    /// Legacy single repo (fallback)
    pub repo: Option<String>,
    /// Global borg binary default
    #[serde(default = "default_borg_bin")]
    pub borg_bin: String,
    /// Global mount root default
    #[serde(default = "default_mount_root")]
    pub mount_root: PathBuf,
    /// Probe SSH availability on startup
    #[serde(default = "default_probe_ssh")]
    pub probe_ssh: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RepoConfig {
    pub name: String,
    /// Path/URL of the Borg repository
    pub repo: String,
    /// Optional repo-specific borg binary
    pub borg_bin: Option<String>,
    /// Optional repo-specific mount root
    pub mount_root: Option<PathBuf>,
    /// Optional backup presets for this repo
    #[serde(default)]
    pub backups: Vec<BackupConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BackupConfig {
    /// Identifier used on the CLI
    pub name: String,
    /// Paths or patterns to include
    pub includes: Vec<String>,
    /// Paths or patterns to exclude
    #[serde(default)]
    pub excludes: Vec<String>,
    /// Optional compression mode, e.g. "lz4" or "zstd,5"
    #[serde(default)]
    pub compression: Option<String>,
    /// If true, stay on the same file system
    #[serde(default)]
    pub one_file_system: bool,
    /// If true, add --exclude-caches
    #[serde(default)]
    pub exclude_caches: bool,
    /// Archive name prefix (final name becomes "<prefix><name>-<timestamp>")
    #[serde(default)]
    pub archive_prefix: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RepoCtx {
    pub name: String,
    pub repo: String,
    pub borg_bin: String,
    pub mount_root: PathBuf,
    pub backups: Vec<BackupConfig>,
    pub status: RepoStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoStatus {
    Ok,
    MissingLocal,
    RemoteOk,
    RemoteAuthNeeded,
    Unknown,
}

pub fn default_borg_bin() -> String {
    "borg".to_string()
}

pub fn default_mount_root() -> PathBuf {
    env::temp_dir().join("borg-tool-mounts")
}

pub fn default_probe_ssh() -> bool {
    true
}

pub fn default_config_path() -> PathBuf {
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
    PathBuf::from("config.toml")
}

pub fn load_config(path: &PathBuf) -> Result<Config> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("Cannot read config file {}", path.display()))?;
    let cfg: Config =
        toml::from_str(&raw).with_context(|| format!("Invalid TOML in {}", path.display()))?;
    Ok(cfg)
}

pub fn load_config_resolved(cli_path: Option<PathBuf>) -> Result<(Config, PathBuf)> {
    if let Some(path) = cli_path {
        let cfg = load_config(&path)?;
        return Ok((cfg, path));
    }

    let default_path = default_config_path();
    let fallback_path = PathBuf::from("config.toml");
    let candidates = [default_path.clone(), fallback_path.clone()];
    let mut last_not_found: Option<(PathBuf, anyhow::Error)> = None;

    for path in candidates {
        match load_config(&path) {
            Ok(cfg) => return Ok((cfg, path)),
            Err(err) => {
                let not_found = err
                    .downcast_ref::<std::io::Error>()
                    .map(|ioe| ioe.kind() == ErrorKind::NotFound)
                    .unwrap_or(false);
                if not_found {
                    last_not_found = Some((path, err));
                    continue;
                }
                // any other error should surface immediately
                return Err(err);
            }
        }
    }

    let tried = vec![default_path, fallback_path]
        .into_iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");

    if let Some((_, err)) = last_not_found {
        return Err(err.context(format!("No config file found. Tried: {}", tried)));
    }

    anyhow::bail!("No config file found. Tried: {}", tried)
}

pub fn status_label(status: RepoStatus) -> &'static str {
    match status {
        RepoStatus::Ok => "ok",
        RepoStatus::MissingLocal => "missing",
        RepoStatus::RemoteOk => "remote-ok",
        RepoStatus::RemoteAuthNeeded => "remote-auth?",
        RepoStatus::Unknown => "remote?",
    }
}
