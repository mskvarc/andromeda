//! `@context` cache expiration tests (NGSI-LD 6.3.16).
//!
//! NGSI-LD 6.3.16 mandates that a broker respects `Cache-Control` and
//! `Expires` when fetching external `@context` documents, with a
//! heuristic fallback when neither is present.
//!
//! Exercising the cache respects-headers behaviour requires a local HTTP
//! server that serves `@context` documents with controlled response
//! headers and counts inbound fetches. That infrastructure (akin to the
//! `ContextSourceMock` in `andromeda-test-common::mock`) does not yet
//! have a JSON-LD-context-specific variant. Scaffold documents the gap.

#![allow(clippy::expect_used, clippy::unwrap_used)]

#[tokio::test]
#[ignore = "Needs a host-local JSON-LD @context server that counts fetches and returns chosen Cache-Control/Expires headers; follow-up: add a `ContextDocumentServer` helper to andromeda-test-common::mock and populate the scenarios."]
async fn cache_expiration_follow_up_requires_context_document_server() {}
