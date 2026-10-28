//! The `pg_format` executable the migrations are laid out with, and the
//! repository-root files that configure it.
//!
//! `PG_FORMAT` names a specific executable, and without it the first
//! `pg_format` on `PATH` is taken. Either way the candidate is probed by
//! formatting one statement with exactly the options the migrations are
//! formatted with, so an executable too old for the configuration is refused
//! before any file is read.

use crate::{
    executable_lookup::{ExecutableLookup, ExecutableLookupError},
    formatter_vocabulary::VocabularyFile,
};
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    string::FromUtf8Error,
    thread,
};

/// The executable looked up on `PATH` when `PG_FORMAT` is not set.
const EXECUTABLE_NAME: &str = "pg_format";

/// The variable naming a specific `pg_format` executable.
pub const OVERRIDE_VARIABLE: &str = "PG_FORMAT";

/// The statement a candidate executable is probed with.
const PROBE_STATEMENT: &str = "select 1;\n";

/// The three repository-root files `pg_format` is configured with.
#[derive(Debug)]
pub struct PgFormatConfiguration {
    /// The option file, `.pg_format`.
    options: PathBuf,
    /// The extension keywords `pg_format` does not know, `.pg_format_keywords`.
    keywords: PathBuf,
    /// The extension functions `pg_format` does not know, `.pg_format_functions`.
    functions: PathBuf,
}

impl PgFormatConfiguration {
    /// The configuration files at the root of the repository at `root`.
    ///
    /// # Errors
    ///
    /// Returns [`PgFormatError::MissingConfiguration`] naming the first file
    /// that does not exist.
    pub fn at(root: &Path) -> Result<PgFormatConfiguration, PgFormatError> {
        let existing = |name: &str| {
            let path = root.join(name);
            if path.is_file() {
                Ok(path)
            } else {
                Err(PgFormatError::MissingConfiguration(path))
            }
        };
        Ok(PgFormatConfiguration {
            options: existing(".pg_format")?,
            keywords: existing(VocabularyFile::Keywords.as_ref())?,
            functions: existing(VocabularyFile::Functions.as_ref())?,
        })
    }
}

/// A probed `pg_format` executable together with its configuration.
#[derive(Debug)]
pub struct PgFormat {
    /// The executable.
    executable: PathBuf,
    /// The configuration every run passes.
    configuration: PgFormatConfiguration,
}

impl PgFormat {
    /// The executable `lookup` finds, once it has formatted the probe statement
    /// with `configuration`.
    ///
    /// # Errors
    ///
    /// Returns [`PgFormatError`] when `PG_FORMAT` names no executable, no
    /// `pg_format` is on `PATH`, or the executable found fails the probe.
    pub fn resolve(lookup: &ExecutableLookup, configuration: PgFormatConfiguration) -> Result<PgFormat, PgFormatError> {
        let executable = lookup.find(EXECUTABLE_NAME).map_err(|error| match error {
            ExecutableLookupError::NotOnPath => PgFormatError::NotInstalled,
            ExecutableLookupError::OverrideNotExecutable(executable) => PgFormatError::OverrideNotExecutable(executable),
        })?;
        let candidate = PgFormat { executable, configuration };
        match candidate.format(PROBE_STATEMENT) {
            Ok(output) if !output.is_empty() => Ok(candidate),
            Ok(_) | Err(_) => Err(PgFormatError::ProbeFailed(candidate.executable)),
        }
    }

    /// The text `pg_format` lays `sql` out as.
    ///
    /// # Errors
    ///
    /// Returns [`PgFormatError`] when the executable cannot be run, exits
    /// unsuccessfully, or writes text that is not UTF-8.
    pub fn format(&self, sql: &str) -> Result<String, PgFormatError> {
        let mut child = Command::new(&self.executable)
            .arg("--config")
            .arg(&self.configuration.options)
            .arg("--extra-keyword")
            .arg(&self.configuration.keywords)
            .arg("--extra-function")
            .arg(&self.configuration.functions)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(PgFormatError::Spawn)?;
        let stdin = child.stdin.take();
        // The input is written from its own thread: a file larger than the pipe
        // buffer would otherwise block on stdin while pg_format blocks on stdout.
        let (written, output) = thread::scope(|scope| {
            let writer = scope.spawn(move || stdin.map_or(Ok(()), |mut stdin| stdin.write_all(sql.as_bytes())));
            let output = child.wait_with_output();
            (writer.join(), output)
        });
        let output = output.map_err(PgFormatError::Spawn)?;
        if !output.status.success() {
            return Err(PgFormatError::Failed(output.status));
        }
        match written {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(PgFormatError::Spawn(error)),
            Err(_) => return Err(PgFormatError::WriterPanicked),
        }
        String::from_utf8(output.stdout).map_err(PgFormatError::NotUtf8)
    }
}

