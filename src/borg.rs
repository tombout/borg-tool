use std::{
    fs,
    path::Path,
    process::{Command, Output},
    time::Duration,
};

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

fn run_borg<F>(ctx: &RepoCtx, passphrase: Option<&str>, build: F) -> Result<Output>
where
    F: FnOnce(&mut Command),
{
    let mut cmd = Command::new(&ctx.borg_bin);
    build(&mut cmd);

    if let Some(pass) = passphrase {
        cmd.env("BORG_PASSPHRASE", pass);
    }

    cmd.output()
        .with_context(|| format!("Failed to invoke {} binary", ctx.borg_bin))
}

fn ensure_success(action: &str, output: Output) -> Result<Output> {
    if output.status.success() {
        return Ok(output);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!(
        "borg {} failed with status {}: {}",
        action,
        output.status,
        stderr.trim()
    );
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
        let output = run_borg(ctx, passphrase, |cmd| {
            cmd.args(["list", "--json", &ctx.repo]);
        })?;
        let output = ensure_success("list", output)?;

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
        let output = run_borg(ctx, passphrase, |cmd| {
            cmd.args([
                "list",
                "--json-lines",
                &format!("{}::{}", ctx.repo, archive),
            ]);
        })?;
        let output = ensure_success("list items", output)?;

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

            let output = run_borg(ctx, passphrase, |cmd| {
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
            })?;

            ensure_success("extract", output)?;

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
            ensure_mountpoint_ready(mountpoint)?;

            let output = run_borg(ctx, passphrase, |cmd| {
                cmd.args([
                    "mount",
                    &format!("{}::{}", ctx.repo, archive),
                    &mountpoint.display().to_string(),
                ]);
            })?;

            ensure_success("mount", output)?;

            Ok(())
        },
    )
}

fn ensure_mountpoint_ready(path: &Path) -> Result<()> {
    if path.exists() {
        if !path.is_dir() {
            anyhow::bail!(
                "Mountpoint {} exists and is not a directory",
                path.display()
            );
        }
        let mut entries =
            fs::read_dir(path).with_context(|| format!("Reading mountpoint {}", path.display()))?;
        if entries.next().is_some() {
            anyhow::bail!(
                "Mountpoint {} is not empty; choose an empty directory",
                path.display()
            );
        }
        return Ok(());
    }

    fs::create_dir_all(path).with_context(|| format!("Create mountpoint {}", path.display()))?;
    Ok(())
}

pub fn umount_archive(ctx: &RepoCtx, mountpoint: &Path, passphrase: Option<&str>) -> Result<()> {
    with_spinner(&format!("Unmounting {}", mountpoint.display()), |_pb| {
        let output = run_borg(ctx, passphrase, |cmd| {
            cmd.args(["umount", &mountpoint.display().to_string()]);
        })?;

        ensure_success("umount", output)?;

        Ok(())
    })
}

pub fn default_mountpoint(ctx: &RepoCtx, archive: &str) -> std::path::PathBuf {
    ctx.mount_root.join(archive)
}

