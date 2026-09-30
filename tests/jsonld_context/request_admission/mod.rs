//! Where a request names its `@context`, and what the broker refuses (study
//! group C).
//!
//! Clause 6.3.5 reads a request's context from one `Link` header with the
//! JSON-LD context relation, or from the body of an `application/ld+json`
//! request, never from both: "the presence of a JSON-LD Link header in the
//! incoming HTTP request when the Content-Type header is "application/ld+json"
//! shall result in an HTTP error response of type `BadRequestData`". A document
//! naming several context links is the JSON-LD 1.1 API's "multiple context link
//! headers" error, so a request naming several is refused, identical or not.
//! The relation is matched case-insensitively (IETF RFC 8288 section 2.1.2), a
//! relative target resolves against the request's own URI (section 3.1), and a
//! `profile` link (clause 6.3.21) is not a context at all. Each item of an
//! `application/ld+json` batch is expanded under its own `@context`. A context
//! whose references cycle or run deeper than the broker follows is refused.

mod batch_items;
mod body_forms;
mod link_forms;
mod reference_chains;
mod several_links;

use crate::{broker_endpoints::broker_url, problem_types::BAD_REQUEST_DATA_TYPE, unique_ids::unique_id};
use reqwest::{Client, RequestBuilder, StatusCode};
use serde_json::{Value, json};

/// Create a `Vehicle` whose `speed` is expanded through the core context only.
async fn create_plain_vehicle(client: &Client) -> String {
    let id = unique_id("urn:ngsi-ld:Vehicle:request-context");
    let response = client
        .post(broker_url("entities"))
        .header("Content-Type", "application/json")
        .json(&json!({ "id": id, "type": "Vehicle", "speed": { "type": "Property", "value": 1 } }))
        .send()
        .await
        .expect("create request failed");
    assert_eq!(response.status(), StatusCode::CREATED);
    id
}

/// Send `request` and assert it is refused as clause 5.5.4 `BadRequestData`.
async fn assert_bad_request_data(request: RequestBuilder, why: &str) {
    let response = request.send().await.expect("request failed");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{why}");
    let body: Value = response.json().await.expect("a problem details body");
    assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str(), "{why}: {body}");
}

/// The four verbs a context `Link` accompanies, each addressing `id` with a
/// body where the verb takes one.
fn requests_for_every_verb(client: &Client, id: &str) -> Vec<(&'static str, RequestBuilder)> {
    vec![
        ("GET", client.get(broker_url(&format!("entities/{id}")))),
        ("DELETE", client.delete(broker_url(&format!("entities/{id}")))),
        (
            "POST",
            client
                .post(broker_url("entities"))
                .header("Content-Type", "application/json")
                .json(&json!({ "id": unique_id("urn:ngsi-ld:Vehicle:refused"), "type": "Vehicle" })),
        ),
        (
            "PATCH",
            client
                .patch(broker_url(&format!("entities/{id}/attrs")))
                .header("Content-Type", "application/json")
                .json(&json!({ "speed": { "type": "Property", "value": 2 } })),
        ),
    ]
}

/// A context document mapping `speed` to `iri`.
fn speed_context(iri: &str) -> Value {
    json!({ "@context": { "speed": iri } })
}
