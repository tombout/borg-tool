mod borg;
mod cli;
mod config;
mod ui;

use anyhow::{Context, Result};
use clap::Parser;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    let cmd = cli.command;

    let (mut config, config_path) =
        config::load_config_resolved(cli.config).with_context(|| {
            "Failed to load config (searched default path and ./config.toml when unset)".to_string()
        })?;

    let theme = ui::dialog_theme();
    let mut passphrase_cache: Option<String> = None;

    match cmd {
        None | Some(cli::Commands::Interactive) => loop {
            let repo_ctx = match ui::select_repo_ctx(
                &mut config,
                &config_path,
                cli.repo.as_deref(),
                cmd.as_ref(),
                &theme,
            )? {
                Some(r) => r,
                None => break,
            };
            match ui::run_interactive(&mut config, &config_path, repo_ctx, &mut passphrase_cache)? {
                ui::InteractiveOutcome::Quit => break,
                ui::InteractiveOutcome::ChangeRepo => continue,
            }
        },
        Some(cli::Commands::List) => {
            let repo_ctx = ui::select_repo_ctx(
                &mut config,
                &config_path,
                cli.repo.as_deref(),
                cmd.as_ref(),
                &theme,
            )?
            .ok_or_else(|| anyhow::anyhow!("No repository selected"))?;
            let pass = borg::ensure_passphrase_cached(&mut passphrase_cache, &repo_ctx)?;
            let archives = borg::list_archives(&repo_ctx, pass.as_deref())?;
            ui::print_archives(&archives);
        }
        Some(cli::Commands::Files { ref archive }) => {
            let repo_ctx = ui::select_repo_ctx(
                &mut config,
                &config_path,
                cli.repo.as_deref(),
                cmd.as_ref(),
                &theme,
            )?
            .ok_or_else(|| anyhow::anyhow!("No repository selected"))?;
            let pass = borg::ensure_passphrase_cached(&mut passphrase_cache, &repo_ctx)?;
            let archives = borg::list_archives(&repo_ctx, pass.as_deref())?;
            let selected = match archive {
                Some(name) => archives
                    .iter()
                    .find(|a| a.name == *name)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("Archive '{}' not found", name))?,
                None => match ui::select_archive(&archives, &theme)? {
                    Some(a) => a,
                    None => return Ok(()),
                },
            };
            let items = borg::list_items(&repo_ctx, &selected.name, pass.as_deref())?;
            ui::print_items(&items);
        }
        Some(cli::Commands::Mount {
            ref archive,
            ref target,
        }) => {
            let repo_ctx = ui::select_repo_ctx(
                &mut config,
                &config_path,
                cli.repo.as_deref(),
                cmd.as_ref(),
                &theme,
            )?
            .ok_or_else(|| anyhow::anyhow!("No repository selected"))?;
            borg::ensure_mount_available(&repo_ctx)?;
            let pass = borg::ensure_passphrase_cached(&mut passphrase_cache, &repo_ctx)?;
            let mountpoint = target
                .clone()
                .unwrap_or_else(|| borg::default_mountpoint(&repo_ctx, archive));
            borg::mount_archive(&repo_ctx, archive, &mountpoint, pass.as_deref())?;
            println!("Mounted {} at {}", archive, mountpoint.display());
        }
        Some(cli::Commands::Umount { ref mountpoint }) => {
            let repo_ctx = ui::select_repo_ctx(
                &mut config,
                &config_path,
                cli.repo.as_deref(),
                cmd.as_ref(),
                &theme,
            )?
            .ok_or_else(|| anyhow::anyhow!("No repository selected"))?;
            let pass = borg::ensure_passphrase_cached(&mut passphrase_cache, &repo_ctx)?;
            borg::umount_archive(&repo_ctx, mountpoint, pass.as_deref())?;
            println!("Unmounted {}", mountpoint.display());
        }
        Some(cli::Commands::Backup { ref backup }) => {
            let repo_ctx = ui::select_repo_ctx(
                &mut config,
                &config_path,
                cli.repo.as_deref(),
                cmd.as_ref(),
                &theme,
            )?
            .ok_or_else(|| anyhow::anyhow!("No repository selected"))?;
            let pass = borg::ensure_passphrase_cached(&mut passphrase_cache, &repo_ctx)?;
            let preset = if let Some(name) = backup {
                repo_ctx
                    .backups
                    .iter()
                    .find(|b| b.name == *name)
                    .cloned()
                    .ok_or_else(|| {
                        let names: Vec<&str> =
                            repo_ctx.backups.iter().map(|b| b.name.as_str()).collect();
                        anyhow::anyhow!(
                            "Backup '{}' not found. Available: {}",
                            name,
                            names.join(", ")
                        )
                    })?
            } else {
                match ui::select_backup(&repo_ctx.backups, &theme)? {
                    ui::BackupChoice::Preset(p) => p,
                    _ => return Ok(()),
                }
            };

            borg::run_backup(&repo_ctx, &preset, pass.as_deref())?;
        }
    }

    Ok(())
}
