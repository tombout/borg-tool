# borg-tool-rs

![CI](https://github.com/tombout/borg-tool-rs/actions/workflows/ci.yml/badge.svg)

Small Rust CLI to inspect Borg repositories, mount archives, and run backups from presets — interactively or scripted.

## Requirements
- Rust stable (build/test)
- Borg Backup (`borg`) on `PATH`
- FUSE (optional) for mounting; `/dev/fuse` must exist

## Quickstart
1) Create a config, e.g. `config.toml` (default search: `$XDG_CONFIG_HOME/borg-tool/config.toml`, then `./config.toml`):

```toml
borg_bin = "borg"
mount_root = "/tmp/borg-tool-mounts"
probe_ssh = true

[[repos]]
name = "local"
repo = "/path/to/borg-repo"

[[repos.backups]]
name = "home"
includes = ["/home"]
excludes = ["/home/*/.cache"]
compression = "zstd,6"
exclude_caches = true
```

2) Run interactive mode: `cargo run --`. Without a subcommand you land in the menu (choose repo, run backups, mount archives, browse files).

3) Direct commands:
```
cargo run -- --config config.toml --repo local list
cargo run -- --repo local files <archive>
cargo run -- --repo local backup <preset>
```

## Tests
- Fast suite: `cargo test` (units + small happy-path E2E).
- Mount E2E (needs FUSE):
  - `BORG_TOOL_ENABLE_MOUNT_TESTS=1 cargo test e2e_mount_flow -- --ignored`
- SSH E2E (real repo required):
  - `BORG_TOOL_ENABLE_SSH_TESTS=1 BORG_TOOL_SSH_REPO=ssh://user@host/path cargo test e2e_ssh_repo_list -- --ignored`

## CI
- `.github/workflows/ci.yml` runs fmt, clippy, tests.
- Optional jobs (gate via repo variables):
  - `ENABLE_MOUNT_E2E=1` enables the mount test (Linux + FUSE required).
  - `ENABLE_SSH_E2E=1` enables the SSH test; expects secret `BORG_TOOL_SSH_REPO`.
- Security checks: `cargo audit` and `cargo deny` run in CI.
- CodeQL runs on push/PR and weekly.

## Release process
1) Local preflight: `cargo fmt && cargo clippy --all-targets --all-features && cargo test && cargo audit && cargo deny check`.
2) Build: `cargo build --release` (binary at `target/release/borg-tool-rs`).
3) Checksums: `cd target/release && sha256sum borg-tool-rs > SHA256SUMS`.
4) SBOM (optional but recommended): `cargo install cyclonedx-cargo --locked && cyclonedx-cargo --output-format json --output-file sbom.json`.
5) Tag and push (`vX.Y.Z`); GitHub Actions release workflow builds, generates checksum + SBOM, and uploads artifacts automatically.

## Config notes
- Interactive repo/backup wizards can create entries and save the config (comments are lost on save).
- Passphrase: cached once per run; set `BORG_PASSPHRASE` or `BORG_PASSCOMMAND` to avoid prompts.
