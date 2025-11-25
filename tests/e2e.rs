use std::path::Path;
use std::process::Command;

use assert_cmd::prelude::*;
use assert_fs::prelude::*;
use predicates::str::contains;
use serde::Deserialize;
use tempfile::TempDir;

// Helper structs to parse borg JSON output
#[derive(Deserialize)]
struct BorgArchive {
    #[serde(rename = "archive")]
    name: String,
}

#[derive(Deserialize)]
struct BorgListResponse {
    archives: Vec<BorgArchive>,
}

fn ensure_borg_available() -> bool {
    Command::new("borg")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|v| matches!(v.as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

fn apply_env(cmd: &mut Command, home: &Path) {
    let xdg = home.join(".config");
    cmd.env("HOME", home)
        .env("XDG_CONFIG_HOME", &xdg)
        .env("BORG_PASSPHRASE", "")
        .env("BORG_UNKNOWN_UNENCRYPTED_REPO_ACCESS_IS_OK", "yes")
        .env("BORG_RELOCATED_REPO_ACCESS_IS_OK", "yes");
}

fn borg(args: &[&str], home: &Path) -> Command {
    let mut cmd = Command::new("borg");
    cmd.args(args);
    apply_env(&mut cmd, home);
    cmd
}

#[test]
fn e2e_backup_and_list_flow() -> Result<(), Box<dyn std::error::Error>> {
    if !ensure_borg_available() {
        eprintln!("Skipping: borg not available in PATH");
        return Ok(());
    }

    let temp = assert_fs::TempDir::new()?;
    let repo_path = temp.child("repo.borg");
    let data_dir = temp.child("data");
    data_dir.create_dir_all()?;
    let file = data_dir.child("hello.txt");
    file.write_str("hello world")?;

    // Init repository (unencrypted for test speed)
    let init = borg(
        &[
            "init",
            "--encryption",
            "none",
            repo_path.path().to_str().unwrap(),
        ],
        temp.path(),
    )
    .status()?;
    assert!(init.success(), "borg init failed");

    // Write config pointing to our temp repo
    let config_path = temp.child("config.toml");
    let config = format!(
        r#"borg_bin = "borg"
probe_ssh = false

[[repos]]
name = "local"
repo = "{}"

[[repos.backups]]
name = "test"
includes = ["{}"]
"#,
        repo_path.path().display(),
        data_dir.path().display()
    );
    config_path.write_str(&config)?;

    // Run backup via CLI (non-interactive)
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("borg-tool-rs"));
    apply_env(&mut cmd, temp.path());
    cmd.arg("--config")
        .arg(config_path.path())
        .arg("--repo")
        .arg("local")
        .arg("backup")
        .arg("test");

    cmd.assert().success();

    // Inspect archives directly via borg JSON
    let output = borg(
        &["list", "--json", repo_path.path().to_str().unwrap()],
        temp.path(),
    )
    .output()?;
    assert!(output.status.success(), "borg list failed");
    let parsed: BorgListResponse = serde_json::from_slice(&output.stdout)?;
    assert_eq!(parsed.archives.len(), 1, "expected exactly one archive");
    let archive_name = &parsed.archives[0].name;

    // Ensure CLI list prints the archive name
    let mut list_cmd = Command::new(assert_cmd::cargo::cargo_bin!("borg-tool-rs"));
    apply_env(&mut list_cmd, temp.path());
    list_cmd
        .arg("--config")
        .arg(config_path.path())
        .arg("--repo")
        .arg("local")
        .arg("list")
        .assert()
        .success()
        .stdout(contains(archive_name));

    // List files of the archive
    let mut files_cmd = Command::new(assert_cmd::cargo::cargo_bin!("borg-tool-rs"));
    apply_env(&mut files_cmd, temp.path());
    files_cmd
        .arg("--config")
        .arg(config_path.path())
        .arg("--repo")
        .arg("local")
        .args(["files", archive_name])
        .assert()
        .success()
        .stdout(contains("hello.txt"));

    temp.close()?;
    Ok(())
}

#[test]
#[ignore]
fn e2e_mount_flow() -> Result<(), Box<dyn std::error::Error>> {
    if !env_flag("BORG_TOOL_ENABLE_MOUNT_TESTS") {
        eprintln!("Skipping mount test: BORG_TOOL_ENABLE_MOUNT_TESTS not set");
        return Ok(());
    }
    if !ensure_borg_available() {
        eprintln!("Skipping: borg not available in PATH");
        return Ok(());
    }
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/fuse")
    {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("Skipping: /dev/fuse not available");
            return Ok(());
        }
        Err(e) => {
            eprintln!("Skipping: /dev/fuse not accessible ({e})");
            return Ok(());
        }
    }

    let temp = TempDir::new()?;
    let repo_path = temp.path().join("repo.borg");
    let data_dir = temp.path().join("data");
    std::fs::create_dir_all(&data_dir)?;
    std::fs::write(data_dir.join("hello.txt"), b"hello mount")?;

    let init = borg(
        &["init", "--encryption", "none", repo_path.to_str().unwrap()],
        temp.path(),
    )
    .status()?;
    assert!(init.success(), "borg init failed");

    let config_path = temp.path().join("config.toml");
    std::fs::write(
        &config_path,
        format!(
            r#"borg_bin = "borg"
probe_ssh = false

[[repos]]
name = "local"
repo = "{}"

[[repos.backups]]
name = "test"
includes = ["{}"]
"#,
            repo_path.display(),
            data_dir.display()
        ),
    )?;

    // Create archive via tool
    let mut backup_cmd = Command::new(assert_cmd::cargo::cargo_bin!("borg-tool-rs"));
    apply_env(&mut backup_cmd, temp.path());
    backup_cmd
        .arg("--config")
        .arg(&config_path)
        .arg("--repo")
        .arg("local")
        .arg("backup")
        .arg("test")
        .assert()
        .success();

    // Get archive name via borg json
    let output = borg(
        &["list", "--json", repo_path.to_str().unwrap()],
        temp.path(),
    )
    .output()?;
    assert!(output.status.success());
    let parsed: BorgListResponse = serde_json::from_slice(&output.stdout)?;
    let archive_name = &parsed.archives[0].name;

    let mountpoint = temp.path().join("mnt");
    std::fs::create_dir_all(&mountpoint)?;
    let mut mount_cmd = Command::new(assert_cmd::cargo::cargo_bin!("borg-tool-rs"));
    apply_env(&mut mount_cmd, temp.path());
    mount_cmd
        .arg("--config")
        .arg(&config_path)
        .arg("--repo")
        .arg("local")
        .arg("mount")
        .arg(archive_name)
        .arg("--target")
        .arg(&mountpoint)
        .assert()
        .success();

    // Read file through mount
    let contents = std::fs::read_to_string(mountpoint.join("hello.txt"))?;
    assert_eq!(contents, "hello mount");

    let mut umount_cmd = Command::new(assert_cmd::cargo::cargo_bin!("borg-tool-rs"));
    apply_env(&mut umount_cmd, temp.path());
    umount_cmd
        .arg("--config")
        .arg(&config_path)
        .arg("--repo")
        .arg("local")
        .arg("umount")
        .arg(&mountpoint)
        .assert()
        .success();

    // After umount, directory should still exist but no longer be mounted; reading should fail.
    let read_after = std::fs::read_to_string(mountpoint.join("hello.txt"));
    assert!(
        read_after.is_err(),
        "mountpoint still readable after umount"
    );

    Ok(())
}

