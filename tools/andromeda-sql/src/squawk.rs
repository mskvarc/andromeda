//! The Squawk executable the migrations are linted with, and the
//! repository-root `.squawk.toml` that configures it.
//!
//! `SQUAWK` names a specific executable, and without it the first `squawk` on
//! `PATH` is taken. Its version must equal [`PINNED_VERSION`]: a rule added or
//! changed in another release would pass or fail the tree differently from CI.

use crate::{
    executable_lookup::{ExecutableLookup, ExecutableLookupError},
    lint_finding::{LintFinding, LintRule, LintSeverity, SquawkRule},
    source_range::{LineColumn, SourceRange},
};
use andromeda_postgresql_context_store::schema_contract::server_requirements::PostgresVersion;
use iri_rs::{Iri, iri};
use semver::Version;
use std::{
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    string::FromUtf8Error,
};
use url::Url;

/// The Squawk release the rules are pinned to.
pub const PINNED_VERSION: Version = Version::new(2, 70, 0);

/// The executable looked up on `PATH` when `SQUAWK` is not set.
const EXECUTABLE_NAME: &str = "squawk";

/// The variable naming a specific Squawk executable.
pub const OVERRIDE_VARIABLE: &str = "SQUAWK";

/// The configuration file at the repository root.
const CONFIGURATION_FILE: &str = ".squawk.toml";

/// What `squawk --version` prints before the version.
const VERSION_PREFIX: &str = "squawk ";

/// The address every rule's page is found under.
const DOCUMENTATION_BASE: Iri<&'static str> = iri!("https://squawkhq.com/docs/");

/// The exit status Squawk ends with when it found violations.
const VIOLATIONS_FOUND: i32 = 1;

/// The `.squawk.toml` Squawk is configured with.
#[derive(Debug)]
pub struct SquawkConfiguration {
    /// The configuration file.
    path: PathBuf,
}

impl SquawkConfiguration {
    /// The configuration at the root of the repository at `root`.
    ///
    /// # Errors
    ///
    /// Returns [`SquawkError::MissingConfiguration`] when the file does not exist.
    pub fn at(root: &Path) -> Result<SquawkConfiguration, SquawkError> {
        let path = root.join(CONFIGURATION_FILE);
        if path.is_file() {
            Ok(SquawkConfiguration { path })
        } else {
            Err(SquawkError::MissingConfiguration(path))
        }
    }
}

/// A Squawk executable of the pinned version, together with its configuration.
#[derive(Debug)]
pub struct Squawk {
    /// The executable.
    executable: PathBuf,
    /// The configuration every run passes.
    configuration: SquawkConfiguration,
}

impl Squawk {
    /// The executable `lookup` finds, once it reports [`PINNED_VERSION`].
    ///
    /// # Errors
    ///
    /// Returns [`SquawkError`] when `SQUAWK` names no executable, no `squawk`
    /// is on `PATH`, or the executable found is another version.
    pub fn resolve(lookup: &ExecutableLookup, configuration: SquawkConfiguration) -> Result<Squawk, SquawkError> {
        let executable = lookup.find(EXECUTABLE_NAME).map_err(|error| match error {
            ExecutableLookupError::NotOnPath => SquawkError::NotInstalled,
            ExecutableLookupError::OverrideNotExecutable(executable) => SquawkError::OverrideNotExecutable(executable),
        })?;
        let found = version_of(&executable)?;
        if found == PINNED_VERSION {
            Ok(Squawk { executable, configuration })
        } else {
            Err(SquawkError::WrongVersion { executable, found })
        }
    }

