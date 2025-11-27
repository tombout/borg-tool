# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

- _

## [v0.1.2] - 2025-11-27

### Added
- `just` recipes for common workflows (fmt/check/test/clippy) plus pre-commit, pre-push, and pre-release bundles.

### Changed
- Repo selector shows the host header and uses `Esc` to return to selection instead of exiting.
- Archive names now use ISO-8601 timestamps for consistent sorting and readability.

### Fixed
- Automatically skips backing up the repo itself when include paths cover its directory.

## [v0.1.1] - 2025-11-26

### Fixed
- Interactive startup now launches the setup wizard when no config file exists or is empty, instead of failing.
- Esc from the main menu now quits the application instead of looping back.

## [v0.1.0] - 2025-11-25

First public release of the Borg helper CLI.

- Interactive TUI to choose repositories, list archives, mount/unmount archives, browse files, and run backups.
- Configurable via `config.toml`: multiple repos, per-repo borg binary and mount root, backup presets with
  include/exclude lists, compression, `--exclude-caches`, `--one-file-system`, archive name prefix.
- CLI subcommands for scripted use: `list`, `files <archive>`, `backup <preset>`, `mount <archive> [--target]`,
  `umount <mountpoint>`, plus `interactive`.
- Archive mount helper with automatic mountpoint creation and sanity checks; default mount root under the system temp
  dir.
- Config auto-discovery (`$XDG_CONFIG_HOME/borg-tool/config.toml` then `./config.toml`) and ability to save modified
  configs.
- SSH probing toggle for remote repos; passphrase/env integration via borg (`BORG_PASSPHRASE`, `BORG_PASSCOMMAND`).
- MIT licensed.

[Unreleased]: https://github.com/tombout/borg-tool-rs/compare/v0.1.2...HEAD
[v0.1.2]: https://github.com/tombout/borg-tool-rs/releases/tag/v0.1.2
[v0.1.1]: https://github.com/tombout/borg-tool-rs/releases/tag/v0.1.1
[v0.1.0]: https://github.com/tombout/borg-tool-rs/releases/tag/v0.1.0
