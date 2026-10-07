//! Shared scaffolding for HTTP-binding tests that forward to a mock Context Source, so a fault-injection row expresses its intent (a covered entity, a registration mode, an injected upstream response) rather than re-deriving the forwarded resource paths and registration shape.
//!
//! The forwarded resource paths are anchored at `/ngsi-ld/v1/...` because the broker rebuilds the canonical NGSI-LD path against the registration endpoint authority (clause 6.3.7), and the mock matches request paths verbatim.

use crate::registration::CsrBuilder;
use andromeda_test_client::header::jsonld_link_header;
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use reqwest::header::HeaderValue;
use serde_json::Value;

/// A syntactically valid peer endpoint for a registration whose federation behaviour is not under test, so the endpoint is never actually contacted.
pub const PLACEHOLDER_ENDPOINT: &str = "http://context-source.example/ngsi-ld/v1";

/// The forwarded collection path for entity query and create operations.
pub const FORWARDED_ENTITIES_PATH: &str = "/ngsi-ld/v1/entities";

/// The forwarded path of a Batch Entity Merge (clause 6.20.3.1).
pub const FORWARDED_BATCH_MERGE_PATH: &str = "/ngsi-ld/v1/entityOperations/merge";

/// The forwarded resource path for a single entity, with the id spliced in verbatim (the broker forwards the URN unescaped, as the mock matches it).
pub fn forwarded_entity_path(id: &str) -> String {
    format!("/ngsi-ld/v1/entities/{id}")
}

/// The forwarded `attrs` collection path for one entity.
pub fn forwarded_attrs_path(id: &str) -> String {
    format!("/ngsi-ld/v1/entities/{id}/attrs")
}

/// The forwarded path for one named attribute of an entity.
pub fn forwarded_attr_path(id: &str, attribute: &str) -> String {
    format!("/ngsi-ld/v1/entities/{id}/attrs/{attribute}")
}

/// The compound test-suite `@context` IRI as a Link-header value, used to expand `Vehicle` and its attributes on forwarded query and retrieve requests.
pub fn context_link() -> &'static str {
    COMPOUND_CONTEXT.as_str()
}

/// The compound test-suite `@context` as a JSON-LD `Link` header value, for raw requests that must expand `Vehicle` and its attributes (clause 6.3.5).
pub fn context_link_header() -> HeaderValue {
    jsonld_link_header(context_link()).expect("compound context is a valid Link header")
}

/// A registration covering one entity id of `entity_type` under `mode`, advertising the retrieval operation group so a query or retrieve issued to the front broker is forwarded to `endpoint`. The type is passed explicitly so a test can derive it per run and keep a bare `type=` selector from crossing into another concurrent test.
pub fn retrieval_source(reg_id: &str, endpoint: &str, entity_id: &str, entity_type: &str, mode: &str) -> Value {
    CsrBuilder::new(reg_id, endpoint)
        .mode(mode)
        .entity_type(entity_type)
        .property_names(&["speed"])
        .entity_id(entity_id)
        .operations(&["retrieveEntity", "queryEntity", "queryBatch"])
        .build()
}

/// A registration covering one entity id of `entity_type` under `mode`, advertising the given provision operations so a matching write issued to the front broker is forwarded to `endpoint`.
pub fn provision_source(reg_id: &str, endpoint: &str, entity_id: &str, entity_type: &str, mode: &str, operations: &[&str]) -> Value {
    CsrBuilder::new(reg_id, endpoint)
        .mode(mode)
        .entity_type(entity_type)
        .entity_id(entity_id)
        .property_names(&["speed"])
        .operations(operations)
        .build()
}
