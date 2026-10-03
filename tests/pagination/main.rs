//! Pagination Link header integration tests for the NGSI-LD context broker.
//!
//! Naming convention: `pagination_<endpoint>_<scenario>_<expected_outcome>`.
//!
//! Every test runs under a private tenant of its own, whose collections start
//! empty, against the base tier's default pagination policy (cursor by
//! default, offset on request). The suite of brokers started under other
//! policies is `tests/pagination_policy`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod csource_subscription;
mod cursor_walk;
mod entity;
mod query;
mod registration;
mod subscription;
mod temporal;
