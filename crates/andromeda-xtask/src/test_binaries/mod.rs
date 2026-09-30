//! The compiled integration-test suite binaries and the manifest that maps each
//! suite to its executable. `build` compiles every suite once and packages the
//! binaries beside this manifest; `run` executes one prebuilt binary directly,
//! so a matrix of stacks never recompiles the identical suites per leg.

use crate::{error::XtaskError, stack::Suite};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub(crate) mod build;
pub(crate) mod run;

/// Maps each integration-test suite to its compiled binary. Paths are stored
/// relative to the manifest's own directory so the packaged artifact relocates
/// onto another runner intact.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TestManifest {
    /// Suite name to binary path, each relative to [`Self::base`].
    binaries: BTreeMap<String, PathBuf>,
    /// The directory the relative paths resolve against. Set on load from the
    /// manifest's location, never serialized: the packaged tree carries no
    /// build-machine absolute paths.
    #[serde(skip)]
    base: PathBuf,
}

impl TestManifest {
    /// Assemble a manifest from suite-to-relative-path entries.
    pub(crate) const fn new(binaries: BTreeMap<String, PathBuf>) -> TestManifest {
        TestManifest {
            binaries,
            base: PathBuf::new(),
        }
    }

    /// Read a manifest from disk, resolving its relative paths against its own directory.
    ///
    /// # Errors
    ///
    /// Returns [`XtaskError::TestManifestRead`] if the file cannot be read and
    /// [`XtaskError::TestManifestParse`] if its contents are not valid manifest JSON.
    pub(crate) fn load(path: &Path) -> Result<TestManifest, XtaskError> {
        let text = fs::read_to_string(path).map_err(|source| XtaskError::TestManifestRead {
            path: path.to_path_buf(),
            source,
        })?;
        let mut manifest: TestManifest = serde_json::from_str(&text).map_err(|source| XtaskError::TestManifestParse {
            path: path.to_path_buf(),
            source,
        })?;
        manifest.base = path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
        Ok(manifest)
    }

    /// Write the manifest as pretty JSON.
    ///
    /// # Errors
    ///
    /// Returns [`XtaskError::TestManifestSerialize`] if serialization fails and
    /// [`XtaskError::Io`] if the file cannot be written.
    pub(crate) fn write(&self, path: &Path) -> Result<(), XtaskError> {
        let text = serde_json::to_string_pretty(self).map_err(XtaskError::TestManifestSerialize)?;
        fs::write(path, text)?;
        Ok(())
    }

    /// Resolve a suite's absolute binary path against the manifest's directory.
    ///
    /// # Errors
    ///
    /// Returns [`XtaskError::SuiteBinaryMissing`] if the suite has no entry.
    pub(crate) fn resolve(&self, suite: &'static Suite) -> Result<PathBuf, XtaskError> {
        self.binaries
            .get(suite.name)
            .map(|relative| self.base.join(relative))
            .ok_or(XtaskError::SuiteBinaryMissing(suite))
    }
}

#[cfg(test)]
mod tests {
    use crate::{error::XtaskError, stack::stack_for_suite, test_binaries::TestManifest};
    use std::{collections::BTreeMap, env, fs, path::PathBuf, process};

    #[test]
    fn a_written_manifest_round_trips_and_resolves_against_its_directory() {
        let directory = env::temp_dir().join(format!("andromeda-xtask-manifest-{}", process::id()));
        fs::create_dir_all(&directory).expect("temp dir");
        let manifest_path = directory.join("manifest.json");

        let mut binaries = BTreeMap::new();
        binaries.insert("entity".to_owned(), PathBuf::from("bin/entity"));
        TestManifest::new(binaries).write(&manifest_path).expect("write");

        let loaded = TestManifest::load(&manifest_path).expect("load");
        let entity = stack_for_suite("entity").expect("entity is registered").suite;
        let absent = stack_for_suite("geometry").expect("geometry is registered").suite;
        assert_eq!(loaded.resolve(entity).expect("entity resolves"), directory.join("bin/entity"));
        assert!(matches!(loaded.resolve(absent), Err(XtaskError::SuiteBinaryMissing(missing)) if missing.name == "geometry"));

        fs::remove_dir_all(&directory).expect("cleanup");
    }

    #[test]
    fn an_unreadable_manifest_names_its_path() {
        let path = env::temp_dir().join(format!("andromeda-xtask-manifest-absent-{}.json", process::id()));
        let error = TestManifest::load(&path).expect_err("an absent manifest is rejected");
        assert!(matches!(error, XtaskError::TestManifestRead { path: unread, .. } if unread == path));
    }

    #[test]
    fn a_manifest_that_is_not_manifest_json_names_its_path() {
        let path = env::temp_dir().join(format!("andromeda-xtask-manifest-malformed-{}.json", process::id()));
        fs::write(&path, "[]").expect("write the malformed manifest");
        let error = TestManifest::load(&path).expect_err("a malformed manifest is rejected");
        fs::remove_file(&path).expect("clean up the malformed manifest");
        assert!(matches!(error, XtaskError::TestManifestParse { path: parsed, .. } if parsed == path));
    }
}
