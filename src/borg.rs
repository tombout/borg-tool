use std::{fs, path::Path, process::Command, time::Duration};

use anyhow::{Context, Result};
use chrono::Local;
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;

use crate::config::{BackupConfig, RepoCtx};

#[derive(Debug, Deserialize)]
pub struct BorgListResponse {
    pub archives: Vec<BorgArchive>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BorgArchive {
    #[serde(rename = "archive")]
    pub name: String,
    /// Timestamp string as returned by Borg (RFC3339)
    #[serde(rename = "time")]
    pub time_utc: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BorgItem {
    pub path: String,
    #[serde(rename = "type")]
    pub item_type: Option<String>,
    #[allow(dead_code)]
    pub size: Option<u64>,
}

fn spinner_style() -> ProgressStyle {
    ProgressStyle::with_template("{spinner:.green} {msg}").expect("static spinner template")
}

pub fn with_spinner<T, F>(message: &str, action: F) -> Result<T>
where
    F: FnOnce(&ProgressBar) -> Result<T>,
{
    let pb = ProgressBar::new_spinner();
    pb.set_style(spinner_style());
    pb.set_message(message.to_string());
    pb.enable_steady_tick(Duration::from_millis(120));

    let result = action(&pb);

    match &result {
        Ok(_) => pb.finish_with_message(format!("{} ✓", message)),
        Err(_) => pb.finish_with_message(format!("{} ✗", message)),
    }

    result
}

pub fn list_archives(ctx: &RepoCtx, passphrase: Option<&str>) -> Result<Vec<BorgArchive>> {
    with_spinner("Listing archives", |pb| {
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

        pb.set_message(format!(
            "Listing archives ({} found)",
            parsed.archives.len()
        ));

        Ok(parsed.archives)
    })
}

pub fn list_items(ctx: &RepoCtx, archive: &str, passphrase: Option<&str>) -> Result<Vec<BorgItem>> {
    with_spinner(&format!("Listing items in {}", archive), |_pb| {
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
            let item: BorgItem = serde_json::from_str(trimmed).with_context(|| {
                format!("Failed to parse JSON line {} from borg output", idx + 1)
            })?;
            items.push(item);
        }
        Ok(items)
    })
}

pub fn extract_file(
    ctx: &RepoCtx,
    archive: &str,
    path_in_archive: &str,
    dest_dir: &str,
    passphrase: Option<&str>,
) -> Result<()> {
    with_spinner(
        &format!("Extracting '{}' from {}", path_in_archive, archive),
        |_pb| {
            fs::create_dir_all(dest_dir)
                .with_context(|| format!("Create destination {}", dest_dir))?;

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
        },
    )
}

pub fn mount_archive(
    ctx: &RepoCtx,
    archive: &str,
    mountpoint: &Path,
    passphrase: Option<&str>,
) -> Result<()> {
    with_spinner(
        &format!("Mounting {} to {}", archive, mountpoint.display()),
        |_pb| {
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
        },
    )
}

pub fn umount_archive(ctx: &RepoCtx, mountpoint: &Path, passphrase: Option<&str>) -> Result<()> {
    with_spinner(&format!("Unmounting {}", mountpoint.display()), |_pb| {
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
    })
}

pub fn default_mountpoint(ctx: &RepoCtx, archive: &str) -> std::path::PathBuf {
    ctx.mount_root.join(archive)
}

pub fn ensure_mount_available(ctx: &RepoCtx) -> Result<bool> {
    with_spinner("Checking mount support", |_pb| {
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
    })
}

pub fn build_archive_name(preset: &BackupConfig, repo_name: &str) -> String {
    let ts = Local::now().format("%Y%m%d-%H%M%S");
    let mut segments = Vec::new();

    if let Some(prefix) = preset.archive_prefix.as_deref() {
        if !prefix.is_empty() {
            segments.push(prefix.trim_end_matches(['-', '_']));
        }
    } else {
        segments.push(repo_name);
    }

    segments.push(&preset.name);

    format!("{}-{}", segments.join("-"), ts)
}

pub fn run_backup(ctx: &RepoCtx, preset: &BackupConfig, passphrase: Option<&str>) -> Result<()> {
    if preset.includes.is_empty() {
        anyhow::bail!("Backup '{}' has no includes configured", preset.name);
    }

    let archive_name = build_archive_name(preset, &ctx.name);

    with_spinner(&format!("Creating {}", archive_name), |_pb| {
        let mut cmd = Command::new(&ctx.borg_bin);
        cmd.arg("create");

        if let Some(comp) = &preset.compression {
            cmd.args(["--compression", comp]);
        }
        if preset.one_file_system {
            cmd.arg("--one-file-system");
        }
        if preset.exclude_caches {
            cmd.arg("--exclude-caches");
        }
        for pat in &preset.excludes {
            cmd.args(["--exclude", pat]);
        }

        cmd.arg(format!("{}::{}", ctx.repo, archive_name));
        for inc in &preset.includes {
            cmd.arg(inc);
        }

        if let Some(pass) = passphrase {
            cmd.env("BORG_PASSPHRASE", pass);
        }

        let output = cmd
            .output()
            .with_context(|| format!("Failed to invoke {} binary", ctx.borg_bin))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let hint = if stderr.to_lowercase().contains("permission denied") {
                " (hint: run with sudo for system paths)"
            } else {
                ""
            };
            anyhow::bail!(
                "borg create failed with status {}: {}{}",
                output.status,
                stderr.trim(),
                hint
            );
        }

        Ok(())
    })?;

