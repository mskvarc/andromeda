//! Distributed-operations conformance suite.
//!
//! A standalone, real-setup validation of Andromeda's NGSI-LD distributed
//! subsystem (ETSI GS CIM 009 v1.9.1): Context Source Registration, discovery,
//! the four registration modes, provision and consumption forwarding, entity
//! maps, CSR subscriptions and notifications, context source identity, and the
//! HTTP binding that carries them.
//!
//! The suite runs against the multi-broker compose stack in
//! `docker/distributed/` (brokers A/B/C, host ports 1061/1062/1063) plus
//! in-process mock Context Sources for fault injection and exact forwarded-request
//! assertions. Topologies, requirements, and the conformance ratchet are
//! documented in `docs/distributed-operations-test-plan.md` and the companion
//! `docs/distributed-tests/` catalog.
//!
//! Bring the stack up and source the environment before running:
//!
//! ```sh
//! docker compose -f docker/distributed/docker-compose.distributed.yaml up --build -d
//! set -a && . docker/distributed/distributed.env && set +a
//! cargo test --test distop -- --test-threads=1
//! cargo test --test distop -- --ignored --test-threads=1  # tracked gap tests (ratchet)
//! ```
//!
//! Run single-threaded. A broker's Context Source Registration namespace is
//! global per (broker, tenant): an exclusive or redirect CSR registered by
//! entity type applies to every concurrent request, so a type-scoped exclusive
//! `Vehicle` CSR in one test would make another test's local `Vehicle` create
//! fail with 409 Conflict. Prefix isolation namespaces entity ids, not the
//! shared registration surface, so these tests are serialised rather than run in
//! parallel against the shared stack.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

#[path = "../support/context_source_adapter.rs"]
pub mod context_source_adapter;
#[path = "../support/notification_adapter.rs"]
pub mod notification_adapter;

mod entity;
mod gaps;
mod oracle;
mod registration;
mod topology;

mod cons;
mod disc;
mod fw;
mod http;
mod map;
mod prov;
mod reg;
mod smoke;
