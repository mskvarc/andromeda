//! Integration suite for `temporal.core_write_history`.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.3.1 leaves it to the broker whether a Core
//! API write updates the Temporal Evolution of an Entity: "Brokers that intend
//! to implement the Temporal API should consider updating the Temporal
//! Evolution of an Entity whenever the 'current state' is modified via the
//! Core API." A broker started with `core_write_history = "not-recorded"`
//! leaves every Core write -- entity, attribute, batch, purge and expiry --
//! out of history, keeps current state and everything that reads it exact, and
//! records the Temporal API exactly as before.
//!
//! The self-contained stack is
//! `docker/compose/test/core-write-history/docker-compose.yaml`: one PostgreSQL
//! and three brokers over the same database, so a test writes through one and
//! reads the Temporal Evolution back through any of them.
//!
//! - `core-write-history-not-recorded` (host :1101), the subject.
//! - `core-write-history-deferred` (host :1102), the subject with
//!   `append_mode = "deferred"`.
//! - `core-write-history-recorded` (host :1103), the default, which records
//!   Core writes: the control every contrast is drawn against.
//!
//! ```sh
//! cargo xtask itest --stack core-write-history
//! ```
//!
//! or by hand:
//!
//! ```sh
//! docker compose -f docker/compose/test/core-write-history/docker-compose.yaml up --build -d --wait
//! ANDROMEDA_TEST_BROKER_URL=http://127.0.0.1:1101/ngsi-ld/v1/ \
//! ANDROMEDA_TEST_DEFERRED_BROKER_URL=http://127.0.0.1:1102/ngsi-ld/v1/ \
//! ANDROMEDA_TEST_RECORDING_BROKER_URL=http://127.0.0.1:1103/ngsi-ld/v1/ \
//! ANDROMEDA_TEST_DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5501/postgres \
//! ANDROMEDA_TEST_CALLBACK_HOST=host.docker.internal \
//! cargo test --test core_write_history
//! docker compose -f docker/compose/test/core-write-history/docker-compose.yaml down -v
//! ```
//!
//! Brokers run on the host instead take `docker/compose/test/core-write-history/env`
//! and `ANDROMEDA_TEST_CALLBACK_HOST=127.0.0.1`.
//!
//! Naming convention: `<scenario>`, grouped by module per family of writes.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod attribute_writes;
mod batch_writes;
mod broker_roles;
mod current_state;
mod deferred_append;
mod entity_writes;
mod evolution;
mod notifications;
mod observation;
mod recording_control;
mod temporal_api;
mod tenants;