    println!("Backup '{}' completed", archive_name);
    Ok(())
}

pub fn ensure_passphrase(ctx: &RepoCtx) -> Result<Option<String>> {
    if std::env::var("BORG_PASSCOMMAND").is_ok() || std::env::var("BORG_PASSPHRASE").is_ok() {
        return Ok(None);
    }

    let prompt = format!(
        "Enter passphrase for repo {} (leave empty if none): ",
        ctx.repo
    );
    let pass = rpassword::prompt_password(prompt).context("Reading passphrase failed")?;
    Ok(Some(pass))
}

pub fn ensure_passphrase_cached(
    cached: &mut Option<String>,
    ctx: &RepoCtx,
) -> Result<Option<String>> {
    if cached.is_none() {
        *cached = ensure_passphrase(ctx)?;
    }
    Ok(cached.clone())
}

pub fn probe_remote(repo: &str) -> super::config::RepoStatus {
    let Some(host) = extract_ssh_host(repo) else {
        return super::config::RepoStatus::Unknown;
    };

    let output = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "ConnectTimeout=5",
            &host,
            "true",
        ])
        .output();

    match output {
        Ok(out) if out.status.success() => super::config::RepoStatus::RemoteOk,
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).to_lowercase();
            if stderr.contains("permission denied")
                || stderr.contains("publickey")
                || stderr.contains("password")
            {
                super::config::RepoStatus::RemoteAuthNeeded
            } else {
                super::config::RepoStatus::Unknown
            }
        }
        Err(_) => super::config::RepoStatus::Unknown,
    }
}

pub fn extract_ssh_host(repo: &str) -> Option<String> {
    if let Some(rest) = repo.strip_prefix("ssh://") {
        let host_part = rest.split('/').next().unwrap_or(rest);
        let host_port = host_part.split('@').last().unwrap_or(host_part);
        // drop possible path after colon for scp-like paths inside ssh:// already handled above
        let host = host_port.split(':').next().unwrap_or(host_port);
        return Some(host.to_string());
    }

    // scp-like syntax user@host:/path or user@host:repo
    if repo.contains('@') && repo.contains(':') {
        let after_at = repo.split('@').nth(1)?;
        let host = after_at.split(':').next().unwrap_or(after_at);
        return Some(host.to_string());
    }

    None
}

pub fn repo_status(repo: &str, probe_ssh: bool) -> super::config::RepoStatus {
    if repo.contains("://") || (repo.contains('@') && repo.contains(':')) {
        return if probe_ssh {
            probe_remote(repo)
        } else {
            super::config::RepoStatus::Unknown
        };
    }

    let path = Path::new(repo);
    if path.exists() {
        super::config::RepoStatus::Ok
    } else {
        super::config::RepoStatus::MissingLocal
    }
}
