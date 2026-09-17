//! Andromeda's integration-test orchestrator: one command that drives a
//! docker-compose stack's lifecycle, wires the `ANDROMEDA_TEST_*` environment,
//! and runs the suites that stack owns.

use crate::{cli::Cli, error::XtaskError};
use clap::Parser;
use std::process::ExitCode;

mod certs;
mod cli;
mod compose;
mod error;
mod etsi;
mod lifecycle;
mod local_broker;
mod readiness;
mod stack;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Parse the command line and execute the selected command.
fn run() -> Result<(), XtaskError> {
    Cli::parse().execute()
}
