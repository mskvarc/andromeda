//! `andromeda-sql format`, run as a process against `pg_format`.

use pretty_assertions::assert_eq;
use std::{
    fs,
    io,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

/// The repository the package is built from.
fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A repository root holding the committed `pg_format` configuration and the
/// given migrations.
fn repository_with(migrations: &[(&str, &str)]) -> io::Result<TempDir> {
    let root = TempDir::new()?;
    for configuration in [".pg_format", ".pg_format_keywords", ".pg_format_functions"] {
        fs::copy(repository_root().join(configuration), root.path().join(configuration))?;
    }
    for (file, text) in migrations {
        let path = root.path().join("migrations").join(file);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, text)?;
    }
    Ok(root)
}

fn format(root: &Path, arguments: &[&str]) -> io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_andromeda-sql"))
        .arg("--root")
        .arg(root)
        .arg("format")
        .args(arguments)
        .output()
}

fn read(root: &Path, file: &str) -> io::Result<String> {
    fs::read_to_string(root.join("migrations").join(file))
}

const CANONICAL: &str = "create table tenants (\n    tenant_key bigint generated always as identity (cache 1) primary key,\n    name text not null\n);\n";

const DRIFTED: &str = "create table entities (entity_key bigint generated always as identity (cache 1) primary key, kind text not null);\n\n\n";

const DRIFTED_CANONICAL: &str =
    "create table entities (\n    entity_key bigint generated always as identity (cache 1) primary key,\n    kind text not null\n);\n";

const FUNCTIONS: &str = "-- Refuses every change.\ncreate function refuse_change ()\n    returns trigger\n    language plpgsql\n    as $$\nbegin\n    raise   exception 'no (\n        change)';\nend;\n$$;\n\n-- Doubles.\ncreate or replace function double_it (x integer)\n    returns integer\n    language sql\n    as 'select  x *  2';\n\nCREATE   TABLE t (a integer);\n";

const FUNCTIONS_CANONICAL: &str = "-- Refuses every change.\ncreate function refuse_change ()\n    returns trigger\n    language plpgsql\n    as $$\nbegin\n    raise   exception 'no (\n        change)';\nend;\n$$;\n\n-- Doubles.\ncreate or replace function double_it (x integer)\n    returns integer\n    language sql\n    as 'select  x *  2';\n\ncreate table t (\n    a integer\n);\n";

#[test]
fn every_committed_migration_is_canonical() -> io::Result<()> {
    let output = format(&repository_root(), &["--check"])?;
    assert_eq!(
        (output.status.success(), String::from_utf8_lossy(&output.stderr).into_owned()),
        (true, String::new())
    );
    Ok(())
}

#[test]
fn a_check_lists_every_drifted_file_writes_nothing_and_fails() -> io::Result<()> {
    let root = repository_with(&[
        ("platform/0001_tenants.sql", CANONICAL),
        ("tenant/0002_entities.sql", DRIFTED),
        ("branch/0003_types.sql", DRIFTED),
    ])?;
    let output = format(root.path(), &["--check"])?;
    assert_eq!(output.status.success(), false);
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "not formatted: migrations/tenant/0002_entities.sql\nnot formatted: migrations/branch/0003_types.sql\nandromeda-sql: 2 migration(s) are not formatted; run `cargo run -p andromeda-sql -- format` to fix\n"
    );
    assert_eq!(
        (read(root.path(), "tenant/0002_entities.sql")?, read(root.path(), "branch/0003_types.sql")?),
        (DRIFTED.to_owned(), DRIFTED.to_owned())
    );
    Ok(())
}

#[test]
fn a_rewrite_formats_the_drifted_files_and_a_second_run_changes_nothing() -> io::Result<()> {
    let root = repository_with(&[("platform/0001_tenants.sql", CANONICAL), ("tenant/0002_entities.sql", DRIFTED)])?;
    let first = format(root.path(), &[])?;
    assert_eq!(
        (first.status.success(), String::from_utf8_lossy(&first.stdout).into_owned()),
        (true, "formatted migrations/tenant/0002_entities.sql\n".to_owned())
    );
    assert_eq!(read(root.path(), "tenant/0002_entities.sql")?, DRIFTED_CANONICAL);
    let second = format(root.path(), &[])?;
    assert_eq!(
        (second.status.success(), String::from_utf8_lossy(&second.stdout).into_owned()),
        (true, String::new())
    );
    assert_eq!(read(root.path(), "platform/0001_tenants.sql")?, CANONICAL);
    Ok(())
}

#[test]
fn function_statements_keep_their_text_through_pg_format() -> io::Result<()> {
    let root = repository_with(&[("tenant/0001_functions.sql", FUNCTIONS)])?;
    let output = format(root.path(), &[])?;
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(read(root.path(), "tenant/0001_functions.sql")?, FUNCTIONS_CANONICAL);
    Ok(())
}

#[test]
fn a_misnamed_migration_is_a_named_error() -> io::Result<()> {
    let root = repository_with(&[("tenant/0001_entities.sql", CANONICAL), ("tenant/0002-Types.sql", CANONICAL)])?;
    let output = format(root.path(), &["--check"])?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.success(), false);
    assert!(stderr.contains("0002-Types.sql does not match NNNN_snake_case.sql"), "{stderr}");
    Ok(())
}

#[test]
fn a_missing_pg_format_is_a_named_error() -> io::Result<()> {
    let root = repository_with(&[("tenant/0001_entities.sql", CANONICAL)])?;
    let empty = TempDir::new()?;
    let output = Command::new(env!("CARGO_BIN_EXE_andromeda-sql"))
        .arg("--root")
        .arg(root.path())
        .args(["format", "--check"])
        .env("PATH", empty.path())
        .env_remove("PG_FORMAT")
        .output()?;
    assert_eq!(
        (output.status.success(), String::from_utf8_lossy(&output.stderr).into_owned()),
        (
            false,
            "andromeda-sql: pg_format is not installed; install pgFormatter or set PG_FORMAT\n".to_owned()
        )
    );
    Ok(())
}
