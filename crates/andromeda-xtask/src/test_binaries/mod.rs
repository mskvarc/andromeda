//! The compiled integration-test suite binaries and the manifest that maps each
//! suite to its executable. `build` compiles every suite once and packages the
//! binaries beside this manifest; `run` executes one prebuilt binary directly,
//! so a matrix of stacks never recompiles the identical suites per leg.

use crate::error::XtaskError;
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
    /// [`XtaskError::TestManifestSerde`] if its contents are not valid manifest JSON.
    pub(crate) fn load(path: &Path) -> Result<TestManifest, XtaskError> {
        let text = fs::read_to_string(path).map_err(|error| XtaskError::TestManifestRead(format!("{}: {error}", path.display())))?;
        let mut manifest: TestManifest = serde_json::from_str(&text).map_err(|error| XtaskError::TestManifestSerde(format!("{}: {error}", path.display())))?;
        manifest.base = path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
        Ok(manifest)
    }

    /// Write the manifest as pretty JSON.
    ///
    /// # Errors
    ///
    /// Returns [`XtaskError::TestManifestSerde`] if serialization fails and
    /// [`XtaskError::Io`] if the file cannot be written.
    pub(crate) fn write(&self, path: &Path) -> Result<(), XtaskError> {
        let text = serde_json::to_string_pretty(self).map_err(|error| XtaskError::TestManifestSerde(format!("serialize: {error}")))?;
        fs::write(path, text)?;
        Ok(())
    }

    /// Resolve a suite's absolute binary path against the manifest's directory.
    ///
    /// # Errors
    ///
    /// Returns [`XtaskError::SuiteBinaryMissing`] if the suite has no entry.
    pub(crate) fn resolve(&self, suite: &str) -> Result<PathBuf, XtaskError> {
        self.binaries
            .get(suite)
            .map(|relative| self.base.join(relative))
            .ok_or_else(|| XtaskError::SuiteBinaryMissing(suite.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use crate::test_binaries::TestManifest;
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
        assert_eq!(loaded.resolve("entity").expect("entity resolves"), directory.join("bin/entity"));
        assert!(loaded.resolve("absent").is_err());

        fs::remove_dir_all(&directory).expect("cleanup");
    }
}
