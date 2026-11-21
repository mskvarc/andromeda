//! Reading one workspace file for a gate.
//!
//! A file a gate cannot read or parse is itself a violation, so a gate never
//! passes over a file it could not see. Each file is named by its path
//! relative to the workspace root.

use std::{fs, path::Path};
use toml_edit::DocumentMut;

/// The text of the file at `relative` under `root`, or `None` with the reason
/// recorded in `violations`.
pub(crate) fn read_workspace_file(root: &Path, relative: &str, violations: &mut Vec<String>) -> Option<String> {
    match fs::read_to_string(root.join(relative)) {
        Ok(text) => Some(text),
        Err(error) => {
            violations.push(format!("{relative} could not be read: {error}"));
            None
        }
    }
}

/// The file at `relative` under `root` as a TOML document, or `None` with the
/// reason recorded in `violations`.
pub(crate) fn read_toml_document(root: &Path, relative: &str, violations: &mut Vec<String>) -> Option<DocumentMut> {
    let text = read_workspace_file(root, relative, violations)?;
    match text.parse::<DocumentMut>() {
        Ok(document) => Some(document),
        Err(error) => {
            violations.push(format!("{relative} does not parse as TOML: {error}"));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::workspace_file::{read_toml_document, read_workspace_file};
    use pretty_assertions::assert_eq;
    use std::{env, fs, path::PathBuf, process};

    fn scratch_root(name: &str) -> PathBuf {
        let root = env::temp_dir().join(format!("andromeda-workspace-file-{}-{name}", process::id()));
        fs::create_dir_all(&root).unwrap_or_else(|error| panic!("{error}"));
        root
    }

    #[test]
    fn a_readable_toml_file_parses_without_a_violation() {
        let root = scratch_root("toml");
        fs::write(root.join("settings.toml"), "[table]\nkey = \"value\"\n").unwrap_or_else(|error| panic!("{error}"));
        let mut violations = Vec::new();
        let document = read_toml_document(&root, "settings.toml", &mut violations);
        assert_eq!(
            document.and_then(|document| document["table"]["key"].as_str().map(str::to_owned)).as_deref(),
            Some("value")
        );
        assert_eq!(violations, Vec::<String>::new());
    }

    #[test]
    fn a_missing_file_is_a_violation_naming_its_relative_path() {
        let root = scratch_root("missing");
        let mut violations = Vec::new();
        assert_eq!(read_workspace_file(&root, "absent.toml", &mut violations), None);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].starts_with("absent.toml could not be read: "), "{violations:?}");
    }

    #[test]
    fn a_file_that_is_not_toml_is_a_violation_naming_its_relative_path() {
        let root = scratch_root("malformed");
        fs::write(root.join("broken.toml"), "key = \n").unwrap_or_else(|error| panic!("{error}"));
        let mut violations = Vec::new();
        assert!(read_toml_document(&root, "broken.toml", &mut violations).is_none());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].starts_with("broken.toml does not parse as TOML: "), "{violations:?}");
    }
}
