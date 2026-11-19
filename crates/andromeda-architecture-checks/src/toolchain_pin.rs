//! Every official Rust image an image builds from carries the toolchain the
//! workspace pins.
//!
//! The tag of a Rust base image spells the pin of `rust-toolchain.toml` a second
//! time. The runner image takes its toolchain from that tag alone, and a broker
//! image built from another version still builds the right binary, because
//! rustup installs the pin on top, but downloads a second toolchain on every
//! build. The gate holds the tags and the pin together, so they move in one
//! change or not at all.

use crate::source_scan::file_paths;
use std::{fs, path::Path};
use toml_edit::DocumentMut;

/// The file that pins the workspace toolchain, relative to the workspace root.
const TOOLCHAIN_FILE: &str = "rust-toolchain.toml";

/// The directories that hold image definitions, relative to the workspace root.
const IMAGE_DIRECTORIES: [&str; 2] = ["docker", "tools"];

/// The instruction a build stage names its base image on, up to the image.
const STAGE_INSTRUCTION: &str = "FROM ";

/// The references of the official Rust image, up to the tag.
const RUST_IMAGES: [&str; 2] = ["rust:", "docker.io/library/rust:"];

/// Assert that every stage a `Dockerfile` below `docker/` or `tools/` builds
/// from the official Rust image names the version `rust-toolchain.toml` pins
/// in its tag.
///
/// # Panics
///
/// Panics when an image builds from a Rust image of another version, when an
/// image definition or `rust-toolchain.toml` cannot be read, or when
/// `rust-toolchain.toml` names no channel.
pub fn assert_images_build_from_the_pinned_toolchain(workspace_root: &str) {
    let root = Path::new(workspace_root);
    let mut violations = Vec::new();
    if let Some(channel) = pinned_channel(root, &mut violations) {
        for path in IMAGE_DIRECTORIES
            .into_iter()
            .flat_map(|directory| file_paths(&root.join(directory), is_dockerfile))
        {
            let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            match fs::read_to_string(&path) {
                Ok(dockerfile) => violations.extend(
                    base_toolchains(&dockerfile)
                        .filter(|base| *base != channel)
                        .map(|base| format!("{relative} builds from rust:{base}, but {TOOLCHAIN_FILE} pins {channel}")),
                ),
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

/// The toolchain of every official Rust image an image definition builds a
/// stage from: the version its tag names ahead of the variant and the digest,
/// `1.100.0` of `rust:1.100.0-trixie@sha256:...`. A stage built from another
/// image or from an earlier stage names none.
fn base_toolchains(dockerfile: &str) -> impl Iterator<Item = &str> {
    dockerfile
        .lines()
        .filter_map(|line| line.trim().strip_prefix(STAGE_INSTRUCTION))
        .filter_map(|stage| stage.split_whitespace().find(|word| !word.starts_with("--")))
        .filter_map(|image| RUST_IMAGES.iter().find_map(|prefix| image.strip_prefix(prefix)))
        .filter_map(|tag| tag.split(['-', '@']).next())
}

#[cfg(test)]
mod tests {
    use crate::toolchain_pin::{assert_images_build_from_the_pinned_toolchain, base_toolchains, is_dockerfile};
    use pretty_assertions::assert_eq;
    use std::{env, fs, path::Path, process};

    /// A workspace pinning `pinned` whose runner image builds from the Rust
    /// image of version `base`.
    fn scratch_workspace(name: &str, pinned: &str, base: &str) -> String {
        let root = env::temp_dir().join(format!("andromeda-toolchain-pin-{}-{name}", process::id()));
        let runner = root.join("tools/suite/stacks/runner");
        fs::create_dir_all(&runner).unwrap_or_else(|error| panic!("{error}"));
        fs::write(root.join("rust-toolchain.toml"), format!("[toolchain]\nchannel = \"{pinned}\"\n")).unwrap_or_else(|error| panic!("{error}"));
        fs::write(runner.join("Dockerfile"), format!("FROM rust:{base}-trixie@sha256:0123 AS toolchain\n")).unwrap_or_else(|error| panic!("{error}"));
        root.to_string_lossy().into_owned()
    }

    #[test]
    fn an_image_built_from_the_pinned_rust_image_passes() {
        assert_images_build_from_the_pinned_toolchain(&scratch_workspace("agree", "1.100.0", "1.100.0"));
    }

    #[test]
    #[should_panic(expected = "tools/suite/stacks/runner/Dockerfile builds from rust:1.99.1, but rust-toolchain.toml pins 1.100.0")]
    fn an_image_built_from_another_rust_image_fails() {
        assert_images_build_from_the_pinned_toolchain(&scratch_workspace("disagree", "1.100.0", "1.99.1"));
    }

    #[test]
    fn the_base_toolchain_is_read_from_every_rust_stage_only() {
        let dockerfile = "FROM rust:1.100.0-trixie@sha256:0123 AS chef\n\
                          FROM --platform=$BUILDPLATFORM docker.io/library/rust:1.99.1 AS cross\n\
                          FROM chef AS planner\n\
                          FROM gcr.io/distroless/cc-debian13:nonroot@sha256:4567 AS runtime\n";
        assert_eq!(base_toolchains(dockerfile).collect::<Vec<_>>(), ["1.100.0", "1.99.1"]);
    }

    #[test]
    fn a_floating_rust_tag_names_its_own_version() {
        assert_eq!(base_toolchains("FROM rust:1-trixie AS chef\n").collect::<Vec<_>>(), ["1"]);
    }

    #[test]
    fn only_a_file_named_dockerfile_is_an_image_definition() {
        assert!(is_dockerfile(Path::new("tools/andromeda-test-suite/stacks/runner/Dockerfile")));
        assert!(!is_dockerfile(Path::new("tools/andromeda-test-suite/stacks/runner/nextest.toml")));
        assert!(!is_dockerfile(Path::new("docker/images/andromeda/Dockerfile.dockerignore")));
    }
}
