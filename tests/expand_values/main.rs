//! `expandValues` query parameter integration tests (NGSI-LD 6.3.7).
//!
//! `expandValues` names a comma-separated list of attribute paths whose
//! string values the broker must expand against the active `@context`
//! before applying a `q=` filter. The parameter appears on entity
//! queries, temporal queries, and subscription `q` filters.
//!
//! Authoring detailed tests requires fixtures whose `@context` defines
//! term-to-IRI mappings distinct from the default context so the
//! expansion is observable on the outgoing JSON. That fixture work is
//! deferred; this scaffold documents the gap.
//!
//! Naming convention: `expand_values_<surface>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

#[tokio::test]
#[ignore = "expandValues coverage requires context-local term→IRI fixtures; follow-up: author entity-query, temporal-query, and subscription scenarios with a dedicated test @context that makes expansion observable."]
async fn expand_values_follow_up_requires_context_specific_fixture() {}
