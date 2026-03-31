//! Command-line interface definition for the updater binary.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "codex-update-manager")]
#[command(about = "Local update manager for Codex Desktop on Linux")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
/// Top-level commands supported by the updater binary.
pub enum Commands {
    Daemon,
    CheckNow,
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Install a Debian package (.deb) with elevated privileges.
    InstallDeb {
        #[arg(long)]
        path: PathBuf,
    },
    /// Install an RPM package (.rpm) with elevated privileges.
    InstallRpm {
        #[arg(long)]
        path: PathBuf,
    },
    /// Install a Pacman package (.pkg.tar.zst) on Arch Linux with elevated privileges.
    InstallPacman {
        #[arg(long)]
        path: PathBuf,
    },
}
