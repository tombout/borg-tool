use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use dialoguer::{Confirm, Input, Select, console::Term, theme::ColorfulTheme};
use rpassword::prompt_password;

use crate::borg::{
    BorgArchive, BorgItem, default_mountpoint, ensure_mount_available, ensure_passphrase_cached,
    init_repo, list_archives, list_items, mount_archive, repo_status, run_backup, umount_archive,
};
use crate::config::{
    BackupConfig, Config, RepoConfig, RepoCtx, RepoStatus, default_borg_bin, default_mount_root,
    save_config, status_label,
};

#[derive(Debug, Clone, Copy)]
pub enum ArchiveAction {
    Browse,
    Mount,
    UnmountCurrent,
    Back,
}

#[derive(Debug, Clone, Copy)]
pub enum MainAction {
    Archives,
    Backups,
    BackRepo,
    Quit,
}

fn short_hostname() -> String {
    if let Ok(env) = std::env::var("HOSTNAME")
        && !env.trim().is_empty()
    {
        return env;
    }
    match Command::new("hostname").arg("-s").output() {
        Ok(out) if out.status.success() => {
            let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if raw.is_empty() {
                "unknown".to_string()
            } else {
                raw
            }
        }
        _ => "unknown".to_string(),
    }
}

fn show_step(title: &str, lines: &[String]) -> Result<()> {
    let term = Term::stdout();
    term.clear_screen()?;
    term.write_line(title)?;
    if !lines.is_empty() {
        for line in lines {
            term.write_line(line)?;
        }
        term.write_line("")?;
    }
    Ok(())
}

fn show_error_and_wait(message: &str) {
    let term = Term::stdout();
    let _ = term.write_line(message);
    let _ = term.write_line("Press Enter to continue...");
    let _ = term.read_line();
}

fn show_step_with_ctx(
    title: &str,
    lines: &[String],
    host: &str,
    repo: &RepoCtx,
    mount_available: bool,
    mount: Option<&MountInfo>,
) -> Result<()> {
    let term = Term::stdout();
    term.clear_screen()?;
    let mount_line = if mount_available {
        match mount {
            Some(m) => format!("Mount: {} @ {}", m.archive, m.mountpoint.display()),
            None => "Mount: none".to_string(),
        }
    } else {
        "Mount: unavailable".to_string()
    };
    term.write_line(&format!(
        "Host: {} | Repo: {} ({}) | {}",
        host, repo.name, repo.repo, mount_line
    ))?;
    term.write_line("")?;
    term.write_line(title)?;
    if !lines.is_empty() {
        for line in lines {
            term.write_line(line)?;
        }
        term.write_line("")?;
    }
    Ok(())
}

pub fn dialog_theme() -> ColorfulTheme {
    ColorfulTheme::default()
}

pub fn select_archive_action(
    theme: &ColorfulTheme,
    has_mount: bool,
    mount_available: bool,
) -> Result<ArchiveAction> {
    let mut options = vec!["Browse files"];
    if mount_available {
        options.push("Mount");
    }
    if has_mount {
        options.push("Unmount current");
    }
    options.push("Back");

    let choice = Select::with_theme(theme)
        .with_prompt("Action (Enter)")
        .items(&options)
        .default(0)
        .interact_opt()?;

    let action = match choice {
        Some(idx) => {
            let label = options[idx];
            match label {
                "Browse files" => ArchiveAction::Browse,
                "Mount" => ArchiveAction::Mount,
                "Unmount current" => ArchiveAction::UnmountCurrent,
                _ => ArchiveAction::Back,
            }
        }
        None => ArchiveAction::Back,
    };
    Ok(action)
}

