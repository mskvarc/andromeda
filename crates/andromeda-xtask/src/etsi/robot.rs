//! The robot invocation: resolving requested suite subtrees against the checkout
//! and assembling the command line. The suite's own `ErrorListener` is deliberately
//! not loaded -- its end-of-suite diff is quadratic in the accumulated error log
//! and reads as a hung run, while adding nothing `output.xml` does not already hold.

use crate::{
    error::XtaskError,
    etsi::{self, venv},
};
use std::{path::Path, process::Command};

/// The suite's own root under the checkout; a bare subtree resolves beneath it.
const SUITE_ROOT: &str = "TP/NGSI-LD";
/// Robot's console width, matching the recorded run record.
const CONSOLE_WIDTH: &str = "150";

/// Resolve the requested subtrees to checkout-relative paths, defaulting to the
/// whole suite when none are given.
///
/// # Errors
///
/// Returns [`XtaskError::UnknownSubtree`] for a subtree that exists nowhere.
pub(crate) fn resolve_targets(checkout: &Path, requested: &[String]) -> Result<Vec<String>, XtaskError> {
    if requested.is_empty() {
        return Ok(vec![SUITE_ROOT.to_owned()]);
    }
    requested.iter().map(|subtree| resolve_subtree(checkout, subtree)).collect()
}

/// Resolve one requested subtree, accepting a path relative to the checkout root,
/// one relative to the suite root, or a literal existing path.
fn resolve_subtree(checkout: &Path, requested: &str) -> Result<String, XtaskError> {
    let under_root = format!("{SUITE_ROOT}/{requested}");
    if checkout.join(requested).exists() {
        Ok(requested.to_owned())
    } else if checkout.join(&under_root).exists() {
        Ok(under_root)
    } else if Path::new(requested).exists() {
        Ok(requested.to_owned())
    } else {
        Err(XtaskError::UnknownSubtree(requested.to_owned()))
    }
}

/// Run robot against the resolved targets from within the checkout, returning its
/// exit code. A non-zero code means some test cases failed, which is a normal
/// outcome the caller reports rather than an error; only a failure to spawn robot
/// is an error.
///
/// # Errors
///
/// Returns an I/O error when robot cannot be spawned.
pub(crate) fn run(output_dir: &Path, timeout_seconds: u16, extra: &[String], targets: &[String]) -> Result<i32, XtaskError> {
    let modifier = etsi::resource_dir().join("EtsiTestTimeout.py");
    let args = robot_args(output_dir, timeout_seconds, &modifier, extra, targets);
    let status = Command::new(venv::robot_binary()).current_dir(etsi::checkout_dir()).args(&args).status()?;
    Ok(status.code().unwrap_or(1))
}

/// Assemble robot's arguments: output directory, console width, the per-test
/// timeout pre-run modifier, any caller-supplied arguments, then the targets.
fn robot_args(output_dir: &Path, timeout_seconds: u16, modifier: &Path, extra: &[String], targets: &[String]) -> Vec<String> {
    let mut args = vec![
        "--outputdir".to_owned(),
        output_dir.display().to_string(),
        "-W".to_owned(),
        CONSOLE_WIDTH.to_owned(),
        "--prerunmodifier".to_owned(),
        format!("{}:{timeout_seconds}", modifier.display()),
    ];
    args.extend(extra.iter().cloned());
    args.extend(targets.iter().cloned());
    args
}

#[cfg(test)]
mod tests {
    use crate::etsi::robot::{SUITE_ROOT, resolve_subtree, resolve_targets, robot_args};
    use std::{
        env,
        fs,
        path::{Path, PathBuf},
        process,
        thread,
    };

    #[test]
    fn no_requested_subtrees_default_to_the_whole_suite() {
        let targets = resolve_targets(Path::new("/nonexistent"), &[]).expect("the default needs no filesystem");
        assert_eq!(targets, vec![SUITE_ROOT.to_owned()]);
    }

    #[test]
    fn a_bare_subtree_resolves_beneath_the_suite_root() {
        let temporary = tempdir();
        fs::create_dir_all(temporary.join(SUITE_ROOT).join("001_02")).expect("create the suite subtree");
        let resolved = resolve_subtree(&temporary, "001_02").expect("the subtree resolves under the suite root");
        assert_eq!(resolved, format!("{SUITE_ROOT}/001_02"));
        fs::remove_dir_all(&temporary).expect("clean up the temporary tree");
    }

    #[test]
    fn a_checkout_relative_subtree_resolves_verbatim() {
        let temporary = tempdir();
        fs::create_dir_all(temporary.join("resources")).expect("create a checkout-relative path");
        let resolved = resolve_subtree(&temporary, "resources").expect("the path resolves verbatim");
        assert_eq!(resolved, "resources");
        fs::remove_dir_all(&temporary).expect("clean up the temporary tree");
    }

    #[test]
    fn an_unknown_subtree_is_rejected() {
        let temporary = tempdir();
        fs::create_dir_all(&temporary).expect("create the empty checkout");
        assert!(resolve_subtree(&temporary, "does/not/exist").is_err());
        fs::remove_dir_all(&temporary).expect("clean up the temporary tree");
    }

    #[test]
    fn robot_arguments_are_assembled_in_order() {
        let args = robot_args(
            Path::new("/out"),
            300,
            Path::new("/res/EtsiTestTimeout.py"),
            &["--suite".to_owned(), "Foo".to_owned()],
            &["TP/NGSI-LD".to_owned()],
        );
        assert_eq!(
            args,
            vec![
                "--outputdir".to_owned(),
                "/out".to_owned(),
                "-W".to_owned(),
                "150".to_owned(),
                "--prerunmodifier".to_owned(),
                "/res/EtsiTestTimeout.py:300".to_owned(),
                "--suite".to_owned(),
                "Foo".to_owned(),
                "TP/NGSI-LD".to_owned(),
            ]
        );
    }

    /// A unique temporary directory path under the system temp root for one test.
    fn tempdir() -> PathBuf {
        let unique = format!("andromeda-xtask-robot-{}-{:?}", process::id(), thread::current().id());
        env::temp_dir().join(unique)
    }
}
