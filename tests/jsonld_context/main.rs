//! The JSON-LD `@context` integration suite.
//!
//! Everything the broker does with a JSON-LD `@context`, against a running
//! broker: the clause 5.13 management operations, which `@context` a response
//! names in its body or its one `Link` (clauses 5.2.3, 4.4 and 6.3.6), how a
//! request names one (clause 6.3.5), what notifications name (clauses 5.8.1.4
//! and 5.8.6), serving stored documents to remote processors, downloading remote
//! ones, and the identifiers stored documents take.
//!
//! The suite runs against the base tier: the broker under test and a second
//! one on the same database (`ANDROMEDA_TEST_TRUSTED_PROXY_BROKER_URL`, default
//! port 1057), both started with a short `hosted_context_revalidation`
//! (`docker/compose/test/base/docker-compose.yaml`). A broker run on the host
//! needs `ANDROMEDA_TEST_CALLBACK_HOST=127.0.0.1` in the test process, so the
//! origins the tests serve contexts from are reachable. No case touches the
//! network: every remote `@context` is served by the test's own origin, and the
//! independent JSON-LD processor resolves the embedded core context offline.
//!
//! Every test runs in a private tenant, so the suite runs in parallel.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod body_context;
mod broker_endpoints;
mod broker_process;
mod context_api;
mod context_links;
mod context_origin;
mod core_contexts;
mod identity;
mod independent_processor;
mod link_context;
mod management;
mod ngsi_requests;
mod notification_context;
mod own_reference;
mod polling;
mod private_tenant;
mod problem_types;
mod remote;
mod request_admission;
mod serve;
mod unique_ids;
mod user_contexts;
