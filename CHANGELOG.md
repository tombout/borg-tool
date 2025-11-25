# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

- _

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

[Unreleased]: https://github.com/tombout/borg-tool-rs/compare/v0.1.0...HEAD

[v0.1.0]: https://github.com/tombout/borg-tool-rs/releases/tag/v0.1.0
