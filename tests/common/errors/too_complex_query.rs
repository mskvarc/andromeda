//! `TooComplexQuery` (403) Problem-Details tests (NGSI-LD 5.5.2).
//!
//! The framework-independent category exists in `andromeda-ngsi-ld-problem-details`, but no policy path currently classifies a failure as `TooComplexQuery`: there is no configured ceiling on `q=` complexity. The scaffold documents the coverage gap; once a ceiling is introduced the `#[ignore]` can be dropped and the test body populated with a query that exceeds the limit.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

#[tokio::test]
#[ignore = "Broker has no TooComplexQuery ceiling yet; follow-up: wire a configurable q= complexity bound and populate this test."]
async fn too_complex_query_documents_gap_until_ceiling_configured() {}