/// Why `pg_format` could not be found, probed or run.
#[derive(Debug, thiserror::Error)]
pub enum PgFormatError {
    /// No `pg_format` is on `PATH` and `PG_FORMAT` is not set.
    #[error("pg_format is not installed; install pgFormatter or set PG_FORMAT")]
    NotInstalled,
    /// `PG_FORMAT` names no executable.
    #[error("PG_FORMAT={} is not an executable", .0.display())]
    OverrideNotExecutable(PathBuf),
    /// The executable found does not format the probe statement with the
    /// repository configuration.
    #[error("{} does not support the options the migrations are formatted with", .0.display())]
    ProbeFailed(PathBuf),
    /// A configuration file is missing from the repository root.
    #[error("{} is missing", .0.display())]
    MissingConfiguration(PathBuf),
    /// The executable could not be run, or its input not written.
    #[error("pg_format could not be run: {0}")]
    Spawn(io::Error),
    /// The executable exited unsuccessfully; what it wrote to stderr has
    /// already reached the terminal.
    #[error("pg_format failed with {0}")]
    Failed(ExitStatus),
    /// The thread writing the input to the executable panicked.
    #[error("the thread writing to pg_format panicked")]
    WriterPanicked,
    /// The executable wrote text that is not UTF-8.
    #[error("pg_format wrote text that is not UTF-8: {0}")]
    NotUtf8(FromUtf8Error),
}

#[cfg(test)]
mod tests {
    use crate::{
        executable_lookup::ExecutableLookup,
        pg_format::{PgFormat, PgFormatConfiguration, PgFormatError},
    };
    use std::{env, io, path::Path};
    use tempfile::TempDir;

    fn repository_root() -> &'static Path {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
    }

    #[test]
    fn a_path_without_pg_format_is_a_named_error() -> Result<(), PgFormatError> {
        let empty = TempDir::new().map_err(PgFormatError::Spawn)?;
        let lookup = ExecutableLookup::new(None, Some(empty.path().as_os_str().to_owned()), empty.path().to_path_buf());
        let resolved = PgFormat::resolve(&lookup, PgFormatConfiguration::at(repository_root())?);
        assert!(matches!(resolved, Err(PgFormatError::NotInstalled)), "{resolved:?}");
        Ok(())
    }

    #[test]
    fn an_override_naming_no_executable_is_a_named_error() -> Result<(), PgFormatError> {
        let empty = TempDir::new().map_err(PgFormatError::Spawn)?;
        let lookup = ExecutableLookup::new(
            Some("pg_format_missing".into()),
            Some(empty.path().as_os_str().to_owned()),
            empty.path().to_path_buf(),
        );
        let resolved = PgFormat::resolve(&lookup, PgFormatConfiguration::at(repository_root())?);
        assert!(matches!(resolved, Err(PgFormatError::OverrideNotExecutable(_))), "{resolved:?}");
        Ok(())
    }

    #[test]
    fn an_executable_that_is_not_pg_format_fails_the_probe() -> Result<(), PgFormatError> {
        let lookup = ExecutableLookup::new(Some("false".into()), env::var_os("PATH"), env::current_dir().map_err(PgFormatError::Spawn)?);
        let resolved = PgFormat::resolve(&lookup, PgFormatConfiguration::at(repository_root())?);
        assert!(matches!(resolved, Err(PgFormatError::ProbeFailed(_))), "{resolved:?}");
        Ok(())
    }

    #[test]
    fn a_root_without_the_configuration_is_a_named_error() -> io::Result<()> {
        let empty = TempDir::new()?;
        let configuration = PgFormatConfiguration::at(empty.path());
        assert!(
            matches!(&configuration, Err(PgFormatError::MissingConfiguration(path)) if path.ends_with(".pg_format")),
            "{configuration:?}"
        );
        Ok(())
    }
}
