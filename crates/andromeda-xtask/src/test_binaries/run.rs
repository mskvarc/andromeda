//! Execute a single prebuilt suite binary directly through its libtest harness.
//! Running the compiled binary rather than re-driving `cargo test` preserves the
//! exact in-process test ordering the suites rely on, while skipping the compile.

use crate::error::XtaskError;
use std::{path::Path, process::Command};

/// Run one prebuilt suite binary with the same harness arguments and environment
/// `cargo test` would pass it, returning whether the suite passed.
///
/// # Errors
///
/// Returns [`XtaskError::Io`] if the binary cannot be spawned or waited on.
pub(crate) fn run_suite(
    binary: &Path,
    test_threads: u16,
    harness_args: &[String],
    env: &[(&str, String)],
    working_directory: &Path,
) -> Result<bool, XtaskError> {
    let status = Command::new(binary)
        .arg(format!("--test-threads={test_threads}"))
        .args(harness_args)
        .envs(env.iter().map(|(key, value)| (*key, value)))
        .current_dir(working_directory)
        .status()?;
    Ok(status.success())
}
