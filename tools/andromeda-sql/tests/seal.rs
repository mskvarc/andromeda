//! `andromeda-sql seal` and `verify`, run as a process against Squawk,
//! `pg_format` and a git repository built for each test.

/// The repository fixtures the process tests run against.
mod repository_fixture;

use crate::repository_fixture::{CLEAN, header, repository_with, write_migration};
use andromeda_sql::file_digest::FileDigest;
use pretty_assertions::assert_eq;
use std::{
    fs,
    io,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

/// The first migration of every fixture.
const ENTITIES: &str = "tenant/0001_entities.sql";

/// The ledger's path below the root.
const LEDGER: &str = "migrations/seal-ledger.toml";

/// A second migration, canonical and clean.
const KINDS: &str = "create table kinds (\n    kind_key bigint generated always as identity (cache 1),\n    constraint pk_kinds primary key (kind_key)\n);\n";

fn entities() -> String {
    format!("{}{CLEAN}", header("0001", "Entities"))
}

fn kinds(version: &str) -> String {
    format!("{}{KINDS}", header(version, "Kinds"))
}

fn run(root: &Path, arguments: &[&str]) -> io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_andromeda-sql"))
        .arg("--root")
        .arg(root)
        .args(arguments)
        .env_remove("SQUAWK")
        .env_remove("PG_FORMAT")
        .output()
}

/// Whether the run succeeded, and what it printed on standard error.
fn status(output: &Output) -> (bool, String) {
    (output.status.success(), String::from_utf8_lossy(&output.stderr).into_owned())
}

/// Run git in `root` with a fixture identity, the global configuration left
/// out so no signing or hook of the machine applies.
fn git(root: &Path, arguments: &[&str]) -> io::Result<Output> {
    Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid"])
        .args(arguments)
        .output()
}

