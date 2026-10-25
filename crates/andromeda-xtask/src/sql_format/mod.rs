//! Canonical formatting of the SQL migrations.
//!
//! Every file under `migrations/` is reformatted with `pg_format`, driven by
//! the repository-root `.pg_format` config plus the `.pg_format_keywords` and
//! `.pg_format_functions` token lists that teach it the extension vocabulary
//! (`TimescaleDB`, `PostGIS`, `COMPRESSION`, `xid8`) its built-in dictionary does
//! not carry. Function statements are held out of `pg_format`
//! ([`function_statement`]) and kept as written. One fixup runs afterwards:
//! `pg_format` unconditionally opens a parenthesised list on a line of its own
//! even when the list is one short line, which no option disables
//! (`identity (\n    cache 1)`, `insert into t (\n    revision)`,
//! `values (\n    0)`), so every such list is rejoined onto the line that opens
//! it. A list spanning several lines is left as formatted.

mod function_statement;

use crate::{cli::repository_root, error::XtaskError, sql_format::function_statement::FunctionShield};
use regex::Regex;
use std::{
    fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use walkdir::WalkDir;

/// A parenthesised list `pg_format` opened on a line of its own whose items
/// sit on the one following line, closing there; the items are captured.
const SINGLE_LINE_LIST: &str = r"\(\n[ \t]*([^\n()]*)\)";

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
    // `pg_format` force-breaks `(\n    item)` even for a one-line list; this
    // rejoins it.
    let single_line_list = Regex::new(SINGLE_LINE_LIST).map_err(XtaskError::SqlFixupPattern)?;
    let shield = FunctionShield::new().map_err(XtaskError::SqlFixupPattern)?;

    let mut drifted: Vec<PathBuf> = Vec::new();
    for path in discover(&root.join("migrations"))? {
        let current = fs::read_to_string(&path).map_err(|source| XtaskError::SqlRead { path: path.clone(), source })?;
        let formatted = format_file(&path, &current, &config, &keywords, &functions, &single_line_list, &shield)?;
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

/// Run `pg_format` over the text of one file, its function statements held
/// out, and rejoin its single-line lists, returning the canonical text without
/// touching the file on disk.
fn format_file(
    path: &Path,
    current: &str,
    config: &Path,
    keywords: &Path,
    functions: &Path,
    single_line_list: &Regex,
    shield: &FunctionShield,
) -> Result<String, XtaskError> {
    let shielded = shield.shield(current);
    let mut child = Command::new("pg_format")
        .arg("--config")
        .arg(config)
        .arg("--extra-keyword")
        .arg(keywords)
        .arg("--extra-function")
        .arg(functions)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|source| {
            if source.kind() == ErrorKind::NotFound {
                XtaskError::PgFormatMissing
            } else {
                XtaskError::Io(source)
            }
        })?;
    // `pg_format` reads its whole input before it writes, so writing all of
    // it first cannot deadlock on a full output pipe.
    child
        .stdin
        .take()
        .ok_or_else(|| XtaskError::PgFormatFailed(path.to_path_buf()))?
        .write_all(shielded.text.as_bytes())
        .map_err(XtaskError::Io)?;
    let output = child.wait_with_output().map_err(XtaskError::Io)?;
    if !output.status.success() {
        return Err(XtaskError::PgFormatFailed(path.to_path_buf()));
    }
    let formatted = String::from_utf8(output.stdout).map_err(|_source| XtaskError::PgFormatOutputNotUtf8(path.to_path_buf()))?;
    let restored = shield
        .restore(&shielded, &formatted)
        .ok_or_else(|| XtaskError::PgFormatLostFunction(path.to_path_buf()))?;
    Ok(rejoin_single_line_lists(single_line_list, &restored))
}

/// `formatted` with every parenthesised list `pg_format` broke onto a line of
/// its own, yet whose items fit on that one line, rejoined onto the line that
/// opens it.
fn rejoin_single_line_lists(single_line_list: &Regex, formatted: &str) -> String {
    single_line_list.replace_all(formatted, "($1)").into_owned()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::sql_format::{SINGLE_LINE_LIST, rejoin_single_line_lists};
    use regex::Regex;

    fn rejoin(formatted: &str) -> String {
        rejoin_single_line_lists(&Regex::new(SINGLE_LINE_LIST).expect("the fixup pattern compiles"), formatted)
    }

    #[test]
    fn an_identity_cache_option_is_rejoined_onto_its_column() {
        assert_eq!(
            rejoin("    subscription_key bigint generated always as identity (\n        cache 1),\n"),
            "    subscription_key bigint generated always as identity (cache 1),\n"
        );
    }

    #[test]
    fn a_one_line_insert_column_list_and_values_list_are_rejoined() {
        assert_eq!(
            rejoin("insert into context_source_change_clock (\n    revision)\nvalues (\n    0);\n"),
            "insert into context_source_change_clock (revision)\nvalues (0);\n"
        );
    }

    #[test]
    fn a_list_spanning_several_lines_is_left_as_formatted() {
        let table = "create table t (\n    a bigint not null,\n    b bigint\n);\n";
        assert_eq!(rejoin(table), table);
        let options = "with (\n    fillfactor = 50,\n    autovacuum_vacuum_threshold = 100\n);\n";
        assert_eq!(rejoin(options), options);
    }

    #[test]
    fn a_list_holding_a_nested_list_is_left_as_formatted() {
        let nested = "check (\n    status in (1, 2))\n";
        assert_eq!(rejoin(nested), nested);
    }
}
