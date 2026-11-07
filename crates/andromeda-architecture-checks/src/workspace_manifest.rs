//! Reading every workspace manifest as a TOML document, for the gates that
//! check what the manifests declare.

use crate::dependencies::workspace_manifest_paths;
use std::{fs, path::Path};
use toml_edit::DocumentMut;

/// Run `check` over the root manifest and every package manifest, and return
/// the violations it reports, sorted and without duplicates.
///
/// A manifest that cannot be read or parsed is itself a violation, so a gate
/// never passes over a manifest it could not see. Each manifest is named by its
/// path relative to `workspace_root`, with `/` separators.
///
/// # Panics
///
/// Panics when an existing package directory cannot be listed.
pub(crate) fn check_workspace_manifests(workspace_root: &str, check: fn(&str, &DocumentMut, &mut Vec<String>)) -> Vec<String> {
    let root = Path::new(workspace_root);
    let mut violations = Vec::new();
    for path in workspace_manifest_paths(root) {
        let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        match fs::read_to_string(&path) {
            Ok(manifest) => match manifest.parse::<DocumentMut>() {
                Ok(document) => check(&relative, &document, &mut violations),
                Err(error) => violations.push(format!("{relative} does not parse as TOML: {error}")),
            },
            Err(error) => violations.push(format!("{relative} could not be read: {error}")),
        }
    }
    violations.sort();
    violations.dedup();
    violations
}
