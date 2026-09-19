//! Engine-based term compaction on the forwarding path (ETSI GS CIM 009
//! v1.9.1, clause 4.3.6.1: "all constraints specified in the registration shall
//! be respected").
//!
//! A registration stores the entity types and attributes it covers expanded (as
//! IRIs). When the broker forwards a distributed request it must name them in
//! the compact spelling the source's `@context` uses. A last-segment heuristic
//! gets that right only when a term equals the final fragment of its IRI; under
//! a `@context` that aliases a term to an unrelated IRI (`spd` ->
//! `.../vocab/speed`, `Veh` -> `.../vocab/Vehicle`) it forwards the wrong token
//! or drops a covered term, and the client gets an incomplete answer with no
//! error. These tests host such a context, register a Context Source under it,
//! drive a request through the broker, and read the exact term the broker
//! forwarded from the in-process mock source -- asserting the alias, never the
//! IRI's last segment.
//!
//! Concrete per-test designs mirror `tests/regression/discovery_name_compaction.rs`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::{context_source_adapter::ContextSourceAdapter, hosted_context};
use andromeda_test_mocks::jsonld_context_server::JsonLdContextServer;
use axum::http::{HeaderMap, Method};
use serde_json::{Value, json};
use std::time::Duration;

mod read;
mod retrieve;
mod temporal;
mod write;

/// Budget for a forwarded request to reach the mock context source.
pub(crate) const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

/// The path a broker appends to a registration `endpoint` when forwarding a
/// current-entity query or create (clause 4.3.6.3).
pub(crate) const FORWARDED_ENTITIES: &str = "/ngsi-ld/v1/entities";

/// The path a broker appends when forwarding a temporal query (clause 5.7.4.4).
pub(crate) const FORWARDED_TEMPORAL: &str = "/ngsi-ld/v1/temporal/entities";

/// The core context URL, layered under the hosted alias so a registration's
/// non-aliased members still resolve.
pub(crate) const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// The IRI the alias `spd` expands to; its last segment is `speed`, the token a
/// last-segment heuristic would have produced.
pub(crate) const SPEED_IRI: &str = "https://example.org/vocab/speed";

/// The IRI the alias `Veh` expands to; its last segment is `Vehicle`.
pub(crate) const VEHICLE_IRI: &str = "https://example.org/vocab/Vehicle";

/// A `@context` document aliasing `spd`/`Veh` to IRIs whose last segments
/// (`speed`/`Vehicle`) differ from the aliases.
pub(crate) fn aliased_document() -> String {
    json!({ "@context": { "spd": SPEED_IRI, "Veh": VEHICLE_IRI } }).to_string()
}

/// A `@context` document that additionally aliases the literal term `speed` to a
/// *different* IRI, so a covered `spd` (`.../vocab/speed`) and a requested
/// `speed` (`.../other/speed`) share a last segment but not an IRI.
pub(crate) fn coinciding_segment_document() -> String {
    json!({ "@context": { "spd": SPEED_IRI, "Veh": VEHICLE_IRI, "speed": "https://example.org/other/speed" } }).to_string()
}

/// The `@context` a registration is created under: the hosted alias layered
/// over the core context.
pub(crate) fn registration_context(context_url: &str) -> Value {
    json!([context_url, CORE_CONTEXT])
}

/// Serve `document` on `origin` under a path unique to this run, answering the
/// URL the broker fetches it from.
pub(crate) async fn serve(origin: &JsonLdContextServer, document: &str) -> String {
    let (_, url) = hosted_context::serve(origin, "distop-compaction", document, HeaderMap::new()).await;
    url
}

/// Assert the forwarded query string names the aliased term and never the IRI's
/// last segment, for both the type and attribute axes as requested.
pub(crate) fn assert_no_last_segment(forwarded_query: &str) {
    assert!(
        !forwarded_query.contains("speed"),
        "the attribute IRI's last segment must not leak: {forwarded_query}"
    );
    assert!(
        !forwarded_query.contains("Vehicle"),
        "the type IRI's last segment must not leak: {forwarded_query}"
    );
}

/// The count of requests the mock forwarded to the current-entities collection.
pub(crate) fn entity_forward_count(mock: &ContextSourceAdapter) -> usize {
    mock.get_count(Method::GET, FORWARDED_ENTITIES)
}
