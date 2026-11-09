//! Every migration below `migrations/`, read from disk and checked against the
//! layout and the one version space all sets share.
//!
//! `migrations/` holds only the set directories and the seal ledger, and a set
//! directory holds only migration files. Names starting with a dot (editor and
//! file-manager droppings) are passed over at both levels.

use crate::{
    migration_file::{MigrationFile, MigrationFileError},
    migration_set::MigrationSet,
    migration_version::MigrationVersion,
    seal_ledger::LEDGER_FILE_NAME,
};
use std::{
    collections::{BTreeMap, btree_map::Entry},
    ffi::OsStr,
    fs,
    io,
    path::{Path, PathBuf},
};

/// The directory below the repository root that holds the set directories.
const MIGRATIONS_DIRECTORY: &str = "migrations";

/// Every migration of the repository, ordered by version.
#[derive(Debug)]
pub struct MigrationTree {
    /// The migrations, keyed by the version each holds alone.
    files: BTreeMap<MigrationVersion, MigrationFile>,
}

impl MigrationTree {
    /// Read every migration below `root/migrations`.
    ///
    /// # Errors
    ///
    /// Returns [`MigrationTreeError`] when a directory cannot be listed, an
    /// entry is neither a set directory nor a migration file, a file name breaks
    /// the naming rule, or two files hold the same version.
    pub fn read(root: &Path) -> Result<MigrationTree, MigrationTreeError> {
        let mut files = BTreeMap::new();
        for set_directory in visible_entries(&root.join(MIGRATIONS_DIRECTORY))?
            .into_iter()
            .filter(|entry| entry.file_name().is_none_or(|name| name != LEDGER_FILE_NAME) || !entry.is_file())
        {
            let set = set_of(&set_directory)?;
            for path in visible_entries(&set_directory)? {
                if !path.is_file() {
                    return Err(MigrationTreeError::NotAFile(path));
                }
                let file = MigrationFile::new(set, path)?;
                match files.entry(file.version()) {
                    Entry::Vacant(vacant) => {
                        vacant.insert(file);
                    }
                    Entry::Occupied(occupied) => {
                        return Err(MigrationTreeError::VersionHeldTwice {
                            held: Box::new(occupied.remove()),
                            repeated: Box::new(file),
                        });
                    }
                }
            }
        }
        Ok(MigrationTree { files })
    }

    /// The migrations, in version order.
    pub fn into_files(self) -> impl Iterator<Item = MigrationFile> {
        self.files.into_values()
    }
}

/// The set a directory below `migrations/` names.
fn set_of(directory: &Path) -> Result<MigrationSet, MigrationTreeError> {
    let set = directory.file_name().and_then(OsStr::to_str).and_then(|name| name.parse().ok());
    match set {
        Some(set) if directory.is_dir() => Ok(set),
        Some(_) | None => Err(MigrationTreeError::NotASet(directory.to_path_buf())),
    }
}

/// The entries of `directory` whose names do not start with a dot, in name order.
fn visible_entries(directory: &Path) -> Result<Vec<PathBuf>, MigrationTreeError> {
    let unlisted = |source| MigrationTreeError::Unlisted {
        directory: directory.to_path_buf(),
        source,
    };
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory).map_err(unlisted)? {
        let entry = entry.map_err(unlisted)?;
        if !entry.file_name().as_encoded_bytes().starts_with(b".") {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

/// Why the migrations could not be read.
#[derive(Debug, thiserror::Error)]
pub enum MigrationTreeError {
    /// A directory could not be listed.
    #[error("{} could not be listed: {source}", directory.display())]
    Unlisted {
        /// The directory concerned.
        directory: PathBuf,
        /// What the listing failed with.
        source: io::Error,
    },
    /// An entry of `migrations/` is not the directory of a migration set.
    #[error("{} is not one of the migration set directories platform, tenant or branch", .0.display())]
    NotASet(PathBuf),
    /// An entry of a set directory is not a regular file.
    #[error("{} is not a migration file", .0.display())]
    NotAFile(PathBuf),
    /// A file name breaks the naming rule.
    #[error(transparent)]
    Name(#[from] MigrationFileError),
    /// Two files hold the same version, which every set draws from one space.
    #[error("version {} is held by both {held} and {repeated}", held.version())]
    VersionHeldTwice {
        /// The file read first.
        held: Box<MigrationFile>,
        /// The file read second, holding the same version.
        repeated: Box<MigrationFile>,
    },
}

#[cfg(test)]
mod tests {
    use crate::migration_tree::{MigrationTree, MigrationTreeError};
    use pretty_assertions::assert_eq;
    use std::{fs, io, path::Path};
    use tempfile::TempDir;

    fn tree_with(files: &[&str]) -> io::Result<TempDir> {
        let root = TempDir::new()?;
        for file in files {
            let path = root.path().join("migrations").join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, "select 1;\n")?;
        }
        Ok(root)
    }

    fn read(root: &Path) -> Result<Vec<String>, String> {
        MigrationTree::read(root)
            .map(|tree| tree.into_files().map(|file| file.to_string()).collect())
            .map_err(|error| error.to_string())
    }

    #[test]
    fn the_files_of_every_set_are_listed_in_one_version_order() -> io::Result<()> {
        let root = tree_with(&[
            "tenant/0003_types.sql",
            "platform/0001_tenants.sql",
            "branch/0002_entities.sql",
            "tenant/.DS_Store",
            ".DS_Store",
            "seal-ledger.toml",
        ])?;
        assert_eq!(
            read(root.path()),
            Ok(vec![
                "migrations/platform/0001_tenants.sql".to_owned(),
                "migrations/branch/0002_entities.sql".to_owned(),
                "migrations/tenant/0003_types.sql".to_owned(),
            ])
        );
        Ok(())
    }

    #[test]
    fn a_version_held_in_two_sets_is_refused() -> io::Result<()> {
        let root = tree_with(&["tenant/0002_types.sql", "branch/0002_entities.sql"])?;
        assert_eq!(
            read(root.path()),
            Err("version 0002 is held by both migrations/branch/0002_entities.sql and migrations/tenant/0002_types.sql".to_owned())
        );
        Ok(())
    }

    #[test]
    fn a_misnamed_file_is_refused_with_its_path() -> io::Result<()> {
        let root = tree_with(&["tenant/002_types.sql"])?;
        let error = MigrationTree::read(root.path()).err();
        assert!(matches!(error, Some(MigrationTreeError::Name(_))), "{error:?}");
        let message = error.map_or_default(|error| error.to_string());
        assert!(
            message.ends_with("migrations/tenant/002_types.sql does not match NNNN_snake_case.sql: a migration version is exactly four decimal digits"),
            "{message}"
        );
        Ok(())
    }

    #[test]
    fn a_directory_that_names_no_set_is_refused() -> io::Result<()> {
        let root = tree_with(&["snapshot/0002_types.sql"])?;
        assert!(matches!(MigrationTree::read(root.path()), Err(MigrationTreeError::NotASet(_))));
        Ok(())
    }

    #[test]
    fn a_directory_inside_a_set_is_refused() -> io::Result<()> {
        let root = tree_with(&["tenant/nested/0002_types.sql"])?;
        assert!(matches!(MigrationTree::read(root.path()), Err(MigrationTreeError::NotAFile(_))));
        Ok(())
    }

    #[test]
    fn a_root_without_migrations_is_refused() -> io::Result<()> {
        let root = tree_with(&[])?;
        assert!(matches!(MigrationTree::read(root.path()), Err(MigrationTreeError::Unlisted { .. })));
        Ok(())
    }
}
