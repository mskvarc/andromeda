//! The `xtask etsi` command: fetch the pinned official ETSI NGSI-LD Robot
//! Framework suite, provision its robot virtualenv, apply the corrected-material
//! patch set, run it against a freshly built broker on a wiped database, and
//! compare two runs by official test identifier.

use crate::{cli, error::XtaskError};
use clap::{Args, Subcommand};
use diff::DiffArgs;
use run::RunArgs;
use std::path::{Path, PathBuf};

mod broker;
mod diff;
mod merge;
mod patch;
mod robot;
mod run;
mod suite;
mod venv;

/// Arguments to `xtask etsi`.
#[derive(Args)]
pub(crate) struct EtsiArgs {
    #[command(subcommand)]
    command: EtsiCommand,
}

impl EtsiArgs {
    /// Dispatch the selected ETSI subcommand.
    ///
    /// # Errors
    ///
    /// Propagates the selected subcommand's failure.
    pub(crate) fn execute(self) -> Result<(), XtaskError> {
        match self.command {
            EtsiCommand::Run(arguments) => arguments.execute(),
            EtsiCommand::Diff(arguments) => arguments.execute(),
        }
    }
}

/// The ETSI command's subcommands.
#[derive(Subcommand)]
enum EtsiCommand {
    /// Provision the pinned suite and run it against a freshly built broker.
    Run(RunArgs),
    /// Compare two robot runs by official test identifier.
    Diff(DiffArgs),
}

/// The pinned suite checkout, a disposable build artifact under `target/`, never
/// the andromeda repository.
pub(crate) fn checkout_dir() -> PathBuf {
    cli::repository_root().join("target").join("etsi-suite")
}

/// The crate-shipped ETSI resources: the patch sets and the robot pre-run modifier.
pub(crate) fn resource_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("etsi")
}
