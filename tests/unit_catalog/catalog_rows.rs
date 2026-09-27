//! Unit catalog completeness: compiled and seeded catalogues agree entirely.
//!
//! The compiled-in assignment and the seeded `unit_codes` rows are rendered
//! from one generator, so a disagreement between them would mean an identity
//! written by the broker names a different code in the database it was written
//! to. These tests iterate the whole revision-pinned profile rather than
//! sampling it: every code maps to exactly one row, every row maps back to the
//! same typed code, and the catalogue's count, revision and source checksum are
//! the constants the release published.

use crate::fixtures::SUITE;
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_unit_catalog::{
    rec20_revision::{REC20_REVISION, Rec20Revision},
    source_digest::SOURCE_SHA256,
    unit_code_entry::UnitCodeEntry,
    unit_code_id::UnitCodeId,
    unit_code_registry::registry,
};
use pretty_assertions::assert_eq;
use sqlx::Row;

#[tokio::test]
async fn every_code_in_the_pinned_profile_maps_to_exactly_one_catalog_row_and_back() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let catalog = registry().expect("the compiled-in unit catalog parses");

    let rows = sqlx::query("SELECT id, code, first_revision, retired_revision FROM unit_codes ORDER BY id")
        .fetch_all(scope.database_pool())
        .await
        .expect("the seeded unit catalogue must be readable");

    assert_eq!(
        rows.len(),
        catalog.entries().len(),
        "the seeded catalogue and the compiled-in assignment must hold the same registrations"
    );

    for row in &rows {
        let stored: i16 = row.get("id");
        let code: String = row.get("code");
        let first_revision: i16 = row.get("first_revision");
        let retired_revision: Option<i16> = row.get("retired_revision");

        let identity = UnitCodeId::try_from(stored).expect("a seeded identity is positive");
        let entry = catalog.entry(identity).expect("every seeded identity is registered in the compiled catalog");

        assert_eq!(entry.token().as_str(), code, "identity {stored} names a different code in the database");
        assert_eq!(
            entry.first_revision().get(),
            first_revision,
            "identity {stored} was seeded under another revision"
        );
        assert_eq!(
            entry.retired_revision().map(Rec20Revision::get),
            retired_revision,
            "identity {stored} disagrees with the database about its withdrawal"
        );
        // The reverse direction is what a write path exercises: the code a
        // client sent has to resolve back to the identity the row carries.
        assert_eq!(
            catalog.id_of_token(entry.token()),
            Ok(identity),
            "{code} does not encode back to its own identity"
        );
    }

    // Every compiled registration is covered by the loop above only if the two
    // sets have the same identities, which the equal counts plus the successful
    // per-row lookups establish; the reverse containment is asserted here so a
    // registration the seed forgot cannot hide behind a compensating extra row.
    for entry in catalog.entries() {
        let seeded: Option<i16> = sqlx::query_scalar("SELECT id FROM unit_codes WHERE code = $1")
            .bind(entry.token().as_str())
            .fetch_optional(scope.database_pool())
            .await
            .expect("the seeded unit catalogue must be readable");
        assert_eq!(
            seeded,
            Some(UnitCodeEntry::id(entry).get()),
            "{} is registered in the binary but not seeded under the same identity",
            entry.token()
        );
    }

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn the_seeded_catalog_metadata_names_the_revision_and_source_the_release_published() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");

    let row = sqlx::query("SELECT rec20_revision, source_sha256 FROM unit_code_catalog_metadata WHERE singleton")
        .fetch_one(scope.database_pool())
        .await
        .expect("the unit catalogue metadata singleton must exist");

    let revision: i16 = row.get("rec20_revision");
    let digest: String = row.get("source_sha256");
    assert_eq!(
        revision,
        REC20_REVISION.get(),
        "the seeded revision is not the one the binary was generated from"
    );
    assert_eq!(
        digest,
        SOURCE_SHA256.to_hex(),
        "the seeded catalogue was rendered from a different source artefact"
    );

    scope.finish().await.expect("cleanup failed");
}