pub fn select_main_action(theme: &ColorfulTheme) -> Result<MainAction> {
    let options = ["Archives", "Backups", "Change repository", "Quit"];
    let choice = Select::with_theme(theme)
        .with_prompt("What do you want to do?")
        .items(&options)
        .default(0)
        .interact_opt()?;

    let action = match choice {
        Some(0) => MainAction::Archives,
        Some(1) => MainAction::Backups,
        Some(2) => MainAction::BackRepo,
        None => MainAction::BackRepo, // Esc -> go back one level
        _ => MainAction::Quit,
    };
    Ok(action)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractiveOutcome {
    Quit,
    ChangeRepo,
}

#[derive(Debug, Clone)]
pub enum BackupChoice {
    Preset(BackupConfig),
    CreateNew,
    Back,
}

pub fn select_repo_ctx(
    cfg: &mut Config,
    config_path: &Path,
    cli_repo: Option<&str>,
    cmd: Option<&crate::cli::Commands>,
    theme: &ColorfulTheme,
) -> Result<Option<RepoCtx>> {
    let mut repos = build_repo_list(cfg);
    if repos.is_empty() {
        anyhow::bail!("No repositories configured in config file");
    }

    // Single repo fast path
    if repos.len() == 1 {
        let ctx = repos.into_iter().next().unwrap();
        if let Some(req) = cli_repo
            && req != ctx.name
        {
            anyhow::bail!(
                "Repo '{}' not found. Only available repo: {}",
                req,
                ctx.name
            );
        }
        return Ok(Some(ctx));
    }

    // multiple repos
    if let Some(req) = cli_repo {
        if let Some(found) = repos.iter().find(|r| r.name == req) {
            return ensure_repo_available(found.clone(), cmd).map(Some);
        }
        let names = repos.iter().map(|r| r.name.as_str()).collect::<Vec<_>>();
        anyhow::bail!("Repo '{}' not found. Available: {}", req, names.join(", "));
    }

    // interactive selection allowed only for interactive commands
    match cmd {
        None
        | Some(crate::cli::Commands::Interactive)
        | Some(crate::cli::Commands::Backup { .. }) => loop {
            show_step("Choose repository", &[])?;
            let mut labels: Vec<String> = repos
                .iter()
                .map(|r| format!("{}  ({}) [{}]", r.name, r.repo, status_label(r.status)))
                .collect();
            labels.push("Set up new repository".to_string());
            labels.push("Quit".to_string());

            let choice = Select::with_theme(theme)
                .with_prompt("Choose repository (Esc/Quit to exit)")
                .items(&labels)
                .default(0)
                .interact_opt()?;

            match choice {
                Some(idx) if idx < repos.len() => {
                    return ensure_repo_available(repos[idx].clone(), cmd).map(Some);
                }
                Some(idx) if idx == repos.len() => {
                    if let Some(created) = setup_new_repo_wizard(cfg, config_path, theme)? {
                        return Ok(Some(created));
                    } else {
                        // user cancelled or failed; rebuild list in case config changed elsewhere
                        repos = build_repo_list(cfg);
                        continue;
                    }
                }
                _ => return Ok(None),
            }
        },
        _ => {
            let names = repos.iter().map(|r| r.name.as_str()).collect::<Vec<_>>();
            anyhow::bail!(
                "Multiple repos configured. Please choose with --repo <name>. Available: {}",
                names.join(", ")
            );
        }
    }
}

pub fn select_archive(
    archives: &[BorgArchive],
    theme: &ColorfulTheme,
) -> Result<Option<BorgArchive>> {
    let mut items: Vec<String> = archives
        .iter()
        .map(|a| {
            let time = a.time_utc.as_deref().unwrap_or("-");
            format!("{}  [{}]", a.name, time)
        })
        .collect();
    items.push("Back".to_string());

    let selection = Select::with_theme(theme)
        .with_prompt("Choose archive (Back to return)")
        .items(&items)
        .default(0)
        .interact_opt()?;

    Ok(match selection {
        Some(idx) if idx < archives.len() => Some(archives[idx].clone()),
        _ => None,
    })
}

pub fn select_item(items: &[BorgItem], theme: &ColorfulTheme) -> Result<Option<BorgItem>> {
    let mut display: Vec<String> = items
        .iter()
        .map(|i| format!("{:<6} {}", i.item_type.as_deref().unwrap_or(""), i.path))
        .collect();
    display.push("Back".to_string());

    let selection = Select::with_theme(theme)
        .with_prompt("Choose file (Back to return)")
        .items(&display)
        .default(0)
        .interact_opt()?;

    Ok(match selection {
        Some(idx) if idx < items.len() => Some(items[idx].clone()),
        _ => None,
    })
}

pub fn select_backup(backups: &[BackupConfig], theme: &ColorfulTheme) -> Result<BackupChoice> {
    let mut labels: Vec<String> = backups
        .iter()
        .map(|b| format!("{}  ({} includes)", b.name, b.includes.len()))
        .collect();
    labels.push("Create new backup preset".to_string());
    labels.push("Back".to_string());

    let selection = Select::with_theme(theme)
        .with_prompt("Choose backup preset (Back to return)")
        .items(&labels)
        .default(0)
        .interact_opt()?;

    Ok(match selection {
        Some(idx) if idx < backups.len() => BackupChoice::Preset(backups[idx].clone()),
        Some(idx) if idx == backups.len() => BackupChoice::CreateNew,
        _ => BackupChoice::Back,
    })
}

fn build_repo_list(cfg: &Config) -> Vec<RepoCtx> {
    let mut repos: Vec<RepoCtx> = if !cfg.repos.is_empty() {
        cfg.repos
            .iter()
            .map(|r| RepoCtx {
                name: r.name.clone(),
                repo: r.repo.clone(),
                borg_bin: r.borg_bin.clone().unwrap_or_else(|| cfg.borg_bin.clone()),
                mount_root: r
                    .mount_root
                    .clone()
                    .unwrap_or_else(|| cfg.mount_root.clone()),
                backups: r.backups.clone(),
                status: RepoStatus::Unknown,
            })
            .collect()
    } else if let Some(repo) = &cfg.repo {
        vec![RepoCtx {
            name: "default".to_string(),
            repo: repo.clone(),
            borg_bin: cfg.borg_bin.clone(),
            mount_root: cfg.mount_root.clone(),
            backups: Vec::new(),
            status: RepoStatus::Unknown,
        }]
    } else {
        Vec::new()
    };

    if repos.is_empty() {
        return repos;
    }

    let total = repos.len();

    for (idx, repo) in repos.iter_mut().enumerate() {
        let pb = indicatif::ProgressBar::new_spinner();
        pb.set_style(
            indicatif::ProgressStyle::with_template("{spinner:.green} {msg}").expect("template"),
        );
        pb.set_message(format!(
            "({}/{}) Probing {} ({})",
            idx + 1,
            total,
            repo.name,
            repo.repo
        ));
        pb.enable_steady_tick(std::time::Duration::from_millis(120));

        repo.status = repo_status(&repo.repo, cfg.probe_ssh);

        pb.finish_with_message(format!(
            "[{}] {} ({})",
            status_label(repo.status),
            repo.name,
            repo.repo
        ));
    }

    repos
}

fn ensure_repo_available(repo: RepoCtx, cmd: Option<&crate::cli::Commands>) -> Result<RepoCtx> {
    match repo.status {
        RepoStatus::MissingLocal => match cmd {
            None | Some(crate::cli::Commands::Interactive) => {
                println!(
                    "Warning: repo '{}' path '{}' not found.",
                    repo.name, repo.repo
                );
            }
            _ => {
                anyhow::bail!("Repo '{}' path '{}' not found.", repo.name, repo.repo);
            }
        },
        RepoStatus::RemoteAuthNeeded => match cmd {
            None | Some(crate::cli::Commands::Interactive) => {
                println!(
                    "Warning: repo '{}' seems to require SSH auth (no key?).",
                    repo.name
                );
            }
            _ => {
                println!(
                    "Warning: repo '{}' may require SSH auth; proceeding.",
                    repo.name
                );
            }
        },
        _ => {}
    }
    Ok(repo)
}

pub fn run_interactive(
    cfg: &mut Config,
    config_path: &Path,
    mut repo: RepoCtx,
    passphrase_cache: &mut Option<String>,
) -> Result<InteractiveOutcome> {
    let theme = dialog_theme();
    let mut mount_state: Option<MountInfo> = None;
    let mount_available = ensure_mount_available(&repo).unwrap_or(false);
    let host = short_hostname();

    loop {
        let mut main_info = vec![format!("Repo: {} ({})", repo.name, repo.repo)];
        main_info.push(if mount_available {
            match &mount_state {
                Some(m) => format!("Mounted: {} @ {}", m.archive, m.mountpoint.display()),
                None => "Mount available (none mounted)".to_string(),
            }
        } else {
            "Mount unavailable (no FUSE support detected)".to_string()
        });
        show_step_with_ctx(
            "Main menu",
            &main_info,
            &host,
            &repo,
            mount_available,
            mount_state.as_ref(),
        )?;

        match select_main_action(&theme)? {
            MainAction::Archives => {
                let pass = ensure_passphrase_cached(passphrase_cache, &repo)?;
                let archives = list_archives(&repo, pass.as_deref())?;
                if archives.is_empty() {
                    println!("No archives found");
                    continue;
                }

                let mut archive_info = vec![
                    format!("Repo: {} ({})", repo.name, repo.repo),
                    format!("Archives found: {}", archives.len()),
                ];
                archive_info.push(if mount_available {
                    match &mount_state {
                        Some(m) => format!("Mounted: {} @ {}", m.archive, m.mountpoint.display()),
                        None => "Mount available (none mounted)".to_string(),
                    }
                } else {
                    "Mount unavailable (no FUSE support detected)".to_string()
                });
                show_step_with_ctx(
                    "Archives",
                    &archive_info,
                    &host,
                    &repo,
                    mount_available,
                    mount_state.as_ref(),
                )?;

                let archive = match select_archive(&archives, &theme)? {
                    Some(a) => a,
                    None => continue,
                };

                let mut action_info = vec![
                    format!("Repo: {} ({})", repo.name, repo.repo),
                    format!(
                        "Archive: {} [{}]",
                        archive.name,
                        archive.time_utc.as_deref().unwrap_or("-")
                    ),
                ];
                if let Some(m) = &mount_state {
                    action_info.push(format!(
                        "Mounted: {} @ {}",
                        m.archive,
                        m.mountpoint.display()
                    ));
                }
                show_step_with_ctx(
                    "Archive action",
                    &action_info,
                    &host,
                    &repo,
                    mount_available,
                    mount_state.as_ref(),
                )?;

                match select_archive_action(&theme, mount_state.is_some(), mount_available)? {
                    ArchiveAction::Browse => {
                        browse_files(
                            &host,
                            &repo,
                            &archive,
                            pass.as_deref(),
                            &theme,
                            mount_available,
                            mount_state.as_ref(),
                        )?;
                    }
                    ArchiveAction::Mount => {
                        if let Some(active) = &mount_state {
                            if Confirm::with_theme(&theme)
                                .with_prompt(format!(
                                    "Unmount current ({}) before mounting new one?",
                                    active.mountpoint.display()
                                ))
                                .default(true)
                                .interact()?
                            {
                                umount_archive(&repo, &active.mountpoint, pass.as_deref())?;
                                println!("Unmounted {}", active.mountpoint.display());
                            } else {
                                continue;
                            }
                        }

                        let default_mp = default_mountpoint(&repo, &archive.name);
                        let target: String = Input::with_theme(&theme)
                            .with_prompt("Mountpoint")
                            .default(default_mp.display().to_string())
                            .interact_text()?;
                        let target_path = PathBuf::from(target);
                        mount_archive(&repo, &archive.name, &target_path, pass.as_deref())?;
                        println!("Mounted {} at {}", archive.name, target_path.display());
                        mount_state = Some(crate::ui::MountInfo {
                            archive: archive.name.clone(),
                            mountpoint: target_path,
                        });
                    }
                    ArchiveAction::Back => {}
                    ArchiveAction::UnmountCurrent => {
                        if let Some(active) = mount_state.take() {
                            umount_archive(&repo, &active.mountpoint, pass.as_deref())?;
                            println!("Unmounted {}", active.mountpoint.display());
                        }
                    }
                }
            }
            MainAction::Backups => {
                let pass = ensure_passphrase_cached(passphrase_cache, &repo)?;
                if repo.backups.is_empty() {
                    show_step_with_ctx(
                        "Backup presets",
                        &[format!("Repo: {} ({})", repo.name, repo.repo)],
                        &host,
                        &repo,
                        mount_available,
                        mount_state.as_ref(),
                    )?;

                    if Confirm::with_theme(&theme)
                        .with_prompt("No backups configured. Create one now?")
                        .default(true)
                        .interact()?
                        && let Some(new_preset) = setup_backup_preset_wizard(&repo, &theme)?
                    {
                        add_preset_to_config(cfg, &repo.name, new_preset.clone());
                        repo.backups.push(new_preset);
                        maybe_save_config(cfg, config_path, &theme)?;
                    }
                    continue;
                }
                show_step_with_ctx(
                    "Backup presets",
                    &[format!("Repo: {} ({})", repo.name, repo.repo)],
                    &host,
                    &repo,
                    mount_available,
                    mount_state.as_ref(),
                )?;
                let preset = match select_backup(&repo.backups, &theme)? {
                    BackupChoice::Preset(p) => p,
                    BackupChoice::CreateNew => {
                        if let Some(new_preset) = setup_backup_preset_wizard(&repo, &theme)? {
                            add_preset_to_config(cfg, &repo.name, new_preset.clone());
                            repo.backups.push(new_preset.clone());
                            maybe_save_config(cfg, config_path, &theme)?;
                            new_preset
                        } else {
                            continue;
                        }
                    }
                    BackupChoice::Back => continue,
                };
                if let Err(err) = run_backup(&repo, &preset, pass.as_deref()) {
                    show_error_and_wait(&format!("Backup failed: {err}"));
                }
            }
            MainAction::BackRepo => return Ok(InteractiveOutcome::ChangeRepo),
            MainAction::Quit => return Ok(InteractiveOutcome::Quit),
        }
    }
}

pub fn browse_files(
    host: &str,
    repo: &RepoCtx,
    archive: &BorgArchive,
    passphrase: Option<&str>,
    theme: &ColorfulTheme,
    mount_available: bool,
    mount: Option<&MountInfo>,
) -> Result<()> {
    loop {
        show_step_with_ctx(
            "Browse files",
            &[
                format!("Repo: {} ({})", repo.name, repo.repo),
                format!(
                    "Archive: {} [{}]",
                    archive.name,
                    archive.time_utc.as_deref().unwrap_or("-")
                ),
            ],
            host,
            repo,
            mount_available,
            mount,
        )?;
        let items = list_items(repo, &archive.name, passphrase)?;
        if items.is_empty() {
            println!("No files in archive {}", archive.name);
            return Ok(());
        }

        let item = match select_item(&items, theme)? {
            Some(i) => i,
            None => return Ok(()), // back to archive list
        };

        if Confirm::with_theme(theme)
            .with_prompt(format!(
                "Extract '{}' from '{}' to current directory?",
                item.path, archive.name
            ))
            .default(false)
            .interact()?
        {
            let dest: String = Input::with_theme(theme)
                .with_prompt("Destination directory")
                .default(".".to_string())
                .interact_text()?;
            crate::borg::extract_file(repo, &archive.name, &item.path, &dest, passphrase)?;
            println!("Extracted to {}", dest);
        }
    }
}

fn migrate_legacy_repo(cfg: &mut Config) {
    if cfg.repos.is_empty()
        && let Some(legacy) = cfg.repo.take()
    {
        cfg.repos.push(RepoConfig {
            name: "default".to_string(),
            repo: legacy,
            borg_bin: None,
            mount_root: None,
            backups: Vec::new(),
        });
    }
}

fn prompt_new_passphrase(label: &str) -> Result<String> {
    loop {
        let first = prompt_password(format!("{} (hidden): ", label))
            .context("Reading passphrase failed")?;
        let second = prompt_password("Confirm passphrase: ")
            .context("Reading passphrase confirmation failed")?;
        if first == second {
            return Ok(first);
        }
        println!("Passphrases did not match. Please try again.");
    }
}

fn setup_new_repo_wizard(
    cfg: &mut Config,
    config_path: &Path,
    theme: &ColorfulTheme,
) -> Result<Option<RepoCtx>> {
    migrate_legacy_repo(cfg);

    show_step("Set up new repository", &[])?;

    let name: String = Input::with_theme(theme)
        .with_prompt("Repository name (unique)")
        .validate_with(|input: &String| -> Result<(), &str> {
            if input.trim().is_empty() {
                return Err("Name cannot be empty");
            }
            Ok(())
        })
        .interact_text()?;

    if cfg.repos.iter().any(|r| r.name == name) {
        println!("Repo '{}' already exists. Aborting setup.", name);
        return Ok(None);
    }

    let repo_path: String = Input::with_theme(theme)
        .with_prompt("Repository path or SSH URL")
        .interact_text()?;

    let borg_bin_input: String = Input::with_theme(theme)
        .with_prompt("borg binary")
        .default(cfg.borg_bin.clone())
        .interact_text()?;

    let mount_root_input: String = Input::with_theme(theme)
        .with_prompt("Mount root (for mounts)")
        .default(cfg.mount_root.display().to_string())
        .interact_text()?;
    let mount_root = PathBuf::from(mount_root_input);

    let enc_options = ["repokey (encrypted, recommended)", "none (no encryption)"];
    let enc_choice = Select::with_theme(theme)
        .with_prompt("Encryption mode")
        .items(&enc_options)
        .default(0)
        .interact_opt()?;
    let encryption = match enc_choice.unwrap_or(0) {
        0 => "repokey".to_string(),
        _ => "none".to_string(),
    };

    let summary = vec![
        format!("Name: {}", name),
        format!("Repo: {}", repo_path),
        format!("borg bin: {}", borg_bin_input),
        format!("mount root: {}", mount_root.display()),
        format!("encryption: {}", encryption),
    ];
    show_step("Review new repository", &summary)?;

    if !Confirm::with_theme(theme)
        .with_prompt("Proceed to initialize this repository?")
        .default(true)
        .interact()?
    {
        return Ok(None);
    }

    let passphrase = if encryption != "none" {
        Some(prompt_new_passphrase("New repo passphrase")?)
    } else {
        None
    };

    let ctx = RepoCtx {
        name: name.clone(),
        repo: repo_path.clone(),
        borg_bin: borg_bin_input.clone(),
        mount_root: mount_root.clone(),
        backups: Vec::new(),
        status: RepoStatus::Unknown,
    };

    if let Err(err) = init_repo(&ctx, &encryption, passphrase.as_deref()) {
        show_error_and_wait(&format!("Failed to initialize repo: {err}"));
        return Ok(None);
    }

    let mut ready_ctx = ctx.clone();
    ready_ctx.status = RepoStatus::Ok;

    // Add to in-memory config (always, so it is usable right away)
    cfg.repos.push(RepoConfig {
        name: name.clone(),
        repo: repo_path,
        borg_bin: if borg_bin_input == cfg.borg_bin || borg_bin_input == default_borg_bin() {
            None
        } else {
            Some(borg_bin_input)
        },
        mount_root: if mount_root == cfg.mount_root || mount_root == default_mount_root() {
            None
        } else {
            Some(mount_root)
        },
        backups: Vec::new(),
    });

    if Confirm::with_theme(theme)
        .with_prompt(format!(
            "Save updated config to {}? (comments will be lost)",
            config_path.display()
        ))
        .default(true)
        .interact()?
    {
        if let Err(err) = save_config(cfg, config_path) {
            show_error_and_wait(&format!("Failed to save config: {err}"));
        } else {
            println!("Config saved to {}", config_path.display());
        }
    } else {
        println!(
            "Config not saved. Repo '{}' is available for this session only.",
            name
        );
    }

    Ok(Some(ready_ctx))
}

fn parse_list(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn setup_backup_preset_wizard(
    repo: &RepoCtx,
    theme: &ColorfulTheme,
) -> Result<Option<BackupConfig>> {
    show_step("Create backup preset", &[])?;

    let name: String = Input::with_theme(theme)
        .with_prompt("Preset name (unique)")
        .validate_with(|input: &String| -> Result<(), &str> {
            if input.trim().is_empty() {
                return Err("Name cannot be empty");
            }
            Ok(())
        })
        .interact_text()?;

    if repo.backups.iter().any(|b| b.name == name) {
        println!("Preset '{}' already exists. Aborting setup.", name);
        return Ok(None);
    }

    let includes_raw: String = Input::with_theme(theme)
        .with_prompt("Includes (comma-separated paths/patterns)")
        .interact_text()?;
    let includes = parse_list(&includes_raw);
    if includes.is_empty() {
        println!("You must provide at least one include.");
        return Ok(None);
    }

    let excludes_raw: String = Input::with_theme(theme)
        .with_prompt("Excludes (comma-separated, optional)")
        .default("".to_string())
        .interact_text()?;
    let excludes = parse_list(&excludes_raw);

    let compression: String = Input::with_theme(theme)
        .with_prompt("Compression (e.g. zstd,6; empty to skip)")
        .default("zstd,6".to_string())
        .interact_text()?;
    let compression_opt = if compression.trim().is_empty() {
        None
    } else {
        Some(compression)
    };

    let one_file_system = Confirm::with_theme(theme)
        .with_prompt("Stay on one file system? (--one-file-system)")
        .default(true)
        .interact()?;

    let exclude_caches = Confirm::with_theme(theme)
        .with_prompt("Exclude caches? (--exclude-caches)")
        .default(true)
        .interact()?;

    let archive_prefix_input: String = Input::with_theme(theme)
        .with_prompt("Archive name prefix (empty = use repo name)")
        .default("".to_string())
        .interact_text()?;
    let archive_prefix = if archive_prefix_input.trim().is_empty() {
        None
    } else {
        Some(archive_prefix_input.trim().to_string())
    };

    let summary = vec![
        format!("Repo: {}", repo.name),
        format!("Preset: {}", name),
        format!("Includes: {}", includes.join(", ")),
        format!(
            "Excludes: {}",
            if excludes.is_empty() {
                "(none)".to_string()
            } else {
                excludes.join(", ")
            }
        ),
        format!(
            "Compression: {}",
            compression_opt
                .clone()
                .unwrap_or_else(|| "(none)".to_string())
        ),
        format!("one_file_system: {}", one_file_system),
        format!("exclude_caches: {}", exclude_caches),
        format!(
            "archive_prefix: {}",
            archive_prefix
                .clone()
                .unwrap_or_else(|| "(repo name)".to_string())
        ),
    ];

    show_step("Review backup preset", &summary)?;

    if !Confirm::with_theme(theme)
        .with_prompt("Create this preset?")
        .default(true)
        .interact()?
    {
        return Ok(None);
    }

    Ok(Some(BackupConfig {
        name,
        includes,
        excludes,
        compression: compression_opt,
        one_file_system,
        exclude_caches,
        archive_prefix,
    }))
}

fn add_preset_to_config(cfg: &mut Config, repo_name: &str, preset: BackupConfig) {
    if let Some(r) = cfg.repos.iter_mut().find(|r| r.name == repo_name) {
        r.backups.push(preset);
    }
}

fn maybe_save_config(cfg: &Config, config_path: &Path, theme: &ColorfulTheme) -> Result<()> {
    if Confirm::with_theme(theme)
        .with_prompt(format!(
            "Save updated config to {}? (comments will be lost)",
            config_path.display()
        ))
        .default(true)
        .interact()?
    {
        if let Err(err) = save_config(cfg, config_path) {
            show_error_and_wait(&format!("Failed to save config: {err}"));
        } else {
            println!("Config saved to {}", config_path.display());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_list_splits_and_trims() {
        let res = parse_list("/home, /var/log ,,tmp");
        assert_eq!(res, vec!["/home", "/var/log", "tmp"]);
    }

    #[test]
    fn parse_list_handles_empty() {
        let res = parse_list("   ");
        assert!(res.is_empty());
    }
}

pub fn print_archives(archives: &[BorgArchive]) {
    if archives.is_empty() {
        println!("No archives found");
        return;
    }
    for arch in archives {
        let time = arch.time_utc.as_deref().unwrap_or("-");
        println!("{:<40} {}", arch.name, time);
    }
}

pub fn print_items(items: &[BorgItem]) {
    if items.is_empty() {
        println!("No files in archive");
        return;
    }

    for item in items {
        let kind = item.item_type.as_deref().unwrap_or("");
        println!("{:<8} {}", kind, item.path);
    }
}

#[derive(Debug, Clone)]
pub struct MountInfo {
    pub archive: String,
    pub mountpoint: PathBuf,
}
