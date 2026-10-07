//! `EntityMap` creation, update, and query-with-map requests expressed against the broker origin, since the test client exposes only retrieve/delete for `EntityMaps`. Creation is the `GET`/`POST` `/entityMaps` binding (clause 5.14.4), the temporal variant is `/temporal/entityMaps` (clause 5.14.5), and updates use `PATCH /entityMaps/{id}` (clause 5.14.2).

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use axum::http::Method;
use serde_json::Value;

/// The JSON-LD context Link header value carrying the compound test context, so query terms in the request expand the same way the entities were stored.
pub fn jsonld_link() -> String {
    format!(
        "<{}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"",
        COMPOUND_CONTEXT.as_str()
    )
}

/// Create an `EntityMap` from a `GET /entityMaps` query string (no leading `?`).
pub async fn create_entity_map(client: &TestClient, query: &str) -> TestResponse {
    let link = jsonld_link();
    client
        .raw(Method::GET, &format!("/ngsi-ld/v1/entityMaps?{query}"))
        .header("Link", link.as_str())
        .send()
        .await
        .expect("entity map creation request")
}

/// Create a temporal `EntityMap` from a `GET /temporal/entityMaps` query string.
pub async fn create_temporal_entity_map(client: &TestClient, query: &str) -> TestResponse {
    let link = jsonld_link();
    client
        .raw(Method::GET, &format!("/ngsi-ld/v1/temporal/entityMaps?{query}"))
        .header("Link", link.as_str())
        .send()
        .await
        .expect("temporal entity map creation request")
}

/// Partially update an `EntityMap` through `PATCH /entityMaps/{id}`.
pub async fn update_entity_map(client: &TestClient, id: &str, patch: &Value) -> TestResponse {
    let encoded = urlencoding::encode(id);
    client
        .raw(Method::PATCH, &format!("/ngsi-ld/v1/entityMaps/{encoded}"))
        .header("Content-Type", "application/json")
        .json(patch)
        .send()
        .await
        .expect("entity map update request")
}
