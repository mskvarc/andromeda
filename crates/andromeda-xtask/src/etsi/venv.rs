//! The robot virtualenv the suite runs in: created once inside the checkout with
//! the suite's pinned Python requirements, and reused whenever it already exists.

use crate::{error::XtaskError, etsi};
use std::{io::ErrorKind, path::PathBuf, process::Command};

/// Path to the robot executable inside the checkout's virtualenv.
pub(crate) fn robot_binary() -> PathBuf {
    etsi::checkout_dir().join(".venv").join("bin").join("robot")
}

/// Path to the rebot executable inside the checkout's virtualenv. rebot is
/// robotframework's report combiner and ships with it, so [`ensure`] installs it
/// alongside `robot` with no extra requirement.
pub(crate) fn rebot_binary() -> PathBuf {
    etsi::checkout_dir().join(".venv").join("bin").join("rebot")
}

/// Ensure the virtualenv exists with robot installed, building it when absent.
///
/// The virtualenv lives at `.venv` inside the checkout. A hard reset of the
/// checkout only rewinds tracked files, so the untracked virtualenv survives
/// between runs and is reused; it is rebuilt only when its robot binary is missing.
///
/// # Errors
///
/// Returns [`XtaskError::PythonMissing`] when `python3` is not on `PATH`, and a
/// virtualenv-creation or requirement-install failure otherwise.
pub(crate) fn ensure() -> Result<(), XtaskError> {
    if robot_binary().is_file() {
        return Ok(());
    }
    create()
}

/// Create the virtualenv and install the suite's pinned Python requirements.
fn create() -> Result<(), XtaskError> {
    let checkout = etsi::checkout_dir();
    let venv = checkout.join(".venv");
    let created = match Command::new("python3").arg("-m").arg("venv").arg(&venv).status() {
        Ok(status) => status,
        Err(error) if error.kind() == ErrorKind::NotFound => return Err(XtaskError::PythonMissing),
        Err(error) => return Err(error.into()),
    };
    if !created.success() {
        return Err(XtaskError::VenvCreate);
    }
    let pip = venv.join("bin").join("pip");
    let installed = Command::new(pip)
        .args(["install", "--quiet", "--requirement"])
        .arg(checkout.join("requirements.txt"))
        .status()?;
    if installed.success() { Ok(()) } else { Err(XtaskError::PipInstall) }
}
