//! The rule that every checked-in snapshot is one a test still takes.
//!
//! insta writes a snapshot under the name the test gives it, so a test that is
//! renamed or deleted leaves its file behind, and nothing fails until someone
//! runs insta with `--unreferenced=reject`. The gate reads the name argument of
//! every `assert_statement_snapshot!` and `assert_named_snapshot!` in a crate,
//! as a string literal, as a `format!` whose placeholders stand for any text,
//! or as a local name a `let` binds to either, and fails on a file in that
//! crate's `snapshots/` directory that none of those names can produce. A name
//! the sources do not spell that way cannot be read, so it fails too, and a
//! file a placeholder could produce is counted as referenced.

use crate::{
    orphan_snapshot::references::{References, collect_references},
    source_scan::{PACKAGE_DIRECTORIES, every_workspace_source, file_paths},
};
use std::{collections::BTreeMap, path::Path};

mod name_pattern;
mod references;

/// The directory of a crate that holds its snapshots.
const SNAPSHOT_DIRECTORY: &str = "snapshots";

/// The extension of a checked-in snapshot.
const SNAPSHOT_EXTENSION: &str = "snap";

/// Assert that every snapshot below a crate's `snapshots/` directory is taken
/// by a test of that crate, and that every name a test takes can be read.
///
/// # Panics
///
/// Panics when a snapshot file is referenced by no assertion of its crate,
/// when an assertion names its snapshot in a form the gate cannot read, or
/// when the workspace sources cannot be read or parsed.
pub fn assert_every_snapshot_is_referenced(workspace_root: &str) {
    let root = Path::new(workspace_root);
    let mut references: BTreeMap<String, References> = BTreeMap::new();
    for source in every_workspace_source(root) {
        let crate_directory = crate_directory(&source.workspace_path);
        collect_references(&source.workspace_path, &source.syntax, references.entry(crate_directory).or_default());
    }
    let unreadable: Vec<&String> = references.values().flat_map(|held| &held.unresolved).collect();
    assert!(
        unreadable.is_empty(),
        "these snapshot names are neither a string literal nor a `format!` nor a local bound to one, so the gate cannot tell which file they take:\n{}",
        unreadable.iter().map(|site| site.as_str()).collect::<Vec<_>>().join("\n")
    );
    let orphans = orphaned_snapshots(root, &references);
    assert!(
        orphans.is_empty(),
        "these snapshots are taken by no test of their crate; delete them or restore the test:\n{}",
        orphans.join("\n")
    );
}

/// The crate directory a workspace-relative source path belongs to, empty for
/// the root package.
fn crate_directory(workspace_path: &str) -> String {
    let mut components = workspace_path.split('/');
    if let (Some(directory), Some(package)) = (components.next(), components.next())
        && PACKAGE_DIRECTORIES.contains(&directory)
    {
        format!("{directory}/{package}")
    } else {
        String::new()
    }
}

/// The checked-in snapshots no assertion of their crate can produce, as
/// workspace-relative paths.
fn orphaned_snapshots(root: &Path, references: &BTreeMap<String, References>) -> Vec<String> {
    let mut orphans = Vec::new();
    for (crate_directory, held) in references {
        let snapshots = root.join(crate_directory).join(SNAPSHOT_DIRECTORY);
        for path in file_paths(&snapshots, is_snapshot) {
            let name = path
                .strip_prefix(&snapshots)
                .unwrap_or(&path)
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/");
            if !held.patterns.iter().any(|pattern| pattern.matches(&name)) {
                orphans.push(path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/"));
            }
        }
    }
    orphans.sort();
    orphans
}

/// Whether a file is a checked-in snapshot.
fn is_snapshot(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == SNAPSHOT_EXTENSION)
}

#[cfg(test)]
mod tests;
