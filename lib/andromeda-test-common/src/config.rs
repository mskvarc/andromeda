//! Compile-time configuration constants shared across integration tests.
//!
//! These values mirror the existing per-suite constants so migrating a test
//! from the legacy inline helpers to `andromeda_test_common` is a pure
//! cross-reference change.

/// Base URL of the Andromeda broker under test.
///
/// Tests do not start the broker; it must be listening before `cargo test`
/// is invoked (existing project convention).
pub const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";

/// Default tenant database connection string.
///
/// All tests execute against this database. Per-test isolation is achieved
/// through id-prefix scoping inside [`crate::scope::TestScope`], not through
/// separate databases — provisioning a fresh tenant database would require
/// running the full migration chain per test and is prohibitively slow.
pub const DATABASE_URL: &str = "postgres://postgres:postgres@127.0.0.1:5432/postgres";

/// NGSI-LD v1.9 Core `@context` URL.
pub const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// ETSI NGSI-LD Test Suite `@context` URL (base variant).
pub const TEST_SUITE_CONTEXT: &str = "https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite.jsonld";

/// ETSI NGSI-LD Test Suite compound `@context` URL used by the Building fixtures.
pub const COMPOUND_CONTEXT: &str =
    "https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite-compound.jsonld";
