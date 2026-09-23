//! Compile every integration-test suite once and package the binaries beside a
//! [`TestManifest`], so CI builds the suites a single time and fans the artifact
//! out to each stack leg instead of recompiling the workspace per leg.

use crate::{error::XtaskError, stack::STACKS, test_binaries::TestManifest};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// The one line of cargo's `--message-format=json` stream this build reads: the
/// compiler-artifact record that carries a compiled test target's executable.
/// Every other message reason is irrelevant here and collapses to `Other`.
#[derive(Deserialize)]
#[serde(tag = "reason")]
enum CargoMessage {
    /// A compiled artifact, including the integration-test executables.
    #[serde(rename = "compiler-artifact")]
    CompilerArtifact {
        /// The target the artifact was built from.
        target: ArtifactTarget,
        /// The runnable executable, present for test and binary targets.
        #[serde(default)]
        executable: Option<PathBuf>,
    },
    /// Any other message reason (build-script output, diagnostics, build-finished).
    #[serde(other)]
    Other,
}

/// The subset of a compiler-artifact's `target` this build inspects.
#[derive(Deserialize)]
struct ArtifactTarget {
    /// The target name, which for an integration test is the suite name.
    name: String,
    /// The target's cargo kinds; an integration test's is exactly `["test"]`.
    kind: Vec<String>,
}

/// Compile every registered suite and package its binary under `out_dir/bin`,
/// writing `out_dir/manifest.json` mapping each suite to its relative binary path.
///
/// # Errors
///
/// Returns [`XtaskError::TestCompile`] if the compile fails, [`XtaskError::BuildMessageParse`]
/// if cargo's JSON stream cannot be parsed, [`XtaskError::SuiteBinaryMissing`] if a
/// registered suite produced no executable, and [`XtaskError::Io`] on any filesystem error.
pub(crate) fn build(out_dir: &Path) -> Result<(), XtaskError> {
    // Every integration-test target across all stacks, deduplicated. Naming each
    // one keeps the compile to the suites this pipeline runs, leaving the lib and
    // bin unit tests to their own workflow.
    let mut suites: Vec<&'static str> = STACKS.iter().flat_map(|stack| stack.suites.iter().map(|suite| suite.name)).collect();
    suites.sort_unstable();
    suites.dedup();
    let suites = suites;

    let executables = compile_suites(&suites)?;
    package(out_dir, &suites, executables)
}

/// Run `cargo test --no-run` for the named suites and collect each one's executable.
fn compile_suites(suites: &[&'static str]) -> Result<BTreeMap<String, PathBuf>, XtaskError> {
    let mut command = Command::new("cargo");
    command.args(["test", "--package", "andromeda", "--no-run", "--locked", "--message-format=json"]);
    for suite in suites {
        command.arg("--test").arg(suite);
    }
    command.stdout(Stdio::piped());

    let mut child = command.spawn()?;
    let stdout = child.stdout.take().ok_or(XtaskError::TestCompile)?;
    let reader = BufReader::new(stdout);

    let mut executables: BTreeMap<String, PathBuf> = BTreeMap::new();
    for line in reader.lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let message: CargoMessage = serde_json::from_str(&line).map_err(|error| XtaskError::BuildMessageParse(error.to_string()))?;
        match message {
            CargoMessage::CompilerArtifact { target, executable } => {
                // An integration test's kind is exactly `["test"]`; the lib and
                // bin unit-test artifacts (`["lib"]`, `["bin"]`) are not run here.
                if let Some(path) = executable
                    && target.kind.iter().any(|kind| kind == "test")
                {
                    executables.insert(target.name, path);
                }
            }
            CargoMessage::Other => {}
        }
    }

    let status = child.wait()?;
    if status.success() { Ok(executables) } else { Err(XtaskError::TestCompile) }
}

/// Copy each suite's executable into `out_dir/bin` and write the manifest.
fn package(out_dir: &Path, suites: &[&'static str], executables: BTreeMap<String, PathBuf>) -> Result<(), XtaskError> {
    let bin_dir = out_dir.join("bin");
    fs::create_dir_all(&bin_dir)?;

    let mut binaries: BTreeMap<String, PathBuf> = BTreeMap::new();
    for (name, source) in executables {
        let relative = Path::new("bin").join(&name);
        fs::copy(&source, out_dir.join(&relative))?;
        binaries.insert(name, relative);
    }

    // A registered suite that produced no executable would fail only later, in a
    // leg that cannot find its binary; reject it here where the cause is obvious.
    for suite in suites {
        if !binaries.contains_key(*suite) {
            return Err(XtaskError::SuiteBinaryMissing((*suite).to_owned()));
        }
    }

    let manifest = TestManifest::new(binaries);
    manifest.write(&out_dir.join("manifest.json"))
}
