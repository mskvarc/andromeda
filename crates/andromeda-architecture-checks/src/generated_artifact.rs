//! Files a crate generates and the repository checks in.
//!
//! A checked-in copy that differs from what its generator produces fails the
//! check, so a change to the generator's source cannot land without the copies
//! it feeds. Running the check with the [`REGENERATE_VARIABLE`] environment
//! variable set writes the generated content instead of comparing it.

use std::{
    env,
    fs,
    io,
    path::{Path, PathBuf},
};
use thiserror::Error;

/// The environment variable that makes the check write what it generates.
pub const REGENERATE_VARIABLE: &str = "ANDROMEDA_REGENERATE";

/// Why a checked-in copy does not hold what is generated for it.
#[derive(Debug, Error)]
enum Drift {
    /// The copy differs; `line` is the first line that does, counted from 1.
    #[error(
        "{} differs from what is generated for it, first at line {line}; run the tests again with {REGENERATE_VARIABLE}=1 to write it, and check in the result",
        path.display()
    )]
    Stale {
        /// The copy's workspace-relative path.
        path: PathBuf,
        /// The first differing line.
        line: usize,
    },
    /// Regenerating could not write the copy.
    #[error("{} cannot be written", path.display())]
    Unwritable {
        /// The copy's workspace-relative path.
        path: PathBuf,
        /// Why the write failed.
        #[source]
        source: io::Error,
    },
}

/// Assert that the file at `relative_path` under `workspace_root` holds
/// exactly `generated`, or write it there when regenerating.
///
/// # Panics
///
/// Panics when the file differs from `generated`, or cannot be written.
pub fn assert_checked_in(workspace_root: &str, relative_path: &str, generated: &str) {
    let path = Path::new(workspace_root).join(relative_path);
    let checked_in = fs::read_to_string(&path).unwrap_or_default();
    let outcome = settle(&path, relative_path, &checked_in, generated);
    assert!(outcome.is_ok(), "{}", outcome.map_or_else(|drift| drift.to_string(), |()| String::new()));
}

/// Compare the checked-in content with the generated content, writing the
/// generated content when regenerating.
fn settle(path: &Path, relative_path: &str, checked_in: &str, generated: &str) -> Result<(), Drift> {
    if checked_in == generated {
        return Ok(());
    }
    if env::var_os(REGENERATE_VARIABLE).is_some() {
        return fs::write(path, generated).map_err(|source| Drift::Unwritable {
            path: PathBuf::from(relative_path),
            source,
        });
    }
    let line = checked_in
        .lines()
        .zip(generated.lines())
        .position(|(checked_in, generated)| checked_in != generated)
        .unwrap_or_else(|| checked_in.lines().count().min(generated.lines().count()))
        + 1;
    Err(Drift::Stale {
        path: PathBuf::from(relative_path),
        line,
    })
}

#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::generated_artifact::assert_checked_in;
    use std::{env, fs, process};

    fn scratch_file(name: &str, content: &str) -> (String, String) {
        let directory = env::temp_dir().join(format!("andromeda-generated-artifact-{}", process::id()));
        fs::create_dir_all(&directory).unwrap_or_else(|error| panic!("{error}"));
        fs::write(directory.join(name), content).unwrap_or_else(|error| panic!("{error}"));
        (directory.to_string_lossy().into_owned(), name.to_owned())
    }

    #[test]
    fn a_file_matching_what_is_generated_passes() {
        let (root, name) = scratch_file("whole.txt", "generated\n");
        assert_checked_in(&root, &name, "generated\n");
    }

    #[test]
    #[should_panic(expected = "first at line 2")]
    fn a_file_differing_from_what_is_generated_fails_naming_the_first_differing_line() {
        let (root, name) = scratch_file("stale.txt", "same\nold\n");
        assert_checked_in(&root, &name, "same\nnew\n");
    }
}
