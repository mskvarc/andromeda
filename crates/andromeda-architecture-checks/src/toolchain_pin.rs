//! The toolchain an image installs by name is the one the workspace pins.
//!
//! An image whose build context does not hold `rust-toolchain.toml` installs the
//! toolchain its `TOOLCHAIN` build argument names, so that argument spells the
//! pin a second time. The gate holds the two together, so the pin moves in one
//! change or not at all.

use crate::source_scan::file_paths;
use std::{fs, path::Path};
use toml_edit::DocumentMut;

/// The file that pins the workspace toolchain, relative to the workspace root.
const TOOLCHAIN_FILE: &str = "rust-toolchain.toml";

/// The directories that hold image definitions, relative to the workspace root.
const IMAGE_DIRECTORIES: [&str; 2] = ["docker", "tools"];

/// The build argument line an image names its toolchain on, up to the value.
const TOOLCHAIN_ARGUMENT: &str = "ARG TOOLCHAIN=";

/// Assert that every `Dockerfile` below `docker/` and `tools/` that names a
/// toolchain in its `TOOLCHAIN` build argument names the channel
/// `rust-toolchain.toml` pins.
///
/// # Panics
///
/// Panics when an image names another toolchain, when an image definition or
/// `rust-toolchain.toml` cannot be read, or when `rust-toolchain.toml` names no
/// channel.
pub fn assert_images_install_the_pinned_toolchain(workspace_root: &str) {
    let root = Path::new(workspace_root);
    let mut violations = Vec::new();
    if let Some(channel) = pinned_channel(root, &mut violations) {
        for path in IMAGE_DIRECTORIES
            .into_iter()
            .flat_map(|directory| file_paths(&root.join(directory), is_dockerfile))
        {
            let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            match fs::read_to_string(&path) {
                Ok(dockerfile) => {
                    if let Some(installed) = installed_toolchain(&dockerfile)
                        && installed != channel
                    {
                        violations.push(format!("{relative} installs {installed}, but {TOOLCHAIN_FILE} pins {channel}"));
                    }
                }
                Err(error) => violations.push(format!("{relative} could not be read: {error}")),
            }
        }
    }
    assert!(violations.is_empty(), "toolchain pins that disagree:\n{}", violations.join("\n"));
}

/// The channel `rust-toolchain.toml` pins, or `None` with the reason recorded in
/// `violations`.
fn pinned_channel(root: &Path, violations: &mut Vec<String>) -> Option<String> {
    let text = match fs::read_to_string(root.join(TOOLCHAIN_FILE)) {
        Ok(text) => text,
        Err(error) => {
            violations.push(format!("{TOOLCHAIN_FILE} could not be read: {error}"));
            return None;
        }
    };
    let document = match text.parse::<DocumentMut>() {
        Ok(document) => document,
        Err(error) => {
            violations.push(format!("{TOOLCHAIN_FILE} does not parse as TOML: {error}"));
            return None;
        }
    };
    let channel = document
        .get("toolchain")
        .and_then(|table| table.get("channel"))
        .and_then(|item| item.as_str())
        .map(str::to_owned);
    if channel.is_none() {
        violations.push(format!("{TOOLCHAIN_FILE} names no toolchain.channel"));
    }
    channel
}

/// Whether a file is an image definition.
fn is_dockerfile(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "Dockerfile")
}

/// The toolchain an image definition installs by name, when it declares one.
fn installed_toolchain(dockerfile: &str) -> Option<&str> {
    dockerfile.lines().find_map(|line| line.trim().strip_prefix(TOOLCHAIN_ARGUMENT)).map(str::trim)
}

#[cfg(test)]
mod tests {
    use crate::toolchain_pin::{assert_images_install_the_pinned_toolchain, installed_toolchain, is_dockerfile};
    use pretty_assertions::assert_eq;
    use std::{env, fs, path::Path, process};

    fn scratch_workspace(name: &str, pinned: &str, installed: &str) -> String {
        let root = env::temp_dir().join(format!("andromeda-toolchain-pin-{}-{name}", process::id()));
        let runner = root.join("tools/suite/stacks/runner");
        fs::create_dir_all(&runner).unwrap_or_else(|error| panic!("{error}"));
        fs::write(root.join("rust-toolchain.toml"), format!("[toolchain]\nchannel = \"{pinned}\"\n")).unwrap_or_else(|error| panic!("{error}"));
        fs::write(runner.join("Dockerfile"), format!("FROM rust:1 AS toolchain\nARG TOOLCHAIN={installed}\n")).unwrap_or_else(|error| panic!("{error}"));
        root.to_string_lossy().into_owned()
    }

    #[test]
    fn an_image_naming_the_pinned_toolchain_passes() {
        assert_images_install_the_pinned_toolchain(&scratch_workspace("agree", "nightly-2026-09-25", "nightly-2026-09-25"));
    }

    #[test]
    #[should_panic(expected = "tools/suite/stacks/runner/Dockerfile installs nightly-2026-08-21, but rust-toolchain.toml pins nightly-2026-09-25")]
    fn an_image_naming_another_toolchain_fails() {
        assert_images_install_the_pinned_toolchain(&scratch_workspace("disagree", "nightly-2026-09-25", "nightly-2026-08-21"));
    }

    #[test]
    fn the_toolchain_argument_value_is_read_from_its_line() {
        let dockerfile = "FROM rust:1 AS toolchain\n\nARG TOOLCHAIN=nightly-2026-09-25\nARG NEXTEST_VERSION=0.9.149\n";
        assert_eq!(installed_toolchain(dockerfile), Some("nightly-2026-09-25"));
    }

    #[test]
    fn an_image_without_a_toolchain_argument_names_no_toolchain() {
        let dockerfile = "FROM rust:1 AS chef\nCOPY rust-toolchain.toml ./\nRUN rustup show\n";
        assert_eq!(installed_toolchain(dockerfile), None);
    }

    #[test]
    fn only_a_file_named_dockerfile_is_an_image_definition() {
        assert!(is_dockerfile(Path::new("tools/andromeda-test-suite/stacks/runner/Dockerfile")));
        assert!(!is_dockerfile(Path::new("tools/andromeda-test-suite/stacks/runner/nextest.toml")));
        assert!(!is_dockerfile(Path::new("docker/images/andromeda/Dockerfile.dockerignore")));
    }
}
