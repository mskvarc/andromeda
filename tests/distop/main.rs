//! Distributed-operations conformance suite.
//!
//! A standalone, real-setup validation of Andromeda's NGSI-LD distributed
//! subsystem (ETSI GS CIM 009 v1.9.1): Context Source Registration, discovery,
//! the four registration modes, provision and consumption forwarding, entity
//! maps, CSR subscriptions and notifications, context source identity, and the
//! HTTP binding that carries them.
//!
//! The suite runs against the multi-broker compose stack in
//! `docker/compose/test/distributed/` (brokers A/B/C, host ports 1061/1062/1063) plus
//! in-process mock Context Sources for fault injection and exact forwarded-request
//! assertions. Topologies, requirements, and the conformance ratchet are
//! documented in `docs/distributed-operations-test-plan.md`.
//!
//! The canonical runner is `cargo xtask itest distributed`, which brings the
//! four-broker compose stack up, sources its environment, and runs this suite
//! with `--test-threads=1` enforced by the xtask stack definition. That is the
//! path CI takes and the authoritative distributed verdict. The manual
//! equivalent, for iterating against an already-running stack:
//!
//! ```sh
//! docker compose -f docker/compose/test/distributed/docker-compose.yaml up --build -d
//! set -a && . docker/compose/test/distributed/env && set +a
//! cargo test --test distop -- --test-threads=1
//! cargo test --test distop -- --ignored --test-threads=1  # tracked gap tests (ratchet)
//! ```
//!
//! Runs in parallel. Each topology runs under a tenant leased to it alone
//! (`andromeda_test_harness::tenant_lease`): clause 5.12 matches a type-only or
//! entity-less registration against every request of its tenant, so a shared
//! tenant would let one test's retrieval or query reach another test's context
//! sources however its identifiers were scoped. Inside that tenant each test
//! also derives a per-test entity type from its topology prefix
//! (`topology.id("Vehicle")`) and scopes every entity id, registration,
//! subscription selector, and `type=` query to it, and the tenant's database is
//! reset once per process rather than per test (see
//! `andromeda_test_harness::multi_broker`). The few tests that must use the
//! literal compact type `Vehicle` -- short-hand context expansion and the
//! forwarded-request compaction assertions -- carry
//! `#[serial_test::serial(distop_shared_type)]`, which serialises just those
//! among themselves while every other test runs concurrently. The xtask stack
//! definition sets the suite's `--test-threads`; the manual commands above pin
//! `--test-threads=1`, which is always safe to run.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

#[path = "../support/context_source_adapter.rs"]
pub mod context_source_adapter;
#[path = "../support/hosted_context.rs"]
pub mod hosted_context;
#[path = "../support/notification_adapter.rs"]
pub mod notification_adapter;

mod entity;
mod gaps;
mod oracle;
mod registration;
mod topology;

mod compaction;
mod cons;
mod disc;
mod fw;
mod http;
mod map;
mod prov;
mod reg;
mod smoke;
