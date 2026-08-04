//! Compile-time configuration constants shared across integration tests.
//!
//! These values mirror the existing per-suite constants so migrating a test
//! from the legacy inline helpers to `andromeda_test_common` is a pure
//! cross-reference change.

use std::{env, sync::LazyLock};

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

/// Environment variable overriding the host the broker dials back on.
pub const CALLBACK_HOST_ENV: &str = "ANDROMEDA_TEST_CALLBACK_HOST";

/// Host used for broker callbacks when [`CALLBACK_HOST_ENV`] is unset.
const DEFAULT_CALLBACK_HOST: &str = "host.docker.internal";

/// Host name the broker must use to reach a mock bound in the test process.
///
/// The mocks — notification receivers, Context Source stubs, `@context`
/// servers — bind an ephemeral port on the machine running `cargo test`, and
/// the broker then dials that port by name. Which name works depends on where
/// the broker runs. Inside the Docker stack the test host is
/// `host.docker.internal`; a broker started directly on the host cannot
/// resolve that name at all, so every callback it is handed is unreachable and
/// the notification, Context Source and cached-`@context` suites fail for
/// reasons that have nothing to do with the broker's behaviour.
///
/// The Docker name stays the default because that is the usual layout.
/// Point it at loopback for a host-run broker:
///
/// ```sh
/// ANDROMEDA_TEST_CALLBACK_HOST=127.0.0.1 cargo test --test notification
/// ```
pub fn callback_host() -> &'static str {
    static HOST: LazyLock<String> = LazyLock::new(|| env::var(CALLBACK_HOST_ENV).unwrap_or_else(|_| DEFAULT_CALLBACK_HOST.to_owned()));
    &HOST
}

/// URL the broker should dial to reach a mock listening on `port`.
///
/// `path` is appended verbatim and is expected to start with `/`; pass `""`
/// for a bare origin.
pub fn callback_url(port: u16, path: &str) -> String {
    format!("http://{}:{port}{path}", callback_host())
}

/// NGSI-LD v1.9 Core `@context` URL.
pub const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// ETSI NGSI-LD Test Suite `@context` URL (base variant).
pub const TEST_SUITE_CONTEXT: &str = "https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite.jsonld";

/// ETSI NGSI-LD Test Suite compound `@context` URL used by the Building fixtures.
pub const COMPOUND_CONTEXT: &str =
    "https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite-compound.jsonld";
