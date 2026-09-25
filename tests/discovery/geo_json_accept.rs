//! `application/geo+json` Accept negotiation (NGSI-LD 6.3.4, 5.7.1, 5.7.2).
//!
//! `GeoJSON` responses are valid only for Retrieve Entity and Query Entities. Any
//! other resource must answer `application/geo+json` with 406 Not Acceptable.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use reqwest::StatusCode;

use crate::{BASE_URL, COMPOUND_CONTEXT_URL, client, link_header};

#[tokio::test]
async fn list_types_rejects_geo_json_accept_with_406() {
    // Clause 6.3.4: `/types` does not support GeoJSON, so a geo+json Accept is
    // 406 Not Acceptable rather than a mis-typed response.
    let c = client();

    let response = c
        .get(format!("{BASE_URL}/types"))
        .header("Accept", "application/geo+json")
        .send()
        .await
        .expect("list types request failed");
    pretty_assertions::assert_eq!(response.status(), StatusCode::NOT_ACCEPTABLE);
    // Clause 6.3.4: the refusal's "body of the message shall contain the list
    // of the available representations of the resources", and `/types` has no
    // GeoJSON form to offer among them.
    let body: serde_json::Value = response.json().await.expect("clause 6.3.4 requires a body on a 406");
    pretty_assertions::assert_eq!(
        body,
        serde_json::json!({"availableRepresentations": ["application/json", "application/ld+json"]})
    );
}

#[tokio::test]
async fn query_entities_accepts_geo_json_with_200() {
    // Clause 5.7.2: Query Entities does support GeoJSON, so the same Accept is
    // honoured with a 200 rather than refused.
    let c = client();

    let response = c
        .get(format!("{BASE_URL}/entities?type=Building"))
        .header("Accept", "application/geo+json")
        .header("Link", link_header(COMPOUND_CONTEXT_URL.as_str()))
        .send()
        .await
        .expect("query entities request failed");
    pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
}
