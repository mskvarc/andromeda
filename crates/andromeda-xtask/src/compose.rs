//! Docker Compose lifecycle: bringing a stack up on wiped volumes and tearing it down.

use crate::error::XtaskError;
use std::{path::Path, process::Command};

/// Whether `docker compose up` builds the stack's images or reuses ones already
/// tagged into its `image:` names. CI builds the broker image once and tags it to
/// every stack, so the per-stack `up` must not rebuild it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageBuild {
    /// Pass `--build` so compose builds each service's image from its context.
    Build,
    /// Drop `--build` so compose reuses the pre-tagged `image:` for each service.
    Reuse,
}

/// Build the shared `docker compose --file <f> [--profile <p>]` prefix.
fn base_args(compose_file: &Path, profile: Option<&str>) -> Vec<String> {
    let mut args = vec!["compose".to_owned(), "--file".to_owned(), compose_file.display().to_string()];
    if let Some(profile) = profile {
        args.push("--profile".to_owned());
        args.push(profile.to_owned());
    }
    args
}

/// Bring the stack up and block on the compose healthchecks, building the images
/// first unless `image_build` asks to reuse the pre-tagged ones.
///
/// # Errors
///
/// Returns [`XtaskError::ComposeUp`] when `docker compose up` exits non-zero, and
/// [`XtaskError::Io`] when the process cannot be spawned.
pub(crate) fn up(repository_root: &Path, compose_file: &Path, profile: Option<&str>, image_build: ImageBuild) -> Result<(), XtaskError> {
    let mut args = base_args(compose_file, profile);
    args.push("up".to_owned());
    args.push("--detach".to_owned());
    if image_build == ImageBuild::Build {
        args.push("--build".to_owned());
    }
    args.push("--wait".to_owned());
    let status = Command::new("docker").args(&args).current_dir(repository_root).status()?;
    if status.success() { Ok(()) } else { Err(XtaskError::ComposeUp) }
}

/// Tear the stack down, removing volumes and orphaned containers. Best-effort: a
/// failure here (for example, nothing was up) must not mask the run's own result.
pub(crate) fn down(repository_root: &Path, compose_file: &Path, profile: Option<&str>) {
    let mut args = base_args(compose_file, profile);
    args.extend(["down", "--volumes", "--remove-orphans"].iter().map(|argument| (*argument).to_owned()));
    // Best-effort teardown: the exit status is deliberately discarded.
    let _outcome = Command::new("docker").args(&args).current_dir(repository_root).status();
}