    /// Lint `files`, paths relative to `root`, against PostgreSQL `postgres`.
    ///
    /// Each finding is returned with the path of its file as given.
    ///
    /// # Errors
    ///
    /// Returns [`SquawkError`] when Squawk cannot be run, fails, or reports
    /// something that is not its JSON report.
    pub fn lint<File: AsRef<OsStr>>(
        &self,
        root: &Path,
        files: impl IntoIterator<Item = File>,
        postgres: PostgresVersion,
    ) -> Result<Vec<(PathBuf, LintFinding)>, SquawkError> {
        let output = Command::new(&self.executable)
            .current_dir(root)
            .arg("--config")
            .arg(&self.configuration.path)
            .arg("--pg-version")
            .arg(postgres.to_string())
            .args(["--reporter", "json"])
            .args(files)
            .stdin(Stdio::null())
            .stderr(Stdio::inherit())
            .output()
            .map_err(SquawkError::Spawn)?;
        match output.status.code() {
            Some(0 | VIOLATIONS_FOUND) => {}
            Some(_) | None => return Err(SquawkError::Failed(output.status)),
        }
        let violations: Vec<ReportedViolation> = serde_json::from_slice(&output.stdout).map_err(SquawkError::Report)?;
        violations.into_iter().map(ReportedViolation::into_finding).collect()
    }
}

/// The version `executable --version` reports.
fn version_of(executable: &Path) -> Result<Version, SquawkError> {
    let output = Command::new(executable)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(SquawkError::Spawn)?;
    if !output.status.success() {
        return Err(SquawkError::NotSquawk(executable.to_path_buf()));
    }
    let printed = String::from_utf8(output.stdout).map_err(SquawkError::NotUtf8)?;
    let version = printed
        .trim_end()
        .strip_prefix(VERSION_PREFIX)
        .ok_or_else(|| SquawkError::NotSquawk(executable.to_path_buf()))?;
    Version::parse(version).map_err(SquawkError::UnreadableVersion)
}

/// One entry of Squawk's JSON report.
#[derive(Debug, serde::Deserialize)]
struct ReportedViolation {
    /// The file, as it was passed.
    file: PathBuf,
    /// How serious the violation is.
    level: LintSeverity,
    /// The line the violation starts on, from zero.
    line: usize,
    /// The byte column it starts at, from zero.
    column: usize,
    /// The line it ends on, from zero.
    line_end: usize,
    /// The byte column it ends before, from zero.
    column_end: usize,
    /// What is wrong.
    message: String,
    /// How to put it right, when Squawk says.
    help: Option<String>,
    /// The rule broken.
    rule_name: SquawkRule,
}

impl ReportedViolation {
    /// The finding the entry reports, with the file it is in.
    fn into_finding(self) -> Result<(PathBuf, LintFinding), SquawkError> {
        let documentation = Url::parse(DOCUMENTATION_BASE.as_str())
            .and_then(|base| base.join(self.rule_name.as_ref()))
            .map_err(SquawkError::DocumentationUrl)?;
        let range = SourceRange::new(LineColumn::new(self.line, self.column), LineColumn::new(self.line_end, self.column_end));
        Ok((
            self.file,
            LintFinding::new(
                LintRule::Squawk(self.rule_name),
                self.level,
                range,
                self.message,
                self.help,
                Some(documentation),
            ),
        ))
    }
}

/// Why Squawk could not be found, checked or run.
#[derive(Debug, thiserror::Error)]
pub enum SquawkError {
    /// No `squawk` is on `PATH` and `SQUAWK` is not set.
    #[error("squawk is not installed; install Squawk {PINNED_VERSION} or set SQUAWK")]
    NotInstalled,
    /// `SQUAWK` names no executable.
    #[error("SQUAWK={} is not an executable", .0.display())]
    OverrideNotExecutable(PathBuf),
    /// The executable does not report a Squawk version.
    #[error("{} does not report a Squawk version", .0.display())]
    NotSquawk(PathBuf),
    /// The executable is a Squawk release other than the pinned one.
    #[error("{} is Squawk {found}; the rules are pinned to Squawk {PINNED_VERSION}", executable.display())]
    WrongVersion {
        /// The executable concerned.
        executable: PathBuf,
        /// The version it reports.
        found: Version,
    },
    /// The version Squawk reports is not a semantic version.
    #[error("squawk reports a version that is not a semantic version: {0}")]
    UnreadableVersion(semver::Error),
    /// The configuration file is missing from the repository root.
    #[error("{} is missing", .0.display())]
    MissingConfiguration(PathBuf),
    /// Squawk could not be run.
    #[error("squawk could not be run: {0}")]
    Spawn(io::Error),
    /// Squawk exited with a status other than success or violations found;
    /// what it wrote to stderr has already reached the terminal.
    #[error("squawk failed with {0}")]
    Failed(ExitStatus),
    /// Squawk printed text that is not UTF-8.
    #[error("squawk printed text that is not UTF-8: {0}")]
    NotUtf8(FromUtf8Error),
    /// Squawk's report is not the JSON it documents.
    #[error("squawk's report could not be read: {0}")]
    Report(serde_json::Error),
    /// A rule's documentation address could not be formed.
    #[error("a rule's documentation address could not be formed: {0}")]
    DocumentationUrl(url::ParseError),
}

