# borg-tool-rs

Kleines CLI, um Borg-Repositories interaktiv oder skriptbar zu inspizieren, zu mounten und Backups aus Presets zu fahren.

## Voraussetzungen
- Rust stable (Build/Test)
- Borg Backup (`borg`) im `PATH`
- FUSE (optional) für Mount-Funktion; `/dev/fuse` muss verfügbar sein

## Schnellstart
1) Config anlegen, z.B. `config.toml` im Projektverzeichnis (oder `$XDG_CONFIG_HOME/borg-tool/config.toml`):

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

2) Starten: `cargo run --` (öffnet interaktiven Modus). Ohne Subcommand landet man im Menü; dort können Repos gewählt, Backups gestartet, Archive gemountet werden.

3) Direktbefehle:
```
cargo run -- --config config.toml --repo local list
cargo run -- --repo local files <archive>
cargo run -- --repo local backup <preset>
```

## Tests
- Schnelle Suite: `cargo test` (Unit + kleiner E2E-Happy-Path).
- Mount-E2E (benötigt FUSE):
  - `BORG_TOOL_ENABLE_MOUNT_TESTS=1 cargo test e2e_mount_flow -- --ignored`
- SSH-E2E (gegen echtes Repo):
  - `BORG_TOOL_ENABLE_SSH_TESTS=1 BORG_TOOL_SSH_REPO=ssh://user@host/path cargo test e2e_ssh_repo_list -- --ignored`

## CI
- `.github/workflows/ci.yml`: Basis-Job für fmt/clippy/tests.
- Optionale Jobs (per Repository-Variable steuerbar):
  - `ENABLE_MOUNT_E2E=1` schaltet Mount-Test ein (Linux, FUSE nötig).
  - `ENABLE_SSH_E2E=1` schaltet SSH-Test ein; erwartet Secret `BORG_TOOL_SSH_REPO`.

## Hinweise zur Config
- Repo-/Backup-Wizards im interaktiven Modus können neue Repos/Preset anlegen und die Config speichern (Kommentare gehen dabei verloren).
- Passphrase: wird einmal pro Lauf gecacht; `BORG_PASSPHRASE` oder `BORG_PASSCOMMAND` verhindern Prompts.