pub fn ensure_mount_available(ctx: &RepoCtx) -> Result<bool> {
    with_spinner("Checking mount support", |_pb| {
        let output = run_borg(ctx, None, |cmd| {
            cmd.args(["mount", "--help"]);
        })?;

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

pub fn init_repo(ctx: &RepoCtx, encryption: &str, passphrase: Option<&str>) -> Result<()> {
    with_spinner("Initializing repository", |_pb| {
        let output = run_borg(ctx, passphrase, |cmd| {
            cmd.args(["init", "--encryption", encryption, &ctx.repo]);
        })?;

        ensure_success("init", output)?;
        Ok(())
    })
}

pub fn build_archive_name(preset: &BackupConfig, repo_name: &str) -> String {
    let ts = Local::now().format("%Y-%m-%d_%H-%M-%S");
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

    let repo_exclude =
        repo_exclude_pattern(ctx).filter(|pat| !preset.excludes.iter().any(|e| e == pat));
    let archive_name = build_archive_name(preset, &ctx.name);

    with_spinner(&format!("Creating {}", archive_name), |_pb| {
        let output = run_borg(ctx, passphrase, |cmd| {
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
            if let Some(exclude) = &repo_exclude {
                // avoid backing up the repo itself when includes point above it
                cmd.args(["--exclude", exclude]);
            }

            cmd.arg(format!("{}::{}", ctx.repo, archive_name));
            for inc in &preset.includes {
                cmd.arg(inc);
            }
        })?;

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

fn repo_exclude_pattern(ctx: &RepoCtx) -> Option<String> {
    let path = std::path::Path::new(&ctx.repo);
    if !path.is_absolute() || !path.exists() {
        return None;
    }
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_str()
        .map(|s| s.to_string())
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
        let host_port = host_part.rsplit('@').next().unwrap_or(host_part);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn fake_borg_binary(dir: &tempfile::TempDir, capture: &std::path::Path) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.path().join("fake-borg");
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\n",
            capture.display()
        );
        std::fs::write(&path, script).unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[cfg(unix)]
    fn captured_args(path: &std::path::Path) -> Vec<String> {
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn build_archive_name_uses_prefix_and_preset() {
        let preset = BackupConfig {
            name: "home".into(),
            includes: vec!["/data".into()],
            excludes: vec![],
            compression: None,
            one_file_system: false,
            exclude_caches: false,
            archive_prefix: Some("raspi".into()),
        };

        let name = build_archive_name(&preset, "repo");

        // Format: <prefix>-<preset>-YYYYMMDD-HHMMSS
        let parts: Vec<&str> = name.split('-').collect();
        assert!(parts.len() >= 3, "unexpected format: {name}");
        assert_eq!(parts[0], "raspi");
        assert_eq!(parts[1], "home");
    }

    #[test]
    fn build_archive_name_defaults_to_repo_prefix() {
        let preset = BackupConfig {
            name: "sys".into(),
            includes: vec!["/".into()],
            excludes: vec![],
            compression: None,
            one_file_system: false,
            exclude_caches: false,
            archive_prefix: None,
        };

        let name = build_archive_name(&preset, "laptop");
        let parts: Vec<&str> = name.split('-').collect();
        assert!(parts.len() >= 3, "unexpected format: {name}");
        assert_eq!(parts[0], "laptop");
        assert_eq!(parts[1], "sys");
    }

    #[test]
    fn extract_ssh_host_parses_variants() {
        assert_eq!(
            extract_ssh_host("ssh://user@host:22/path"),
            Some("host".into())
        );
        assert_eq!(extract_ssh_host("ssh://host/repo"), Some("host".into()));
        assert_eq!(extract_ssh_host("user@host:/repo"), Some("host".into()));
        assert_eq!(extract_ssh_host("host"), None);
    }

    #[test]
    fn repo_exclude_pattern_returns_local_path() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_path = tmp.path().join("repo");
        std::fs::create_dir(&repo_path).unwrap();
        let ctx = RepoCtx {
            name: "r".into(),
            repo: repo_path.to_string_lossy().into_owned(),
            borg_bin: "borg".into(),
            mount_root: tmp.path().join("mnt"),
            backups: vec![],
            status: super::super::config::RepoStatus::Ok,
        };

        let exclude = repo_exclude_pattern(&ctx).expect("should produce exclude");
        assert_eq!(
            exclude,
            repo_path
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .to_string()
        );
    }

    #[test]
    fn repo_exclude_pattern_skips_remote() {
        let ctx = RepoCtx {
            name: "r".into(),
            repo: "ssh://user@host/remote".into(),
            borg_bin: "borg".into(),
            mount_root: "/mnt".into(),
            backups: vec![],
            status: super::super::config::RepoStatus::Unknown,
        };

        assert!(repo_exclude_pattern(&ctx).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn run_backup_adds_repo_exclude_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_path = tmp.path().join("repo");
        std::fs::create_dir(&repo_path).unwrap();
        let capture = tmp.path().join("args.txt");
        let borg_bin = fake_borg_binary(&tmp, &capture);

        let preset = BackupConfig {
            name: "home".into(),
            includes: vec![tmp.path().to_string_lossy().into_owned()],
            excludes: vec![],
            compression: None,
            one_file_system: false,
            exclude_caches: false,
            archive_prefix: None,
        };
        let ctx = RepoCtx {
            name: "r".into(),
            repo: repo_path.to_string_lossy().into_owned(),
            borg_bin: borg_bin.to_string_lossy().into_owned(),
            mount_root: tmp.path().join("mnt"),
            backups: vec![],
            status: super::super::config::RepoStatus::Ok,
        };

        run_backup(&ctx, &preset, None).unwrap();

        let args = captured_args(&capture);
        let exclude_count = args.iter().filter(|a| *a == "--exclude").count();
        assert_eq!(exclude_count, 1, "expected exactly one auto-exclude");

        let expected_path = repo_path
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert!(
            args.windows(2).any(
                |w| matches!(w, [flag, path] if flag == "--exclude" && path == &expected_path)
            ),
            "exclude list should contain canonical repo path"
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_backup_skips_repo_exclude_when_already_present() {
        let tmp = tempfile::tempdir().unwrap();
        let repo_path = tmp.path().join("repo");
        std::fs::create_dir(&repo_path).unwrap();
        let capture = tmp.path().join("args.txt");
        let borg_bin = fake_borg_binary(&tmp, &capture);
        let canonical = repo_path
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();

        let preset = BackupConfig {
            name: "home".into(),
            includes: vec![tmp.path().to_string_lossy().into_owned()],
            excludes: vec![canonical.clone()],
            compression: None,
            one_file_system: false,
            exclude_caches: false,
            archive_prefix: None,
        };
        let ctx = RepoCtx {
            name: "r".into(),
            repo: repo_path.to_string_lossy().into_owned(),
            borg_bin: borg_bin.to_string_lossy().into_owned(),
            mount_root: tmp.path().join("mnt"),
            backups: vec![],
            status: super::super::config::RepoStatus::Ok,
        };

        run_backup(&ctx, &preset, None).unwrap();

        let args = captured_args(&capture);
        let exclude_count = args.iter().filter(|a| *a == "--exclude").count();
        assert_eq!(
            exclude_count, 1,
            "should not add a second repo exclude when already specified"
        );
        assert!(
            args.windows(2)
                .any(|w| matches!(w, [flag, path] if flag == "--exclude" && path == &canonical)),
            "preset exclude should remain intact"
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_backup_does_not_add_exclude_for_relative_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let capture = tmp.path().join("args.txt");
        let borg_bin = fake_borg_binary(&tmp, &capture);

        let preset = BackupConfig {
            name: "home".into(),
            includes: vec![tmp.path().to_string_lossy().into_owned()],
            excludes: vec![],
            compression: None,
            one_file_system: false,
            exclude_caches: false,
            archive_prefix: None,
        };
        let ctx = RepoCtx {
            name: "r".into(),
            repo: "relative/repo".into(),
            borg_bin: borg_bin.to_string_lossy().into_owned(),
            mount_root: tmp.path().join("mnt"),
            backups: vec![],
            status: super::super::config::RepoStatus::Ok,
        };

        run_backup(&ctx, &preset, None).unwrap();

        let args = captured_args(&capture);
        let exclude_count = args.iter().filter(|a| *a == "--exclude").count();
        assert_eq!(
            exclude_count, 0,
            "relative repo path should not trigger automatic exclude"
        );
    }
}
