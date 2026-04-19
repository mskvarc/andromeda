//! `TooManyResults` (403) Problem-Details tests (NGSI-LD 5.5.2).
//!
//! Same situation as `TooComplexQuery`: the broker exposes the error type
//! but no production code path currently rejects a query for exceeding a
//! result-count ceiling. The scaffold documents the coverage gap; once
//! the ceiling lands, populate this test with a query that matches more
//! than the configured limit.

#![allow(clippy::expect_used, clippy::unwrap_used)]

#[tokio::test]
#[ignore = "Broker has no TooManyResults ceiling yet; follow-up: wire a configurable max-results bound and populate this test."]
async fn too_many_results_documents_gap_until_ceiling_configured() {}
