//! A repository root built in a temporary directory, holding the committed
//! Squawk and `pg_format` configuration and the migrations a test writes.

use std::{
    fs,
    io,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

/// A migration body in its canonical layout that every rule passes.
pub(crate) const CLEAN: &str = "create table entities (\n    entity_key bigint generated always as identity (cache 1),\n    kind text compression lz4 collate \"C\" not null,\n    constraint pk_entities primary key (entity_key)\n);\n";

/// The repository the package is built from.
pub(crate) fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A repository root holding the committed Squawk and `pg_format`
/// configuration, an empty formatter vocabulary, and the given migrations.
pub(crate) fn repository_with(migrations: &[(&str, &str)]) -> io::Result<TempDir> {
    let root = TempDir::new()?;
    for file in [".squawk.toml", ".pg_format"] {
        fs::copy(repository_root().join(file), root.path().join(file))?;
    }
    for file in [".pg_format_functions", ".pg_format_keywords"] {
        fs::write(root.path().join(file), "")?;
    }
    for (file, text) in migrations {
        write_migration(root.path(), file, text)?;
    }
    Ok(root)
}

/// Write the migration `file`, `<set>/<name>`, below `root` with `text`.
pub(crate) fn write_migration(root: &Path, file: &str, text: &str) -> io::Result<()> {
    let path = root.join("migrations").join(file);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text)
}

/// The header every migration opens with, for version `version` titled
/// `title`.
pub(crate) fn header(version: &str, title: &str) -> String {
    let rule = format!("-- {}", "=".repeat(76));
    format!("{rule}\n-- {version}: {title}\n{rule}\n-- Fixture.\n{rule}\n")
}
