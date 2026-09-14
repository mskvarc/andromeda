//! The workspace-wide ban on dumping-ground module names.

use crate::source_scan::rust_file_paths;
use std::path::Path;

/// Module names that describe no domain concept and are forbidden anywhere.
///
/// A module whose name does not say what it owns owns nothing: `utils`,
/// `helpers`, `common`, `shared`, `misc` are pure junk drawers, `core` names
/// the project rather than a concept, and `types` used as a name is the same
/// junk drawer spelled differently. Every item lives in the module named after
/// the thing it belongs to, so none of these names is ever the right one.
const DUMPING_GROUND_NAMES: &[&str] = &["common", "core", "helpers", "misc", "shared", "types", "utils"];

/// Assert that no production module is named after a dumping ground.
///
/// The scan covers every Rust source below `src/`, `crates/`, and `lib/`,
/// matching both a file stem (`utils.rs`) and a directory that carries the
/// concept through its `mod.rs` (`utils/mod.rs`). It reads only file names, so
/// it needs no parse and covers test crates as readily as production ones.
///
/// # Panics
///
/// Panics when a module uses a dumping-ground name, or when the source tree
/// cannot be listed.
pub fn assert_no_dumping_ground_modules(workspace_root: &str) {
    let root = Path::new(workspace_root);
    let mut violations = ["src", "crates", "lib"]
        .into_iter()
        .flat_map(|directory| rust_file_paths(&root.join(directory)))
        .filter_map(|path| {
            let concept = module_concept_name(&path)?;
            DUMPING_GROUND_NAMES.contains(&concept).then(|| {
                let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                format!("{relative} is named after a dumping ground; name the module after what it owns")
            })
        })
        .collect::<Vec<_>>();
    violations.sort();
    violations.dedup();
    assert!(violations.is_empty(), "dumping-ground module names:\n{}", violations.join("\n"));
}

/// The concept a module file names: its stem, or the directory it heads when it
/// is a `mod.rs`.
///
/// `lib.rs` and `main.rs` are crate entry points rather than concept modules,
/// so they name no concept of their own and are never a violation.
fn module_concept_name(path: &Path) -> Option<&str> {
    let stem = path.file_stem().and_then(|value| value.to_str())?;
    match stem {
        "lib" | "main" => None,
        "mod" => path.parent().and_then(Path::file_name).and_then(|value| value.to_str()),
        concept => Some(concept),
    }
}

#[cfg(test)]
mod tests {
    use crate::module_naming::{DUMPING_GROUND_NAMES, module_concept_name};
    use std::path::Path;

    #[test]
    fn a_file_stem_names_the_concept() {
        assert_eq!(module_concept_name(Path::new("crates/andromeda-entity/src/utils.rs")), Some("utils"));
    }

    #[test]
    fn a_mod_file_names_the_directory_it_heads() {
        assert_eq!(module_concept_name(Path::new("crates/andromeda-entity/src/helpers/mod.rs")), Some("helpers"));
    }

    #[test]
    fn crate_entry_points_name_no_concept() {
        assert_eq!(module_concept_name(Path::new("src/main.rs")), None);
        assert_eq!(module_concept_name(Path::new("crates/andromeda-entity/src/lib.rs")), None);
    }

    #[test]
    fn the_forbidden_set_covers_the_claude_md_list() {
        for name in ["common", "helpers", "shared", "misc", "core", "types", "utils"] {
            assert!(DUMPING_GROUND_NAMES.contains(&name), "{name} must be forbidden");
        }
    }

    #[test]
    fn a_named_concept_is_not_a_dumping_ground() {
        let concept = module_concept_name(Path::new("crates/andromeda-entity/src/mutation.rs"));
        assert_eq!(concept, Some("mutation"));
        assert!(concept.is_some_and(|name| !DUMPING_GROUND_NAMES.contains(&name)));
    }
}