/// Commit everything below `root` to a repository made there.
fn commit_all(root: &Path) -> io::Result<()> {
    for arguments in [&["init", "--quiet"][..], &["add", "--all"], &["commit", "--quiet", "--message", "fixture"]] {
        let output = git(root, arguments)?;
        assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}

/// A repository whose one migration is sealed by v1.0.0.
fn sealed_repository() -> io::Result<TempDir> {
    let root = repository_with(&[(ENTITIES, &entities())])?;
    assert_eq!(status(&run(root.path(), &["seal", "v1.0.0"])?), (true, String::new()));
    Ok(root)
}

/// The violations `verify` prints, with its closing count.
fn verify(root: &Path, arguments: &[&str]) -> io::Result<(bool, String)> {
    Ok(status(&run(root, &[&["verify"][..], arguments].concat())?))
}

#[test]
fn a_seal_writes_one_section_and_a_second_seal_with_no_new_migration_is_refused() -> io::Result<()> {
    let root = sealed_repository()?;
    let digest = FileDigest::of(entities().as_bytes());
    assert_eq!(
        fs::read_to_string(root.path().join(LEDGER))?,
        format!(
            "# Written by `andromeda-sql seal`. A sealed migration and its section never change.\n\nalgorithm = \"sha-256\"\n\n[[release]]\nversion = \"v1.0.0\"\n\n[[release.migration]]\nset = \"tenant\"\nname = \"0001_entities.sql\"\nsha256 = \"{digest}\"\n"
        )
    );
    assert_eq!(verify(root.path(), &[])?, (true, String::new()));
    assert_eq!(
        status(&run(root.path(), &["seal", "v1.1.0"])?),
        (
            false,
            "andromeda-sql: every migration is sealed already, so v1.1.0 has nothing to seal\n".to_owned()
        )
    );
    Ok(())
}

#[test]
fn a_later_seal_appends_a_section_holding_only_the_new_migration() -> io::Result<()> {
    let root = sealed_repository()?;
    let before = fs::read_to_string(root.path().join(LEDGER))?;
    write_migration(root.path(), "tenant/0002_kinds.sql", &kinds("0002"))?;
    assert_eq!(status(&run(root.path(), &["seal", "v1.1.0"])?), (true, String::new()));
    let after = fs::read_to_string(root.path().join(LEDGER))?;
    let digest = FileDigest::of(kinds("0002").as_bytes());
    assert_eq!(
        after,
        format!("{before}\n[[release]]\nversion = \"v1.1.0\"\n\n[[release.migration]]\nset = \"tenant\"\nname = \"0002_kinds.sql\"\nsha256 = \"{digest}\"\n")
    );
    Ok(())
}

#[test]
fn a_version_not_after_the_last_sealed_release_is_refused() -> io::Result<()> {
    let root = sealed_repository()?;
    write_migration(root.path(), "tenant/0002_kinds.sql", &kinds("0002"))?;
    assert_eq!(
        status(&run(root.path(), &["seal", "v1.0.0"])?),
        (false, "andromeda-sql: v1.0.0 does not follow v1.0.0, the last sealed release\n".to_owned())
    );
    Ok(())
}

#[test]
fn a_pre_release_is_never_sealed() -> io::Result<()> {
    let root = repository_with(&[(ENTITIES, &entities())])?;
    for tag in ["v1.0.0-alpha.1", "v1.0.0-beta.2", "v1.0.0-rc.1"] {
        let (succeeded, stderr) = status(&run(root.path(), &["seal", tag])?);
        assert_eq!(succeeded, false, "{tag}");
        assert!(stderr.contains(&format!("invalid value '{tag}'")), "{stderr}");
        assert!(stderr.contains("only a stable release vX.Y.Z is sealed"), "{stderr}");
    }
    assert!(!root.path().join(LEDGER).exists());
    Ok(())
}

#[test]
fn a_seal_is_refused_while_the_lint_fails() -> io::Result<()> {
    let uncompressed = "create table entities (\n    entity_key bigint not null,\n    kind text collate \"C\" not null,\n    constraint pk_entities primary key (entity_key)\n);\n";
    let root = repository_with(&[(ENTITIES, &format!("{}{uncompressed}", header("0001", "Entities")))])?;
    let (succeeded, stderr) = status(&run(root.path(), &["seal", "v1.0.0"])?);
    assert_eq!(succeeded, false);
    for expected in ["lz4-compression", "andromeda-sql: the lint fails, so nothing is sealed"] {
        assert!(stderr.contains(expected), "{expected:?} missing from:\n{stderr}");
    }
    assert!(!root.path().join(LEDGER).exists());
    Ok(())
}

#[test]
fn a_seal_is_refused_while_a_migration_is_not_formatted() -> io::Result<()> {
    let drifted = format!("{}create table kinds (kind_key bigint not null);\n", header("0001", "Kinds"));
    let root = repository_with(&[("tenant/0001_kinds.sql", &drifted)])?;
    assert_eq!(
        status(&run(root.path(), &["seal", "v1.0.0"])?),
        (
            false,
            "not formatted: migrations/tenant/0001_kinds.sql\nandromeda-sql: 1 migration(s) are not formatted, so nothing is sealed\n".to_owned()
        )
    );
    Ok(())
}

#[test]
fn an_edited_sealed_migration_fails_verify_and_blocks_the_next_seal() -> io::Result<()> {
    let root = sealed_repository()?;
    let edited = entities().replace("kind text", "kind_name text");
    write_migration(root.path(), ENTITIES, &edited)?;
    let expected = format!(
        "migrations/tenant/0001_entities.sql: sealed by v1.0.0 and edited since; a sealed migration never changes (sealed as {}, now {})\n",
        FileDigest::of(entities().as_bytes()),
        FileDigest::of(edited.as_bytes())
    );
    assert_eq!(
        verify(root.path(), &[])?,
        (false, format!("{expected}andromeda-sql: 1 violation(s) of the seal\n"))
    );
    write_migration(root.path(), "tenant/0002_kinds.sql", &kinds("0002"))?;
    assert_eq!(
        status(&run(root.path(), &["seal", "v1.1.0"])?),
        (
            false,
            format!("{expected}andromeda-sql: 1 violation(s) of the seal\nandromeda-sql: the seal is broken, so nothing is sealed\n")
        )
    );
    Ok(())
}

#[test]
fn a_deleted_sealed_migration_fails_verify() -> io::Result<()> {
    let root = sealed_repository()?;
    fs::remove_file(root.path().join("migrations").join(ENTITIES))?;
    assert_eq!(
        verify(root.path(), &[])?,
        (
            false,
            "migrations/tenant/0001_entities.sql: sealed by v1.0.0 and deleted or renamed since; a sealed migration stays where it was sealed\nandromeda-sql: 1 violation(s) of the seal\n".to_owned()
        )
    );
    Ok(())
}

#[test]
fn a_renamed_sealed_migration_fails_verify() -> io::Result<()> {
    let root = sealed_repository()?;
    let migrations = root.path().join("migrations");
    fs::rename(migrations.join(ENTITIES), migrations.join("tenant/0001_entity_rows.sql"))?;
    assert_eq!(
        verify(root.path(), &[])?,
        (
            false,
            "migrations/tenant/0001_entities.sql: sealed by v1.0.0 and deleted or renamed since; a sealed migration stays where it was sealed\nmigrations/tenant/0001_entity_rows.sql: not sealed, and its version is not above 0001, the highest sealed one; a new migration is numbered after every shipped one\nandromeda-sql: 2 violation(s) of the seal\n".to_owned()
        )
    );
    Ok(())
}

#[test]
fn a_new_migration_numbered_below_the_highest_sealed_one_fails_verify() -> io::Result<()> {
    let root = repository_with(&[("tenant/0002_kinds.sql", &kinds("0002"))])?;
    assert_eq!(status(&run(root.path(), &["seal", "v1.0.0"])?), (true, String::new()));
    write_migration(root.path(), ENTITIES, &entities())?;
    assert_eq!(
        verify(root.path(), &[])?,
        (
            false,
            "migrations/tenant/0001_entities.sql: not sealed, and its version is not above 0002, the highest sealed one; a new migration is numbered after every shipped one\nandromeda-sql: 1 violation(s) of the seal\n".to_owned()
        )
    );
    Ok(())
}

#[test]
fn a_hand_edited_ledger_fails_verify() -> io::Result<()> {
    let root = sealed_repository()?;
    let ledger = root.path().join(LEDGER);
    let edited = fs::read_to_string(&ledger)?.replace("set = \"tenant\"\n", "set = \"tenant\"  # sealed on release day\n");
    fs::write(&ledger, edited)?;
    assert_eq!(
        verify(root.path(), &[])?,
        (
            false,
            "migrations/seal-ledger.toml: not in the canonical form `andromeda-sql seal` writes; the ledger is never edited by hand\nandromeda-sql: 1 violation(s) of the seal\n".to_owned()
        )
    );
    Ok(())
}

#[test]
fn verify_against_a_revision_passes_when_only_a_section_was_appended() -> io::Result<()> {
    let root = sealed_repository()?;
    commit_all(root.path())?;
    write_migration(root.path(), "tenant/0002_kinds.sql", &kinds("0002"))?;
    assert_eq!(status(&run(root.path(), &["seal", "v1.1.0"])?), (true, String::new()));
    assert_eq!(verify(root.path(), &["--against", "HEAD"])?, (true, String::new()));
    Ok(())
}

#[test]
fn verify_against_a_revision_fails_when_a_sealed_migration_was_edited_and_sealed_again() -> io::Result<()> {
    let root = sealed_repository()?;
    commit_all(root.path())?;
    let edited = entities().replace("kind text", "kind_name text");
    write_migration(root.path(), ENTITIES, &edited)?;
    let ledger = root.path().join(LEDGER);
    let resealed = fs::read_to_string(&ledger)?.replace(
        &FileDigest::of(entities().as_bytes()).to_string(),
        &FileDigest::of(edited.as_bytes()).to_string(),
    );
    fs::write(&ledger, resealed)?;
    assert_eq!(verify(root.path(), &[])?, (true, String::new()));
    assert_eq!(
        verify(root.path(), &["--against", "HEAD"])?,
        (
            false,
            "migrations/seal-ledger.toml: the section of v1.0.0 differs from the one at HEAD; a sealed section never changes\nandromeda-sql: 1 violation(s) of the seal\n".to_owned()
        )
    );
    Ok(())
}

#[test]
fn verify_against_a_revision_fails_when_a_section_was_removed() -> io::Result<()> {
    let root = sealed_repository()?;
    commit_all(root.path())?;
    fs::remove_file(root.path().join(LEDGER))?;
    fs::remove_file(root.path().join("migrations").join(ENTITIES))?;
    assert_eq!(
        verify(root.path(), &["--against", "HEAD"])?,
        (
            false,
            "migrations/seal-ledger.toml: the section of v1.0.0 at HEAD is gone; a sealed section is never removed\nandromeda-sql: 1 violation(s) of the seal\n".to_owned()
        )
    );
    Ok(())
}

#[test]
fn verify_against_a_revision_without_a_ledger_holds_nothing_against_the_working_copy() -> io::Result<()> {
    let root = repository_with(&[(ENTITIES, &entities())])?;
    commit_all(root.path())?;
    assert_eq!(status(&run(root.path(), &["seal", "v1.0.0"])?), (true, String::new()));
    assert_eq!(verify(root.path(), &["--against", "HEAD"])?, (true, String::new()));
    Ok(())
}

#[test]
fn a_formatter_change_after_the_seal_leaves_the_sealed_migration_as_sealed() -> io::Result<()> {
    let root = sealed_repository()?;
    let options = root.path().join(".pg_format");
    fs::write(&options, fs::read_to_string(&options)?.replace("spaces=4", "spaces=2"))?;
    assert_eq!(status(&run(root.path(), &["format", "--check"])?), (true, String::new()));
    assert_eq!(status(&run(root.path(), &["format"])?), (true, String::new()));
    assert_eq!(fs::read_to_string(root.path().join("migrations").join(ENTITIES))?, entities());
    write_migration(root.path(), "tenant/0002_kinds.sql", &kinds("0002"))?;
    let (succeeded, stderr) = status(&run(root.path(), &["format", "--check"])?);
    assert_eq!(succeeded, false);
    assert!(stderr.starts_with("not formatted: migrations/tenant/0002_kinds.sql\n"), "{stderr}");
    Ok(())
}

#[test]
fn a_check_added_to_a_sealed_table_fails_the_lint_until_it_is_added_not_valid_and_validated_later() -> io::Result<()> {
    let root = sealed_repository()?;
    let check = format!(
        "{}alter table entities\n    add constraint chk_entities_kind_length check (octet_length(kind) <= 256);\n",
        header("0002", "Entity kind length")
    );
    write_migration(root.path(), "tenant/0002_entity_kind_length.sql", &check)?;
    let (succeeded, stderr) = status(&run(root.path(), &["lint"])?);
    assert_eq!(succeeded, false, "{stderr}");
    for expected in [
        "constraint-on-sealed-table",
        "migrations/tenant/0002_entity_kind_length.sql:7:9",
        "check chk_entities_kind_length is added to entities, which a sealed migration created, without not valid",
        "andromeda-sql: 0 error(s) and 1 warning(s) in 1 file(s)",
    ] {
        assert!(stderr.contains(expected), "{expected:?} missing from:\n{stderr}");
    }
    write_migration(
        root.path(),
        "tenant/0002_entity_kind_length.sql",
        &check.replace("<= 256);", "<= 256) not valid;"),
    )?;
    write_migration(
        root.path(),
        "tenant/0003_entity_kind_length_validation.sql",
        &format!(
            "{}alter table entities\n    validate constraint chk_entities_kind_length;\n",
            header("0003", "Entity kind length validation")
        ),
    )?;
    assert_eq!(status(&run(root.path(), &["lint"])?), (true, String::new()));
    Ok(())
}

#[test]
fn a_stable_tag_on_the_commit_that_sealed_it_passes() -> io::Result<()> {
    let root = sealed_repository()?;
    assert_eq!(verify(root.path(), &["--release", "v1.0.0"])?, (true, String::new()));
    Ok(())
}

#[test]
fn a_stable_tag_whose_tree_differs_from_the_sealed_hashes_fails() -> io::Result<()> {
    let root = sealed_repository()?;
    let edited = entities().replace("kind text", "kind_name text");
    write_migration(root.path(), ENTITIES, &edited)?;
    assert_eq!(
        verify(root.path(), &["--release", "v1.0.0"])?,
        (
            false,
            format!(
                "migrations/tenant/0001_entities.sql: sealed by v1.0.0 and edited since; a sealed migration never changes (sealed as {}, now {})\nandromeda-sql: 1 violation(s) of the seal\n",
                FileDigest::of(entities().as_bytes()),
                FileDigest::of(edited.as_bytes())
            )
        )
    );
    Ok(())
}

#[test]
fn a_stable_tag_with_no_section_fails() -> io::Result<()> {
    let root = repository_with(&[(ENTITIES, &entities())])?;
    assert_eq!(
        verify(root.path(), &["--release", "v1.0.0"])?,
        (
            false,
            "migrations/seal-ledger.toml: v1.0.0 has no section; a stable release is sealed before it is tagged\nmigrations/tenant/0001_entities.sql: not sealed, and v1.0.0 is tagged; every migration a stable release ships is sealed\nandromeda-sql: 2 violation(s) of the seal\n".to_owned()
        )
    );
    Ok(())
}

#[test]
fn a_stable_tag_holding_a_migration_added_after_its_seal_fails() -> io::Result<()> {
    let root = sealed_repository()?;
    write_migration(root.path(), "tenant/0002_kinds.sql", &kinds("0002"))?;
    assert_eq!(
        verify(root.path(), &["--release", "v1.0.0"])?,
        (
            false,
            "migrations/tenant/0002_kinds.sql: not sealed, and v1.0.0 is tagged; every migration a stable release ships is sealed\nandromeda-sql: 1 violation(s) of the seal\n".to_owned()
        )
    );
    Ok(())
}

#[test]
fn a_stable_tag_behind_a_later_sealed_release_fails() -> io::Result<()> {
    let root = sealed_repository()?;
    write_migration(root.path(), "tenant/0002_kinds.sql", &kinds("0002"))?;
    assert_eq!(status(&run(root.path(), &["seal", "v1.1.0"])?), (true, String::new()));
    assert_eq!(
        verify(root.path(), &["--release", "v1.0.0"])?,
        (
            false,
            "migrations/seal-ledger.toml: v1.1.0 is sealed after v1.0.0; a stable release is tagged on the commit where it is the last release sealed\nandromeda-sql: 1 violation(s) of the seal\n".to_owned()
        )
    );
    Ok(())
}

#[test]
fn an_rc_tag_with_no_section_passes() -> io::Result<()> {
    let root = repository_with(&[(ENTITIES, &entities())])?;
    assert_eq!(verify(root.path(), &["--release", "v1.0.0-rc1"])?, (true, String::new()));
    Ok(())
}

#[test]
fn a_stable_tag_passes_against_the_previous_release_tag_when_it_only_appended_a_section() -> io::Result<()> {
    let root = sealed_repository()?;
    commit_all(root.path())?;
    let tagged = git(root.path(), &["tag", "v1.0.0"])?;
    assert!(tagged.status.success(), "{}", String::from_utf8_lossy(&tagged.stderr));
    write_migration(root.path(), "tenant/0002_kinds.sql", &kinds("0002"))?;
    assert_eq!(status(&run(root.path(), &["seal", "v1.1.0"])?), (true, String::new()));
    assert_eq!(verify(root.path(), &["--release", "v1.1.0", "--against", "v1.0.0"])?, (true, String::new()));
    Ok(())
}
