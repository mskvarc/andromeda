//! A git revision of the repository, and the reading of one file as that
//! revision holds it.
//!
//! The repository is read in-process through `gix`, read-only: the revision is
//! resolved to its tree and the file looked up there, so a path the tree does
//! not hold is told apart from a revision that does not exist.

use std::{
    path::{Path, PathBuf},
    string::FromUtf8Error,
};

/// A revision as `git rev-parse` reads it: a branch, a tag, a commit or an
/// expression over them.
#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display, derive_more::FromStr)]
pub struct GitRevision(String);

impl GitRevision {
    /// The text of `path`, relative to the repository root, in the tree of this
    /// revision of the repository holding `root`; absent when the tree holds no
    /// such file.
    ///
    /// # Errors
    ///
    /// Returns [`GitRevisionError`] when `root` is in no repository, the
    /// revision names no commit or tree, or the file cannot be read as UTF-8.
    pub fn read_file(&self, root: &Path, path: &Path) -> Result<Option<String>, GitRevisionError> {
        let repository = gix::discover(root).map_err(|source| GitRevisionError::Repository {
            root: root.to_path_buf(),
            source,
        })?;
        let unresolved = |source| GitRevisionError::Revision {
            revision: self.clone(),
            source,
        };
        let tree = repository
            .rev_parse_single(self.0.as_str())
            .and_then(|id| id.object())
            .and_then(gix::Object::peel_to_tree)
            .map_err(unresolved)?;
        let Some(entry) = tree.lookup_entry_by_path(path).map_err(unresolved)? else {
            return Ok(None);
        };
        let blob = entry.object().map_err(unresolved)?.detach();
        String::from_utf8(blob.data).map(Some).map_err(|source| GitRevisionError::NotUtf8 {
            revision: self.clone(),
            path: path.to_path_buf(),
            source,
        })
    }
}

/// Why a file could not be read at a revision.
#[derive(Debug, thiserror::Error)]
pub enum GitRevisionError {
    /// The root is in no git repository.
    #[error("{} is in no git repository: {source}", root.display())]
    Repository {
        /// The root concerned.
        root: PathBuf,
        /// Why the repository could not be opened.
        source: gix::Error,
    },
    /// The revision or its tree could not be read.
    #[error("{revision} could not be read: {source}")]
    Revision {
        /// The revision concerned.
        revision: GitRevision,
        /// Why it could not be read.
        source: gix::Error,
    },
    /// The file at the revision is not UTF-8.
    #[error("{} at {revision} is not UTF-8: {source}", path.display())]
    NotUtf8 {
        /// The revision concerned.
        revision: GitRevision,
        /// The file concerned.
        path: PathBuf,
        /// Why it is not text.
        source: FromUtf8Error,
    },
}
