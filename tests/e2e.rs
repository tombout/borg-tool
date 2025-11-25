use std::path::Path;
use std::process::Command;

use assert_cmd::prelude::*;
use assert_fs::prelude::*;
use predicates::str::contains;
use serde::Deserialize;

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
    let mut cmd = Command::cargo_bin("borg-tool-rs")?;
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
    let mut list_cmd = Command::cargo_bin("borg-tool-rs")?;
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
    let mut files_cmd = Command::cargo_bin("borg-tool-rs")?;
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
