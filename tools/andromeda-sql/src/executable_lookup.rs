//! Where an external tool's executable is looked for.
//!
//! A tool is found by an environment variable naming a specific executable (a
//! path, or a name looked up on `PATH`), and without it by its own name on
//! `PATH`. A shell alias is invisible to a child process, so an alias is
//! expressed through the variable instead.

use std::{env, ffi::OsString, io, path::PathBuf};

/// Where one tool's executable is looked for.
#[derive(Debug)]
pub struct ExecutableLookup {
    /// The executable the tool's variable names, when it is set and not empty.
    override_executable: Option<OsString>,
    /// The directories `PATH` lists.
    search_path: Option<OsString>,
    /// The directory a relative executable path is resolved against.
    working_directory: PathBuf,
}

impl ExecutableLookup {
    /// The lookup the process environment describes, with `override_variable`
    /// naming the tool's own variable.
    ///
    /// # Errors
    ///
    /// Returns the I/O error when the working directory cannot be read.
    pub fn from_environment(override_variable: &str) -> io::Result<ExecutableLookup> {
        Ok(ExecutableLookup {
            override_executable: env::var_os(override_variable).filter(|value| !value.is_empty()),
            search_path: env::var_os("PATH"),
            working_directory: env::current_dir()?,
        })
    }

    /// A lookup over explicit values, for a caller that does not read the
    /// process environment.
    #[must_use]
    pub const fn new(override_executable: Option<OsString>, search_path: Option<OsString>, working_directory: PathBuf) -> ExecutableLookup {
        ExecutableLookup {
            override_executable,
            search_path,
            working_directory,
        }
    }

    /// The executable the override names, or else the one called `name` on `PATH`.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutableLookupError`] when the override names no executable,
    /// or no override is set and `name` is not on `PATH`.
    pub fn find(&self, name: &str) -> Result<PathBuf, ExecutableLookupError> {
        let search_path = self.search_path.as_ref();
        match &self.override_executable {
            Some(executable) => which::which_in(executable, search_path, &self.working_directory)
                .map_err(|_| ExecutableLookupError::OverrideNotExecutable(PathBuf::from(executable))),
            None => which::which_in(name, search_path, &self.working_directory).map_err(|_| ExecutableLookupError::NotOnPath),
        }
    }
}

/// Why an executable could not be found.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExecutableLookupError {
    /// No override is set and the tool is not on `PATH`.
    #[error("the executable is not on PATH")]
    NotOnPath,
    /// The override names no executable.
    #[error("{} is not an executable", .0.display())]
    OverrideNotExecutable(PathBuf),
}

#[cfg(test)]
mod tests {
    use crate::executable_lookup::{ExecutableLookup, ExecutableLookupError};
    use pretty_assertions::assert_eq;
    use std::io;
    use tempfile::TempDir;

    #[test]
    fn a_name_missing_from_the_path_is_not_found() -> io::Result<()> {
        let empty = TempDir::new()?;
        let lookup = ExecutableLookup::new(None, Some(empty.path().as_os_str().to_owned()), empty.path().to_path_buf());
        assert_eq!(lookup.find("pg_format"), Err(ExecutableLookupError::NotOnPath));
        Ok(())
    }

    #[test]
    fn an_override_naming_no_executable_is_refused_with_its_value() -> io::Result<()> {
        let empty = TempDir::new()?;
        let lookup = ExecutableLookup::new(
            Some("missing_tool".into()),
            Some(empty.path().as_os_str().to_owned()),
            empty.path().to_path_buf(),
        );
        assert_eq!(
            lookup.find("pg_format"),
            Err(ExecutableLookupError::OverrideNotExecutable("missing_tool".into()))
        );
        Ok(())
    }
}
