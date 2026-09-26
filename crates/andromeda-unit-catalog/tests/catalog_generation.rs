//! Generation and stability of the committed unit-code catalog.
//!
//! One artifact, one generator: the identity snapshot the crate compiles in and
//! the seed migration the database loads are rendered from the same assignment,
//! so the two can never disagree about which number names which code. The tests
//! compare the committed files against a fresh rendering and fail when they
//! differ; setting `ANDROMEDA_UNIT_CATALOG_BLESS` rewrites them instead, which
//! is the only way either file is ever edited.

use andromeda_unit_catalog::{
    rec20_revision::REC20_REVISION,
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

/// Path of the generated seed migration, relative to the crate root.
const SEED_PATH: &str = "../../migrations/tenant/0010_unit_codes_seed.sql";

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

/// Render the seed migration that loads the catalog into a tenant schema.
fn render_seed(entries: &[UnitCodeEntry]) -> Result<String, CatalogGenerationFailure> {
    let mut rendered = String::new();
    rendered.push_str("-- ============================================================================\n");
    rendered.push_str("-- 0010: Unit-code catalog seed\n");
    rendered.push_str("-- ============================================================================\n");
    rendered.push_str("-- Generated from the unit-code assignment every broker carries; never edited\n");
    rendered.push_str("-- by hand.\n");
    rendered.push_str("--\n");
    rendered.push_str("-- The assignment is append-only: an identity is never reassigned and a row\n");
    rendered.push_str("-- is never deleted, because stored attributes point at these numbers.\n");
    rendered.push_str("-- ============================================================================\n\n");
    rendered.push_str("INSERT INTO unit_codes (id, code, first_revision, retired_revision) VALUES\n");

    let mut rows = entries.iter().peekable();
    while let Some(entry) = rows.next() {
        let retired = entry
            .retired_revision()
            .map_or_else(|| String::from("NULL"), |revision| revision.get().to_string());
        let terminator = if rows.peek().is_some() { "," } else { ";" };
        writeln!(
            &mut rendered,
            "    ({}, '{}', {}, {}){}",
            entry.id(),
            entry.token(),
            entry.first_revision(),
            retired,
            terminator
        )?;
    }

    rendered.push_str("\nINSERT INTO unit_code_catalog_metadata (singleton, rec20_revision, source_sha256, generated_at) VALUES\n");
    writeln!(&mut rendered, "    (TRUE, {REC20_REVISION}, '{SOURCE_SHA256}', '{GENERATED_AT}');")?;
    Ok(rendered)
}

/// Compare `expected` against the committed file at `path`, or rewrite it when
/// the bless variable is set.
fn ensure_generated(path: &str, expected: &str) -> Result<(), CatalogGenerationFailure> {
    let absolute = crate_path(path);
    if env::var_os(BLESS).is_some() {
        return fs::write(&absolute, expected).map_err(|_| CatalogGenerationFailure::Unreadable { path: path.to_owned() });
    }
    let committed = fs::read_to_string(&absolute).map_err(|_| CatalogGenerationFailure::Unreadable { path: path.to_owned() })?;
    if committed == expected {
        Ok(())
    } else {
        Err(CatalogGenerationFailure::Stale { path: path.to_owned() })
    }
}

#[test]
fn the_committed_identity_snapshot_matches_a_fresh_rendering() -> Result<(), CatalogGenerationFailure> {
    ensure_generated(SNAPSHOT_PATH, &render_snapshot(&assignment()?)?)
}

#[test]
fn the_generated_seed_migration_matches_a_fresh_rendering() -> Result<(), CatalogGenerationFailure> {
    ensure_generated(SEED_PATH, &render_seed(&assignment()?)?)
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

/// Verification matrix row "Unit catalog completeness": the reverse direction.
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

/// Verification matrix row "Unit ID stability".
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
