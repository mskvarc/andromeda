//! `andromeda-sql lint`, run as a process against Squawk and `pg_format`.

/// The repository fixtures the process tests run against.
mod repository_fixture;

use crate::repository_fixture::{CLEAN, header, repository_root, repository_with};
use pretty_assertions::assert_eq;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    fs,
    io,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

fn lint(root: &Path) -> io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_andromeda-sql"))
        .arg("--root")
        .arg(root)
        .arg("lint")
        .env_remove("SQUAWK")
        .env_remove("PG_FORMAT")
        .output()
}

/// A volatile default makes PostgreSQL rewrite the table under an `ACCESS
/// EXCLUSIVE` lock; a stable one such as `now()` does not since PostgreSQL 11,
/// so Squawk only flags the volatile kind.
const VOLATILE_DEFAULT: &str =
    "-- Records when each Entity was created.\nalter table entities\n    add column created_at timestamptz default clock_timestamp();\n";

/// A table of fixed-width columns only, so a column added to it lands after
/// no variable-length one.
const FIXED_WIDTH: &str =
    "create table entities (\n    entity_key bigint generated always as identity (cache 1),\n    constraint pk_entities primary key (entity_key)\n);\n";

#[test]
fn every_committed_migration_passes_the_lint() -> io::Result<()> {
    let output = lint(&repository_root())?;
    assert_eq!(
        (output.status.success(), String::from_utf8_lossy(&output.stderr).into_owned()),
        (true, String::new())
    );
    Ok(())
}

#[test]
fn a_clean_migration_passes() -> io::Result<()> {
    let root = repository_with(&[("tenant/0001_entities.sql", &format!("{}{CLEAN}", header("0001", "Entities")))])?;
    let output = lint(root.path())?;
    assert_eq!(
        (output.status.success(), String::from_utf8_lossy(&output.stderr).into_owned()),
        (true, String::new())
    );
    Ok(())
}

#[test]
fn a_column_added_with_a_volatile_default_fails_at_its_line() -> io::Result<()> {
    let root = repository_with(&[
        ("tenant/0001_entities.sql", &format!("{}{FIXED_WIDTH}", header("0001", "Entities"))),
        (
            "tenant/0002_entity_created_at.sql",
            &format!("{}{VOLATILE_DEFAULT}", header("0002", "Entity creation instant")),
        ),
    ])?;
    let output = lint(root.path())?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.success(), false, "{stderr}");
    for expected in [
        "adding-field-with-default",
        "migrations/tenant/0002_entity_created_at.sql:8:",
        "add column created_at timestamptz default clock_timestamp();",
        "andromeda-sql: 0 error(s) and 1 warning(s) in 1 file(s)",
    ] {
        assert!(stderr.contains(expected), "{expected:?} missing from:\n{stderr}");
    }
    Ok(())
}

#[test]
fn a_missing_squawk_is_a_named_error() -> io::Result<()> {
    let root = repository_with(&[("tenant/0001_entities.sql", CLEAN)])?;
    let empty = TempDir::new()?;
    let output = Command::new(env!("CARGO_BIN_EXE_andromeda-sql"))
        .arg("--root")
        .arg(root.path())
        .arg("lint")
        .env("PATH", empty.path())
        .env_remove("SQUAWK")
        .env_remove("PG_FORMAT")
        .output()?;
    assert_eq!(
        (output.status.success(), String::from_utf8_lossy(&output.stderr).into_owned()),
        (
            false,
            "andromeda-sql: squawk is not installed; install Squawk 2.70.0 or set SQUAWK\n".to_owned()
        )
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn another_squawk_release_is_refused() -> io::Result<()> {
    let root = repository_with(&[("tenant/0001_entities.sql", CLEAN)])?;
    let directory = TempDir::new()?;
    let script = directory.path().join("squawk");
    fs::write(&script, "#!/bin/sh\necho 'squawk 2.71.0'\n")?;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
    let output = Command::new(env!("CARGO_BIN_EXE_andromeda-sql"))
        .arg("--root")
        .arg(root.path())
        .arg("lint")
        .env("SQUAWK", &script)
        .output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.success(), false);
    assert!(stderr.ends_with("is Squawk 2.71.0; the rules are pinned to Squawk 2.70.0\n"), "{stderr}");
    Ok(())
}

#[test]
fn a_migration_squawk_cannot_parse_is_an_error() -> io::Result<()> {
    let root = repository_with(&[("tenant/0001_entities.sql", "selec 1;\n")])?;
    let output = lint(root.path())?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.success(), false, "{stderr}");
    for expected in [
        "syntax-error",
        "migrations/tenant/0001_entities.sql:1:",
        "andromeda-sql: 2 error(s) and 0 warning(s) in 1 file(s)",
    ] {
        assert!(stderr.contains(expected), "{expected:?} missing from:\n{stderr}");
    }
    Ok(())
}

#[test]
fn an_andromeda_rule_finding_is_reported_where_it_is_and_counted() -> io::Result<()> {
    let uncompressed = "create table entities (\n    entity_key bigint not null,\n    kind text collate \"C\" not null,\n    constraint pk_entities primary key (entity_key)\n);\n";
    let root = repository_with(&[("tenant/0001_entities.sql", &format!("{}{uncompressed}", header("0001", "Entities")))])?;
    let output = lint(root.path())?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.success(), false, "{stderr}");
    for expected in [
        "lz4-compression",
        "migrations/tenant/0001_entities.sql:8:5",
        "entities.kind is text, which PostgreSQL compresses, and declares no compression lz4",
        "help: declare `compression lz4` on the column",
        "andromeda-sql: 0 error(s) and 1 warning(s) in 1 file(s)",
    ] {
        assert!(stderr.contains(expected), "{expected:?} missing from:\n{stderr}");
    }
    Ok(())
}

#[test]
fn a_stale_vocabulary_word_is_reported_in_its_file() -> io::Result<()> {
    let root = repository_with(&[("tenant/0001_entities.sql", &format!("{}{CLEAN}", header("0001", "Entities")))])?;
    fs::write(root.path().join(".pg_format_functions"), "sub_path_hash\n")?;
    let output = lint(root.path())?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.success(), false, "{stderr}");
    for expected in [
        "formatter-vocabulary",
        "[.pg_format_functions:1:1]",
        "no migration uses sub_path_hash, which .pg_format_functions lists",
    ] {
        assert!(stderr.contains(expected), "{expected:?} missing from:\n{stderr}");
    }
    Ok(())
}
