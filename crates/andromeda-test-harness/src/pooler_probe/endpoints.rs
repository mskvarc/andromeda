//! The transaction-pooling tier's endpoint URLs, read from the environment the
//! xtask runner sets, with defaults matching a hand-brought-up stack, and the
//! `application_name` each broker connects to `PostgreSQL` under.

use std::env;

/// The pooled broker under test's `application_name` (its compose `PGAPPNAME`).
pub const BROKER_APPLICATION_NAME: &str = "broker";

/// The second pooled replica's `application_name`.
pub const BROKER_REPLICA_APPLICATION_NAME: &str = "broker-replica";

/// The wide-cache pooled broker's `application_name`.
pub const BROKER_WIDE_APPLICATION_NAME: &str = "broker-wide";

/// The un-pooled baseline broker's `application_name`.
pub const BROKER_DIRECT_APPLICATION_NAME: &str = "broker-direct";

/// Read `key` from the environment, falling back to `default`.
fn var_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_owned())
}

/// The pooled broker under test (port 1067).
#[must_use]
pub fn broker() -> String {
    var_or("ANDROMEDA_TEST_BROKER_URL", "http://127.0.0.1:1067/ngsi-ld/v1/")
}

/// The second pooled replica broker (port 1068).
#[must_use]
pub fn broker_replica() -> String {
    var_or("ANDROMEDA_TEST_BROKER_REPLICA_URL", "http://127.0.0.1:1068/ngsi-ld/v1/")
}

/// The wide-cache pooled broker (port 1069).
#[must_use]
pub fn broker_wide() -> String {
    var_or("ANDROMEDA_TEST_BROKER_WIDE_URL", "http://127.0.0.1:1069/ngsi-ld/v1/")
}

/// The un-pooled baseline broker (port 1070).
#[must_use]
pub fn broker_direct() -> String {
    var_or("ANDROMEDA_TEST_BROKER_DIRECT_URL", "http://127.0.0.1:1070/ngsi-ld/v1/")
}

/// The direct PostgreSQL endpoint the suites sample and clean up over.
#[must_use]
pub fn database_url() -> String {
    var_or("ANDROMEDA_TEST_DATABASE_URL", "postgres://postgres:postgres@127.0.0.1:5467/postgres")
}

/// The `PostgreSQL` control database reached through `PgBouncer`, as a pooled
/// broker reaches it.
#[must_use]
pub fn pooler_url() -> String {
    var_or("ANDROMEDA_TEST_POOLER_URL", "postgres://postgres:postgres@127.0.0.1:6467/postgres")
}

/// The `PgBouncer` admin-console endpoint (db `pgbouncer`).
#[must_use]
pub fn pgbouncer_admin_url() -> String {
    var_or("ANDROMEDA_TEST_PGBOUNCER_ADMIN_URL", "postgres://postgres:postgres@127.0.0.1:6467/pgbouncer")
}
