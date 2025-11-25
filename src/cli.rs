use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// CLI entry point.
#[derive(Parser, Debug)]
#[command(
    name = "borg-tool",
    version,
    about = "Minimal helper to inspect a Borg repository"
)]
pub struct Cli {
    /// Path to the config file
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    /// Which configured repo to use (by name)
    #[arg(short, long)]
    pub repo: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// List all archives in the configured repository
    List,
    /// List files inside a chosen archive
    Files {
        /// Archive name; if omitted, you will be prompted to choose
        archive: Option<String>,
    },
    /// Start interactive navigation
    Interactive,
    /// Mount an archive to a target path
    Mount {
        /// Archive name
        archive: String,
        /// Target mountpoint
        #[arg(short, long)]
        target: Option<PathBuf>,
    },
    /// Unmount a mounted archive (by mountpoint)
    Umount {
        /// Mountpoint to unmount
        mountpoint: PathBuf,
    },
    /// Create a configured backup
    Backup {
        /// Backup configuration name; if omitted, you will be prompted
        backup: Option<String>,
    },
}
