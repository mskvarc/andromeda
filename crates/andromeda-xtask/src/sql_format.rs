//! Canonical formatting of the SQL migrations.
//!
//! Every file under `migrations/` is reformatted with `pg_format`, driven by
//! the repository-root `.pg_format` config plus the `.pg_format_keywords` and
//! `.pg_format_functions` token lists that teach it the extension vocabulary
//! (`TimescaleDB`, `PostGIS`, `COMPRESSION`, `xid8`) its built-in dictionary does
//! not carry. One fixup runs afterwards: `pg_format` unconditionally breaks the
//! `CACHE` option of a `GENERATED ... AS IDENTITY` column onto its own line,
//! which no option disables, so the identity clause is rejoined here.

use crate::{cli::repository_root, error::XtaskError};
use regex::Regex;
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Command,
};
use walkdir::WalkDir;

/// Whether the command rewrites the migrations or only reports drift.
#[derive(Clone, Copy)]
pub(crate) enum FormatMode {
    /// Rewrite each migration whose formatting differs from canonical.
    Write,
    /// Leave files untouched and fail if any is not already canonical.
    Check,
}

/// Reformat every migration, writing or checking according to `mode`.
///
/// # Errors
///
/// Fails if `pg_format` is absent or errors on a file, if a migration cannot be
/// read or written, or, in [`FormatMode::Check`], if any migration is not in
/// canonical format.
pub(crate) fn run(mode: FormatMode) -> Result<(), XtaskError> {
    let root = repository_root();
    let config = root.join(".pg_format");
    let keywords = root.join(".pg_format_keywords");
    let functions = root.join(".pg_format_functions");
    // `pg_format` force-breaks `identity (\n    cache N)`; this rejoins it.
    let identity_cache = Regex::new(r"identity \(\n[ \t]*(cache \d+)\)").map_err(XtaskError::SqlFixupPattern)?;

    let mut drifted: Vec<PathBuf> = Vec::new();
    for path in discover(&root.join("migrations"))? {
        let formatted = format_file(&path, &config, &keywords, &functions, &identity_cache)?;
        let current = fs::read_to_string(&path).map_err(|source| XtaskError::SqlRead { path: path.clone(), source })?;
        if formatted == current {
            continue;
        }
        match mode {
            FormatMode::Write => {
                fs::write(&path, &formatted).map_err(|source| XtaskError::SqlWrite { path: path.clone(), source })?;
                println!("formatted {}", path.display());
            }
            FormatMode::Check => drifted.push(path),
        }
    }

    if let FormatMode::Check = mode
        && !drifted.is_empty()
    {
        for path in &drifted {
            eprintln!("not formatted: {}", path.display());
        }
        return Err(XtaskError::SqlFormatDrift(drifted.len()));
    }
    Ok(())
}

/// Collect every `*.sql` file under `migrations`, in a deterministic order.
fn discover(migrations: &Path) -> Result<Vec<PathBuf>, XtaskError> {
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in WalkDir::new(migrations).sort_by_file_name() {
        let entry = entry.map_err(XtaskError::SqlDiscover)?;
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "sql") {
            files.push(path.to_path_buf());
        }
    }
    Ok(files)
}

/// Run `pg_format` over one file and apply the identity-cache fixup, returning
/// the canonical text without touching the file on disk.
fn format_file(path: &Path, config: &Path, keywords: &Path, functions: &Path, identity_cache: &Regex) -> Result<String, XtaskError> {
    let output = Command::new("pg_format")
        .arg("--config")
        .arg(config)
        .arg("--extra-keyword")
        .arg(keywords)
        .arg("--extra-function")
        .arg(functions)
        .arg(path)
        .output()
        .map_err(|source| {
            if source.kind() == ErrorKind::NotFound {
                XtaskError::PgFormatMissing
            } else {
                XtaskError::Io(source)
            }
        })?;
    if !output.status.success() {
        return Err(XtaskError::PgFormatFailed(path.to_path_buf()));
    }
    let formatted = String::from_utf8(output.stdout).map_err(|_source| XtaskError::PgFormatOutputNotUtf8(path.to_path_buf()))?;
    Ok(identity_cache.replace_all(&formatted, "identity ($1)").into_owned())
}
