//! The workspace-wide ceiling on production file length.
//!
//! The project rules split a file past ~400 production lines on sight and never lets
//! one past 600. This gate makes that measurable rather than a hand-kept
//! ledger: a file over the hard limit fails outright, and a file over the
//! signal line is tolerated only while it is on a baseline that can only shrink
//! — a new file crossing the signal line fails, and a baseline file that falls
//! back under it must be removed. Production lines are counted to the first
//! `#[cfg(test)]`, the same measure the project's own rule uses; integration
//! tests, `*tests.rs` files, build scripts, and the focused test crates are not
//! production and are not counted.

use crate::source_scan::rust_file_paths;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

/// The hard limit: a production source past this many lines fails outright.
pub const GOD_FILE_HARD_LIMIT: usize = 600;

/// The signal line: a production source past this is a split signal, tolerated
/// only while it is on the shrinking baseline.
pub const GOD_FILE_SIGNAL_LIMIT: usize = 400;

/// The production files between the signal line and the hard limit when the gate
/// was introduced.
///
/// Each entry is a file the project has yet to split. A new file over the signal
/// line fails the gate immediately; a file split back under the line must be
/// removed from this list, which a stale-entry check enforces, so the baseline
/// can only shrink toward empty.
pub const GOD_FILE_BASELINE: &[&str] = &[];

/// Assert that no production file exceeds the hard limit, and that the set of
/// files over the signal line has only shrunk from the baseline.
///
/// # Panics
///
/// Panics on a file over the hard limit, on a new file over the signal line, on
/// a baseline file that has fallen back under the signal line, or when a
/// production source cannot be read.
pub fn assert_no_god_files(workspace_root: &str, baseline: &[&str]) {
    let root = Path::new(workspace_root);
    let baseline: BTreeSet<&str> = baseline.iter().copied().collect();

    let mut over_hard = Vec::new();
    let mut new_over_signal = Vec::new();
    let mut over_signal_paths = BTreeSet::new();
    let mut unreadable = Vec::new();

    for path in production_source_paths(root) {
        let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) => {
                unreadable.push(format!("{relative} could not be read: {error}"));
                continue;
            }
        };
        let lines = production_line_count(&source);
        if lines > GOD_FILE_HARD_LIMIT {
            over_hard.push(format!("{relative}: {lines} production lines"));
            over_signal_paths.insert(relative);
        } else if lines > GOD_FILE_SIGNAL_LIMIT {
            if !baseline.contains(relative.as_str()) {
                new_over_signal.push(format!("{relative}: {lines} production lines"));
            }
            over_signal_paths.insert(relative);
        }
    }
    over_hard.sort();
    new_over_signal.sort();

    let stale: Vec<&str> = baseline.iter().copied().filter(|path| !over_signal_paths.contains(*path)).collect();

    assert!(unreadable.is_empty(), "production sources could not be read:\n{}", unreadable.join("\n"));
    assert!(
        over_hard.is_empty(),
        "these files exceed the {GOD_FILE_HARD_LIMIT}-line hard limit and must be split:\n{}",
        over_hard.join("\n")
    );
    assert!(
        new_over_signal.is_empty(),
        "these files crossed the {GOD_FILE_SIGNAL_LIMIT}-line signal without being on the baseline; split them (the baseline only shrinks):\n{}",
        new_over_signal.join("\n")
    );
    assert!(
        stale.is_empty(),
        "these baseline files are back under the {GOD_FILE_SIGNAL_LIMIT}-line signal; delete them from the baseline:\n{}",
        stale.join("\n")
    );
}

/// Every production `.rs` file below `src/`, `crates/`, and `lib/`.
///
/// Integration tests under `tests/`, `*tests.rs` files and build scripts are not
/// production code, so the length rule does not measure them.
fn production_source_paths(root: &Path) -> Vec<PathBuf> {
    ["src", "crates", "lib"]
        .into_iter()
        .flat_map(|directory| rust_file_paths(&root.join(directory)))
        .filter(|path| {
            let relative = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
            !relative.contains("/tests/") && !relative.ends_with("build.rs") && !relative.ends_with("tests.rs")
        })
        .collect()
}

/// The number of lines a file ships before its first `#[cfg(test)]` line, or all
/// of them when it has none.
///
/// This is the project's own production-line measure: the count is taken to the
/// first `#[cfg(test)]` because inline unit tests live at the bottom of a file,
/// and a whole-file count would charge a module for the tests that exercise it.
fn production_line_count(source: &str) -> usize {
    match source.lines().position(|line| line.trim_start().starts_with("#[cfg(test)]")) {
        Some(index) => index,
        None => source.lines().count(),
    }
}

#[cfg(test)]
mod tests {
    use crate::god_file::production_line_count;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_file_is_measured_to_its_first_cfg_test() {
        let source = "line one\nline two\n#[cfg(test)]\nmod tests {\n}\n";
        assert_eq!(production_line_count(source), 2);
    }

    #[test]
    fn a_file_without_tests_is_measured_whole() {
        let source = "line one\nline two\nline three\n";
        assert_eq!(production_line_count(source), 3);
    }

    #[test]
    fn an_indented_cfg_test_still_bounds_the_count() {
        let source = "line one\n    #[cfg(test)]\n    mod tests {}\n";
        assert_eq!(production_line_count(source), 1);
    }
}