#[cfg(test)]
mod tests {
    use crate::{
        executable_lookup::ExecutableLookup,
        squawk::{Squawk, SquawkConfiguration, SquawkError},
    };
    use std::{env, path::Path};
    #[cfg(unix)]
    use std::{fs, io, os::unix::fs::PermissionsExt};
    use tempfile::TempDir;

    fn repository_root() -> &'static Path {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
    }

    #[cfg(unix)]
    /// A directory holding one executable script named `squawk` that prints `version`.
    fn fake_squawk(version: &str) -> io::Result<TempDir> {
        let directory = TempDir::new()?;
        let script = directory.path().join("squawk");
        fs::write(&script, format!("#!/bin/sh\necho 'squawk {version}'\n"))?;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
        Ok(directory)
    }

    #[test]
    fn a_path_without_squawk_is_a_named_error() -> Result<(), SquawkError> {
        let empty = TempDir::new().map_err(SquawkError::Spawn)?;
        let lookup = ExecutableLookup::new(None, Some(empty.path().as_os_str().to_owned()), empty.path().to_path_buf());
        let resolved = Squawk::resolve(&lookup, SquawkConfiguration::at(repository_root())?);
        assert!(matches!(resolved, Err(SquawkError::NotInstalled)), "{resolved:?}");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn another_squawk_release_is_refused_with_both_versions() -> Result<(), SquawkError> {
        let directory = fake_squawk("2.69.1").map_err(SquawkError::Spawn)?;
        let lookup = ExecutableLookup::new(None, Some(directory.path().as_os_str().to_owned()), directory.path().to_path_buf());
        let resolved = Squawk::resolve(&lookup, SquawkConfiguration::at(repository_root())?);
        let message = resolved.as_ref().err().map(ToString::to_string).unwrap_or_default();
        assert!(matches!(resolved, Err(SquawkError::WrongVersion { .. })), "{resolved:?}");
        assert!(message.ends_with("is Squawk 2.69.1; the rules are pinned to Squawk 2.70.0"), "{message}");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn the_pinned_release_is_accepted() -> Result<(), SquawkError> {
        let directory = fake_squawk("2.70.0").map_err(SquawkError::Spawn)?;
        let lookup = ExecutableLookup::new(None, Some(directory.path().as_os_str().to_owned()), directory.path().to_path_buf());
        let resolved = Squawk::resolve(&lookup, SquawkConfiguration::at(repository_root())?);
        assert!(resolved.is_ok(), "{resolved:?}");
        Ok(())
    }

    #[test]
    fn an_executable_that_is_not_squawk_is_refused() -> Result<(), SquawkError> {
        let lookup = ExecutableLookup::new(Some("true".into()), env::var_os("PATH"), env::current_dir().map_err(SquawkError::Spawn)?);
        let resolved = Squawk::resolve(&lookup, SquawkConfiguration::at(repository_root())?);
        assert!(matches!(resolved, Err(SquawkError::NotSquawk(_))), "{resolved:?}");
        Ok(())
    }
}