#[test]
#[ignore]
fn e2e_ssh_repo_list() -> Result<(), Box<dyn std::error::Error>> {
    if !env_flag("BORG_TOOL_ENABLE_SSH_TESTS") {
        eprintln!("Skipping SSH test: BORG_TOOL_ENABLE_SSH_TESTS not set");
        return Ok(());
    }
    if !ensure_borg_available() {
        eprintln!("Skipping: borg not available in PATH");
        return Ok(());
    }

    let repo_url = match std::env::var("BORG_TOOL_SSH_REPO") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => {
            eprintln!("Skipping SSH test: BORG_TOOL_SSH_REPO not provided");
            return Ok(());
        }
    };

    let temp = TempDir::new()?;
    let config_path = temp.path().join("config.toml");
    std::fs::write(
        &config_path,
        format!(
            r#"borg_bin = "borg"
probe_ssh = true

[[repos]]
name = "remote"
repo = "{}"
"#,
            repo_url
        ),
    )?;

    let mut list_cmd = Command::new(assert_cmd::cargo::cargo_bin!("borg-tool-rs"));
    apply_env(&mut list_cmd, temp.path());
    list_cmd
        .arg("--config")
        .arg(&config_path)
        .arg("--repo")
        .arg("remote")
        .arg("list")
        .assert();

    Ok(())
}
