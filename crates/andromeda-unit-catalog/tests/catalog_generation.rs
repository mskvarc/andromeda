//! Generation and stability of the committed unit-code catalog.
//!
//! One artifact, one generator: the identity snapshot the crate compiles in and
//! the seed migration the database loads are rendered from the same assignment,
//! so the two can never disagree about which number names which code. The tests
//! compare the committed files against a fresh rendering and fail when they
//! differ; setting `ANDROMEDA_UNIT_CATALOG_BLESS` rewrites them instead, which
//! is the only way either file is ever edited.

use andromeda_unit_catalog::{
    rec20_revision::{REC20_REVISION, Rec20Revision},
    source_digest::SOURCE_SHA256,
    unit_code_entry::UnitCodeEntry,
    unit_code_id::UnitCodeId,
    unit_code_registry::{RETIRED_REVISION_UNSET, UnitCodeRegistry, UnitCodeSnapshotMalformed, registry},
    unit_code_token::UnitCodeToken,
};
use cefact_units::UnitOfMeasure;
use pretty_assertions::assert_eq;
use std::{
    collections::HashSet,
    env,
    fmt::{self, Write},
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

/// Environment variable that rewrites the committed files instead of checking them.
const BLESS: &str = "ANDROMEDA_UNIT_CATALOG_BLESS";

/// Instant recorded as the catalog release's generation time.
///
/// A fixed value rather than the wall clock: the seed migration is a committed
/// artifact, and a timestamp that moved on every rendering would make it
/// impossible to check that a regeneration changed nothing else.
const GENERATED_AT: &str = "2026-08-11T00:00:00Z";

/// Header of the committed identity snapshot.
const SNAPSHOT_HEADER: &str = "# generated - never hand-edited\n\
# UN/CEFACT Recommendation 20 unit-code identity registry.\n\
# Append-only: an identity assigned here is never reassigned and never removed.\n\
# Columns: identity, common code, first revision, retiring revision ('-' while current).\n\
# Regenerate with: ANDROMEDA_UNIT_CATALOG_BLESS=1 cargo test -p andromeda-unit-catalog\n";

/// Path of the committed identity snapshot, relative to the crate root.
const SNAPSHOT_PATH: &str = "data/unit-code-ids.tsv";

/// The three migration directories, relative to the crate root. The seed series
/// shares one global version space with every other migration, so a new seed
/// takes a number none of these already holds.
const MIGRATION_DIRECTORIES: [&str; 3] = ["../../migrations/platform", "../../migrations/tenant", "../../migrations/branch"];

/// The directory a newly minted seed migration is written into.
const SEED_DIRECTORY: &str = "../../migrations/tenant";

/// The filename stem, without its version prefix, of the full catalog seed. The
/// first release renders the whole assignment into one such file; a later
/// revision never rewrites it but appends its own update migration.
const FULL_SEED_STEM: &str = "unit_codes_seed";

/// The identity assignment could not be produced or does not match what is committed.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
enum CatalogGenerationFailure {
    /// The committed snapshot does not parse.
    #[error("the committed identity snapshot is unusable: {0}")]
    Snapshot(#[from] UnitCodeSnapshotMalformed),
    /// A code the source artifact defines is not a well-formed common code.
    #[error("the source artifact defines {0}, which is not a well-formed common code")]
    Token(String),
    /// Every two-byte identity has been assigned.
    #[error("the catalog has exhausted the identities a two-byte key can hold")]
    IdentitySpaceExhausted,
    /// A committed file could not be read or written.
    #[error("{path} could not be read or written")]
    Unreadable {
        /// The file that could not be reached.
        path: String,
    },
    /// A committed file differs from a fresh rendering.
    #[error("{path} is stale; regenerate it with {BLESS}=1 cargo test -p andromeda-unit-catalog")]
    Stale {
        /// The file that no longer matches the generator.
        path: String,
    },
    /// A rendering could not be assembled in its buffer.
    #[error("a catalog rendering could not be assembled")]
    Rendering(#[from] fmt::Error),
}

/// Return the crate-root-relative `path` as an absolute one.
fn crate_path(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

/// Return the identity assignment: every committed registration unchanged,
/// followed by each code the source artifact adds, in ascending code order.
///
/// Sorting by code rather than by the artifact's own row order is what makes a
/// regeneration reproducible: the artifact lists Annex I before Annex II, and a
/// reshuffle there must not be able to move an identity.
fn assignment() -> Result<Vec<UnitCodeEntry>, CatalogGenerationFailure> {
    let mut codes: Vec<&'static str> = UnitOfMeasure::ALL.iter().map(|unit| unit.code()).collect();
    codes.sort_unstable();
    let codes = codes;
    assignment_over(registry()?, &codes)
}

/// Return the assignment a release generates from `catalog` and the codes a
/// source revision defines.
///
/// Taken as arguments rather than read from the committed files so the same
/// rule that produces a release can be applied to a fixture catalog and a
/// fixture revision, which is how the upgrade behaviour is checked without
/// waiting for Recommendation 20 to publish one.
fn assignment_over(catalog: &UnitCodeRegistry, codes: &[&'static str]) -> Result<Vec<UnitCodeEntry>, CatalogGenerationFailure> {
    let mut entries: Vec<UnitCodeEntry> = catalog.entries().to_vec();
    let mut next = match entries.iter().map(UnitCodeEntry::id).max() {
        Some(highest) => highest.next().ok_or(CatalogGenerationFailure::IdentitySpaceExhausted)?,
        None => UnitCodeId::first(),
    };

    for code in codes.iter().copied() {
        let token = UnitCodeToken::try_from(code).map_err(|_| CatalogGenerationFailure::Token(code.to_owned()))?;
        if catalog.id_of_token(token).is_ok() {
            continue;
        }
        entries.push(UnitCodeEntry::new(next, token, REC20_REVISION, None));
        next = next.next().ok_or(CatalogGenerationFailure::IdentitySpaceExhausted)?;
    }

    Ok(entries)
}

/// Render the committed identity snapshot.
fn render_snapshot(entries: &[UnitCodeEntry]) -> Result<String, CatalogGenerationFailure> {
    let mut rendered = String::from(SNAPSHOT_HEADER);
    for entry in entries {
        let retired = entry
            .retired_revision()
            .map_or_else(|| RETIRED_REVISION_UNSET.to_owned(), |revision| revision.get().to_string());
        writeln!(&mut rendered, "{}\t{}\t{}\t{}", entry.id(), entry.token(), entry.first_revision(), retired)?;
    }
    Ok(rendered)
}

/// Render the full seed migration that loads the whole assignment into a tenant
/// schema, in the canonical form the repository's `pg_format` configuration
/// produces, so a format run over the committed file changes nothing.
///
/// The rows are carried as four parallel arrays through `unnest` rather than a
/// row-per-line `VALUES` list: `pg_format` keeps the arrays on one line, so the
/// file stays a fixed point at a few dozen lines instead of one that the
/// formatter would explode to thousands.
fn render_full_seed(entries: &[UnitCodeEntry], version: i64) -> Result<String, CatalogGenerationFailure> {
    let ids = numeric_array(entries.iter().map(|entry| i64::from(entry.id().get())));
    let codes = code_array(entries.iter());
    let firsts = numeric_array(entries.iter().map(|entry| i64::from(entry.first_revision().get())));
    let retireds = entries
        .iter()
        .map(|entry| {
            entry
                .retired_revision()
                .map_or_else(|| String::from("null"), |revision| revision.get().to_string())
        })
        .collect::<Vec<_>>()
        .join(", ");

    let mut rendered = String::new();
    rendered.push_str("-- ============================================================================\n");
    writeln!(&mut rendered, "-- {version:04}: Unit-code catalog seed")?;
    rendered.push_str("-- ============================================================================\n");
    rendered.push_str("-- Generated from the unit-code assignment every broker carries; never edited\n");
    rendered.push_str("-- by hand.\n");
    rendered.push_str("--\n");
    rendered.push_str("-- The assignment is append-only: an identity is never reassigned and a row\n");
    rendered.push_str("-- is never deleted, because stored attributes point at these numbers.\n");
    rendered.push_str("-- ============================================================================\n");
    rendered.push_str("insert into unit_codes (\n    id,\n    code,\n    first_revision,\n    retired_revision)\n");
    rendered.push_str("select\n    *\nfrom\n");
    writeln!(
        &mut rendered,
        "    unnest(array[{ids}]::smallint[], array[{codes}]::text[], array[{firsts}]::smallint[], array[{retireds}]::smallint[]);"
    )?;
    rendered.push('\n');
    rendered.push_str("insert into unit_code_catalog_metadata (\n    singleton,\n    rec20_revision,\n    source_sha256,\n    generated_at)\n");
    rendered.push_str("values (\n    true,\n");
    writeln!(&mut rendered, "    {REC20_REVISION},")?;
    writeln!(&mut rendered, "    '{SOURCE_SHA256}',")?;
    writeln!(&mut rendered, "    '{GENERATED_AT}');")?;
    rendered.push('\n');
    Ok(rendered)
}

/// A `pg_format`-canonical integer array literal body: the values joined by a
/// comma and a space, as they appear inside `array[...]`.
fn numeric_array(values: impl Iterator<Item = i64>) -> String {
    values.map(|value| value.to_string()).collect::<Vec<_>>().join(", ")
}

/// The code array literal body for `entries`: each token single-quoted, joined
/// by a comma and a space.
fn code_array<'a>(entries: impl Iterator<Item = &'a UnitCodeEntry>) -> String {
    entries.map(|entry| format!("'{}'", entry.token())).collect::<Vec<_>>().join(", ")
}

/// Render the seed migration that upgrades an already-seeded catalog to
/// `revision`: it appends the codes that revision introduces and marks the codes
/// it withdraws, and never rewrites an earlier seed. The output is in the
/// repository's canonical `pg_format` form, so a format run leaves it unchanged.
fn render_update_seed(version: i64, revision: i16, added: &[UnitCodeEntry], withdrawn: &[UnitCodeEntry]) -> Result<String, CatalogGenerationFailure> {
    let mut rendered = String::new();
    rendered.push_str("-- ============================================================================\n");
    writeln!(&mut rendered, "-- {version:04}: Unit-code catalog update to revision {revision}")?;
    rendered.push_str("-- ============================================================================\n");
    rendered.push_str("-- Generated from the unit-code assignment every broker carries; never edited\n");
    rendered.push_str("-- by hand.\n");
    rendered.push_str("--\n");
    rendered.push_str("-- Appends the codes a newer Recommendation 20 revision introduces and marks the\n");
    rendered.push_str("-- codes it withdraws, without rewriting an earlier seed.\n");
    rendered.push_str("-- ============================================================================\n");

    if !added.is_empty() {
        let ids = numeric_array(added.iter().map(|entry| i64::from(entry.id().get())));
        let codes = code_array(added.iter());
        let firsts = numeric_array(added.iter().map(|entry| i64::from(entry.first_revision().get())));
        let retireds = added
            .iter()
            .map(|entry| {
                entry
                    .retired_revision()
                    .map_or_else(|| String::from("null"), |revision| revision.get().to_string())
            })
            .collect::<Vec<_>>()
            .join(", ");
        rendered.push_str("insert into unit_codes (\n    id,\n    code,\n    first_revision,\n    retired_revision)\n");
        rendered.push_str("select\n    *\nfrom\n");
        writeln!(
            &mut rendered,
            "    unnest(array[{ids}]::smallint[], array[{codes}]::text[], array[{firsts}]::smallint[], array[{retireds}]::smallint[])"
        )?;
        rendered.push_str("on conflict (id)\n    do nothing;\n\n");
    }

    if !withdrawn.is_empty() {
        let codes = code_array(withdrawn.iter());
        rendered.push_str("update\n    unit_codes\nset\n");
        writeln!(&mut rendered, "    retired_revision = {revision}")?;
        rendered.push_str("where\n");
        writeln!(&mut rendered, "    code = any (array[{codes}]::text[])")?;
        rendered.push_str("    and retired_revision is null;\n\n");
    }

    rendered.push_str("update\n    unit_code_catalog_metadata\nset\n");
    writeln!(&mut rendered, "    rec20_revision = {revision},")?;
    writeln!(&mut rendered, "    source_sha256 = '{SOURCE_SHA256}',")?;
    writeln!(&mut rendered, "    generated_at = '{GENERATED_AT}'")?;
    rendered.push_str("where\n    singleton;\n\n");
    Ok(rendered)
}

/// Compare `expected` against the committed file at `path`, or rewrite it when
/// the bless variable is set.
fn ensure_file_matches(path: &Path, expected: &str) -> Result<(), CatalogGenerationFailure> {
    let shown = path.display().to_string();
    if env::var_os(BLESS).is_some() {
        return fs::write(path, expected).map_err(|_| CatalogGenerationFailure::Unreadable { path: shown });
    }
    let committed = fs::read_to_string(path).map_err(|_| CatalogGenerationFailure::Unreadable { path: shown.clone() })?;
    if committed == expected {
        Ok(())
    } else {
        Err(CatalogGenerationFailure::Stale { path: shown })
    }
}

/// Every migration version number committed across the three directories.
fn committed_migration_numbers() -> Result<Vec<i64>, CatalogGenerationFailure> {
    let mut numbers = Vec::new();
    for directory in MIGRATION_DIRECTORIES {
        let entries = fs::read_dir(crate_path(directory)).map_err(|_| CatalogGenerationFailure::Unreadable { path: directory.to_owned() })?;
        for entry in entries {
            let entry = entry.map_err(|_| CatalogGenerationFailure::Unreadable { path: directory.to_owned() })?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if let Some(number) = name
                .strip_suffix(".sql")
                .and_then(|stem| stem.split_once('_'))
                .and_then(|(prefix, _)| prefix.parse::<i64>().ok())
            {
                numbers.push(number);
            }
        }
    }
    numbers.sort_unstable();
    Ok(numbers)
}

/// The lowest version number no committed migration uses.
fn next_free_migration_number() -> Result<i64, CatalogGenerationFailure> {
    Ok(committed_migration_numbers()?.into_iter().max().map_or(1, |highest| highest + 1))
}

/// The committed full seed migration, searched across the three directories.
fn full_seed_path() -> Result<Option<PathBuf>, CatalogGenerationFailure> {
    let suffix = format!("_{FULL_SEED_STEM}.sql");
    for directory in MIGRATION_DIRECTORIES {
        let entries = fs::read_dir(crate_path(directory)).map_err(|_| CatalogGenerationFailure::Unreadable { path: directory.to_owned() })?;
        for entry in entries {
            let entry = entry.map_err(|_| CatalogGenerationFailure::Unreadable { path: directory.to_owned() })?;
            if entry.file_name().to_str().is_some_and(|name| name.ends_with(suffix.as_str())) {
                return Ok(Some(entry.path()));
            }
        }
    }
    Ok(None)
}

/// The version number in a migration filename of the `NNNN_name.sql` shape.
fn version_of(path: &Path) -> Result<i64, CatalogGenerationFailure> {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.split_once('_'))
        .and_then(|(prefix, _)| prefix.parse::<i64>().ok())
        .ok_or_else(|| CatalogGenerationFailure::Unreadable {
            path: path.display().to_string(),
        })
}

#[test]
fn the_committed_identity_snapshot_matches_a_fresh_rendering() -> Result<(), CatalogGenerationFailure> {
    ensure_file_matches(&crate_path(SNAPSHOT_PATH), &render_snapshot(&assignment()?)?)
}

#[test]
fn the_committed_full_seed_matches_a_fresh_rendering() -> Result<(), CatalogGenerationFailure> {
    let entries = assignment()?;
    match full_seed_path()? {
        Some(path) => {
            let version = version_of(&path)?;
            ensure_file_matches(&path, &render_full_seed(&entries, version)?)
        }
        None if env::var_os(BLESS).is_some() => {
            let version = next_free_migration_number()?;
            let path = crate_path(SEED_DIRECTORY).join(format!("{version:04}_{FULL_SEED_STEM}.sql"));
            fs::write(&path, render_full_seed(&entries, version)?).map_err(|_| CatalogGenerationFailure::Unreadable {
                path: path.display().to_string(),
            })
        }
        None => Err(CatalogGenerationFailure::Stale {
            path: FULL_SEED_STEM.to_owned(),
        }),
    }
}

#[test]
fn a_seed_migration_takes_a_version_no_other_migration_holds() -> Result<(), CatalogGenerationFailure> {
    let numbers = committed_migration_numbers()?;
    let next = next_free_migration_number()?;
    assert!(!numbers.contains(&next), "the next free number {next} is already taken");
    assert!(
        numbers.iter().all(|&number| number < next),
        "a committed migration is numbered at or beyond the next free number {next}"
    );
    Ok(())
}

/// Build one registration from its parts, for the update-rendering fixtures.
fn fixture_entry(id: i16, code: &'static str, first: i16, retired: Option<i16>) -> Result<UnitCodeEntry, CatalogGenerationFailure> {
    let token = UnitCodeToken::try_from(code).map_err(|_| CatalogGenerationFailure::Token(code.to_owned()))?;
    let first = Rec20Revision::try_from(first).map_err(|_| CatalogGenerationFailure::Token(code.to_owned()))?;
    let retired = retired
        .map(Rec20Revision::try_from)
        .transpose()
        .map_err(|_| CatalogGenerationFailure::Token(code.to_owned()))?;
    let id = UnitCodeId::try_from(id).map_err(|_| CatalogGenerationFailure::Token(code.to_owned()))?;
    Ok(UnitCodeEntry::new(id, token, first, retired))
}

#[test]
fn a_revision_bump_emits_an_update_migration_that_does_not_rewrite_the_full_seed() -> Result<(), CatalogGenerationFailure> {
    // Revision 18 introduces one code and withdraws one the catalog already
    // carries. The update migration that models the bump appends the new code
    // and marks the withdrawn one; it is a wholly separate file that never
    // restates the full seed.
    let added = [fixture_entry(2000, "QQ", 18, None)?];
    let withdrawn = [fixture_entry(3, "ZZ", 9, Some(18))?];
    let rendered = render_update_seed(32, 18, &added, &withdrawn)?;

    assert!(rendered.contains("-- 0032: Unit-code catalog update to revision 18"), "{rendered}");
    assert!(rendered.contains("insert into unit_codes (\n    id,\n    code,"), "{rendered}");
    assert!(rendered.contains("array['QQ']::text[]"), "{rendered}");
    assert!(rendered.contains("on conflict (id)\n    do nothing;"), "{rendered}");
    assert!(rendered.contains("    retired_revision = 18\n"), "{rendered}");
    assert!(rendered.contains("code = any (array['ZZ']::text[])"), "{rendered}");
    assert!(rendered.contains("rec20_revision = 18,"), "{rendered}");
    assert!(
        !rendered.contains("Unit-code catalog seed"),
        "an update migration must not restate the full seed: {rendered}"
    );
    Ok(())
}

#[test]
fn an_update_migration_with_only_new_codes_omits_the_withdrawal_statement() -> Result<(), CatalogGenerationFailure> {
    let added = [fixture_entry(2000, "QQ", 18, None)?];
    let rendered = render_update_seed(32, 18, &added, &[])?;
    assert!(rendered.contains("on conflict (id)\n    do nothing;"), "{rendered}");
    assert!(
        !rendered.contains("retired_revision = 18"),
        "no code is withdrawn, so no update statement: {rendered}"
    );
    assert!(rendered.contains("rec20_revision = 18,"), "{rendered}");
    Ok(())
}

#[test]
fn every_assigned_identity_is_positive_and_fits_a_two_byte_key() -> Result<(), CatalogGenerationFailure> {
    for entry in assignment()? {
        assert!(entry.id().get() > 0, "{} was assigned a non-positive identity", entry.token());
        assert!(
            i32::from(entry.id().get()) < 32_768,
            "{} was assigned an identity beyond the two-byte ceiling",
            entry.token()
        );
    }
    Ok(())
}

#[test]
fn no_identity_and_no_common_code_is_assigned_twice() -> Result<(), CatalogGenerationFailure> {
    let entries = assignment()?;
    let identities: HashSet<UnitCodeId> = entries.iter().map(UnitCodeEntry::id).collect();
    let tokens: HashSet<&str> = entries.iter().map(|entry| entry.token().as_str()).collect();
    assert_eq!(identities.len(), entries.len(), "an identity was assigned to more than one common code");
    assert_eq!(tokens.len(), entries.len(), "a common code was assigned more than one identity");
    Ok(())
}

#[test]
fn regeneration_leaves_every_committed_identity_where_it_was() -> Result<(), CatalogGenerationFailure> {
    let committed = registry()?;
    for entry in assignment()? {
        if let Ok(existing) = committed.id_of_token(entry.token()) {
            assert_eq!(existing, entry.id(), "regeneration moved {} to a different identity", entry.token());
        }
    }
    assert!(
        assignment()?.len() >= committed.entries().len(),
        "regeneration dropped registrations the committed snapshot holds"
    );
    Ok(())
}

#[test]
fn every_code_the_source_artifact_defines_is_registered() -> Result<(), CatalogGenerationFailure> {
    let catalog = registry()?;
    for unit in UnitOfMeasure::ALL {
        let registered = catalog.id_of(unit.unit_code());
        assert!(registered.is_ok(), "{} is defined by the source artifact but holds no identity", unit.code());
    }
    Ok(())
}

/// Identities this release published for codes an NGSI-LD payload commonly carries.
///
/// Spot checks rather than a digest of the whole file: a digest changes on
/// every legitimate append and so says nothing, while a handful of fixed pairs
/// fails loudly the moment a regeneration renumbers the catalogue instead of
/// extending it.
const PUBLISHED_IDENTITIES: &[(i16, &str)] = &[(367, "CEL"), (1121, "KGM"), (1407, "MTR"), (1695, "SEC"), (1756, "ZZ")];

#[test]
fn well_known_codes_keep_the_identities_this_release_published() -> Result<(), CatalogGenerationFailure> {
    let catalog = registry()?;
    for (published, code) in PUBLISHED_IDENTITIES {
        let token = UnitCodeToken::try_from(*code).map_err(|_| CatalogGenerationFailure::Token((*code).to_owned()))?;
        let assigned = catalog.id_of_token(token).ok().map(UnitCodeId::get);
        assert_eq!(assigned, Some(*published), "{code} no longer carries the identity this release published");
    }
    Ok(())
}

#[test]
fn a_common_code_the_catalog_never_registered_fails_to_encode() -> Result<(), CatalogGenerationFailure> {
    let catalog = registry()?;
    let unregistered = UnitCodeToken::try_from("Q99").map_err(|_| CatalogGenerationFailure::Token(String::from("Q99")))?;
    assert!(
        catalog.id_of_token(unregistered).is_err(),
        "Q99 is not a Recommendation 20 common code and must not decode"
    );
    Ok(())
}

/// Unit catalog completeness in the reverse direction: every identity decodes
/// back to its own code.
///
/// Encoding is checked by the generation tests above; decoding is what a read
/// path does, once per unit-bearing attribute, and it has to land back on the
/// code the identity was assigned for.
#[test]
fn every_identity_decodes_back_to_the_code_it_was_assigned_for() -> Result<(), CatalogGenerationFailure> {
    let catalog = registry()?;
    for entry in catalog.entries() {
        let decoded = catalog.entry(entry.id()).map(UnitCodeEntry::token);
        assert_eq!(
            decoded.map(UnitCodeToken::as_str),
            Ok(entry.token().as_str()),
            "identity {} does not decode back to its own code",
            entry.id()
        );
        assert_eq!(
            catalog.id_of_token(entry.token()),
            Ok(entry.id()),
            "{} does not encode back to its own identity",
            entry.token()
        );
    }
    Ok(())
}

/// Unit ID stability: upgrading to a newer revision keeps every identity.
///
/// A newer source revision is modelled as a fixture catalog upgraded against a
/// code list that keeps two codes, retires one and adds one: the surviving
/// codes must keep their identities, the withdrawn one must stay decodable, and
/// the new one must take an identity nothing has ever used.
#[test]
fn upgrading_a_catalog_to_a_newer_revision_keeps_every_identity_and_appends_the_new_ones() -> Result<(), CatalogGenerationFailure> {
    // Identity 3 was withdrawn by revision 17 while identities 1 and 2 remain
    // current; the upgrade offers a code the fixture has never registered.
    let released = UnitCodeRegistry::parse("1\tKGM\t9\t-\n2\tMTR\t9\t-\n3\tZZZ\t9\t17\n")?;
    let upgraded = assignment_over(&released, &["KGM", "MTR", "SEC"])?;

    let identity_of = |entries: &[UnitCodeEntry], code: &str| entries.iter().find(|entry| entry.token().as_str() == code).map(UnitCodeEntry::id);

    assert_eq!(
        identity_of(&upgraded, "KGM"),
        UnitCodeId::try_from(1_i16).ok(),
        "an existing code must keep its identity"
    );
    assert_eq!(
        identity_of(&upgraded, "MTR"),
        UnitCodeId::try_from(2_i16).ok(),
        "an existing code must keep its identity"
    );
    assert_eq!(
        identity_of(&upgraded, "ZZZ"),
        UnitCodeId::try_from(3_i16).ok(),
        "a withdrawn code keeps its registration so stored rows stay readable"
    );
    assert_eq!(
        identity_of(&upgraded, "SEC"),
        UnitCodeId::try_from(4_i16).ok(),
        "a new code takes the next identity nothing has used"
    );

    // The withdrawn code is still resolvable by token -- it is only no longer
    // offered -- which is what keeps an attribute written under it decodable.
    let withdrawn = UnitCodeToken::try_from("ZZZ").map_err(|_| CatalogGenerationFailure::Token(String::from("ZZZ")))?;
    assert_eq!(released.id_of_token(withdrawn).ok(), UnitCodeId::try_from(3_i16).ok());
    assert_eq!(
        upgraded.iter().filter(|entry| entry.token().as_str() == "ZZZ").count(),
        1,
        "the upgrade must neither drop nor duplicate a withdrawn registration"
    );

    Ok(())
}
