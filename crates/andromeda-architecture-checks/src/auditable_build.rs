//! The broker image builds a broker binary that carries its dependency tree.
//!
//! Scanners and SBOM generators see the crates a Rust binary links only
//! through the dependency list `cargo auditable` embeds in it. A binary built
//! with a plain `cargo build` carries none, so the image's SBOMs list no crate,
//! scanners report nothing against them, and the image's VEX statements name
//! crates no scanner sees. The gate holds every build of the broker binary in
//! the broker image to `cargo auditable build`.

use crate::workspace_file::read_workspace_file;
use std::path::Path;

/// The broker image's definition, relative to the workspace root.
const BROKER_IMAGE: &str = "docker/images/andromeda/Dockerfile";

/// The name of the broker binary, as `--bin` selects it.
const BROKER_BINARY: &str = "andromeda";

/// The words of a build that embeds the dependency tree.
const AUDITABLE_BUILD: [&str; 3] = ["cargo", "auditable", "build"];

/// Assert that the broker image builds the broker binary, and builds it with
/// `cargo auditable build` every time.
///
/// # Panics
///
/// Panics when the image builds the broker binary with another command, when
/// it builds no broker binary the gate can find, or when its definition cannot
/// be read.
pub fn assert_the_broker_image_builds_an_auditable_binary(workspace_root: &str) {
    let mut violations = Vec::new();
    if let Some(dockerfile) = read_workspace_file(Path::new(workspace_root), BROKER_IMAGE, &mut violations) {
        violations.extend(unauditable_builds(&dockerfile));
    }
    assert!(
        violations.is_empty(),
        "broker binaries built without their dependency tree:\n{}",
        violations.join("\n")
    );
}

/// One finding per build of the broker binary in `dockerfile` that is no
/// `cargo auditable build`, or the one finding that it builds none.
fn unauditable_builds(dockerfile: &str) -> Vec<String> {
    let builds: Vec<String> = instructions(dockerfile).filter(|instruction| builds_the_broker(instruction)).collect();
    if builds.is_empty() {
        return vec![format!(
            "{BROKER_IMAGE} builds no binary with `--bin {BROKER_BINARY}`, so nothing shows that the broker binary carries its dependency tree"
        )];
    }
    builds
        .iter()
        .filter(|build| !is_auditable(build))
        .map(|build| format!("{BROKER_IMAGE} builds the broker binary with `{build}`, which is no `cargo auditable build`"))
        .collect()
}

/// The instructions of `dockerfile`, each a line with the lines it continues
/// through trailing backslashes joined on, its words separated by single
/// spaces.
fn instructions(dockerfile: &str) -> impl Iterator<Item = String> {
    let mut joined = Vec::new();
    let mut current = String::new();
    for line in dockerfile.lines() {
        let (text, continues) = line.trim_end().strip_suffix('\\').map_or((line, false), |text| (text, true));
        current.push(' ');
        current.push_str(text);
        if !continues {
            joined.push(current.split_whitespace().collect::<Vec<_>>().join(" "));
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        joined.push(current.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    joined.into_iter()
}

/// Whether `instruction` runs a cargo build that selects the broker binary,
/// as `--bin andromeda` or `--bin=andromeda`.
fn builds_the_broker(instruction: &str) -> bool {
    let words: Vec<&str> = instruction.split_whitespace().collect();
    let selects_the_broker =
        words.windows(2).any(|pair| pair == ["--bin", BROKER_BINARY]) || words.iter().any(|word| word.strip_prefix("--bin=") == Some(BROKER_BINARY));
    selects_the_broker && words.contains(&"cargo") && words.contains(&"build")
}

/// Whether `instruction` runs `cargo auditable build`.
fn is_auditable(instruction: &str) -> bool {
    let words: Vec<&str> = instruction.split_whitespace().collect();
    words.windows(AUDITABLE_BUILD.len()).any(|window| window == AUDITABLE_BUILD)
}

#[cfg(test)]
mod tests {
    use crate::auditable_build::{assert_the_broker_image_builds_an_auditable_binary, instructions, unauditable_builds};
    use pretty_assertions::assert_eq;
    use std::{env, fs, process};

    const COOK: &str =
        "RUN --mount=type=cache,target=/var/cache/sccache \\\n    with-sccache cargo chef cook --profile dist --locked --recipe-path recipe.json\n";

    fn image(build: &str) -> String {
        format!(
            "FROM chef AS builder\n{COOK}RUN --mount=type=cache,target=/var/cache/sccache \\\n    {build} \\\n    && install -D -m 0755 target/dist/andromeda /dist/andromeda\n"
        )
    }

    #[test]
    fn an_auditable_build_of_the_broker_continued_over_lines_passes() {
        assert_eq!(
            unauditable_builds(&image("with-sccache cargo auditable build --profile dist --locked --bin andromeda")),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_plain_build_of_the_broker_is_a_finding() {
        for build in [
            "with-sccache cargo build --profile dist --locked --bin andromeda",
            "cargo build --bin=andromeda --profile dist",
        ] {
            assert_eq!(
                unauditable_builds(&image(build)),
                [format!(
                    "docker/images/andromeda/Dockerfile builds the broker binary with `RUN --mount=type=cache,target=/var/cache/sccache {build} && install -D -m 0755 target/dist/andromeda /dist/andromeda`, which is no `cargo auditable build`"
                )],
                "{build}"
            );
        }
    }

    #[test]
    fn an_image_that_builds_no_broker_binary_is_a_finding() {
        for build in [
            "cargo auditable build --profile dist --bin andromeda-sql",
            "cargo auditable build --profile dist",
        ] {
            assert_eq!(unauditable_builds(&image(build)).len(), 1, "{build}");
        }
    }

    #[test]
    fn instructions_join_their_continued_lines_and_keep_the_others_apart() {
        assert_eq!(
            instructions("FROM chef\nRUN a \\\n    b\\\n  c\n\nCOPY . .\n").collect::<Vec<_>>(),
            ["FROM chef", "RUN a b c", "", "COPY . ."]
        );
    }

    #[test]
    #[should_panic(expected = "docker/images/andromeda/Dockerfile could not be read")]
    fn a_workspace_without_the_broker_image_fails() {
        let root = env::temp_dir().join(format!("andromeda-auditable-build-{}-absent", process::id()));
        fs::create_dir_all(&root).unwrap();
        assert_the_broker_image_builds_an_auditable_binary(&root.to_string_lossy());
    }
}
