//! Integration suite for Andromeda's opt-in extensions.
//!
//! A general home for extension integration tests: one module per feature, so a
//! future extension adds a sibling rather than growing an existing file.
//!
//! The self-contained stack is `docker/compose/test/extensions/docker-compose.yaml`
//! — one Postgres plus two brokers: `extensions-enabled` (host :1091, both
//! extensions on) and `extensions-conformance` (host :1092, `conformance_mode`
//! on with the flags also on, which the `gate` module targets):
//!
//! ```sh
//! docker compose -f docker/compose/test/extensions/docker-compose.yaml up --build -d
//! ANDROMEDA_TEST_BROKER_URL=http://127.0.0.1:1091/ngsi-ld/v1 \
//! ANDROMEDA_TEST_DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5491/postgres \
//! ANDROMEDA_TEST_CALLBACK_HOST=127.0.0.1 \
//! ANDROMEDA_TEST_CONFORMANCE_BROKER_URL=http://127.0.0.1:1092/ngsi-ld/v1 \
//! cargo test --test extensions -- --test-threads=1
//! docker compose -f docker/compose/test/extensions/docker-compose.yaml down -v
//! ```
//!
//! Alternatively, run the brokers on the host against any Postgres with the
//! variables in `docker/compose/test/extensions/env`. The `gate` module skips its
//! tests when `ANDROMEDA_TEST_CONFORMANCE_BROKER_URL` is unset, so the suite
//! runs without the second broker.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod gate;
mod purge_dataset_scope;
mod seeding;
mod temporal_aggregation_grouping;
mod temporal_deep_chain;
mod temporal_delete_by_query;
mod temporal_join;
mod temporal_linked_retrieval;
