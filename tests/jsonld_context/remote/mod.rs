//! Downloading remote `@context` documents (clauses 5.5.6, 5.13.1 and 6.3.16).
//!
//! NGSI-LD sets no media type for a fetched context: it references only the
//! JSON-LD 1.1 Syntax specification, not the Processing API whose document
//! loader refuses a non-JSON `Content-Type`. Every document these cases fetch
//! is served by the test's own origin.

mod alternate_link;
mod caching;
mod media_types;
mod redirects;

use crate::{broker_endpoints::broker_url, ngsi_requests::stored_entity, unique_ids::unique_id};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};

/// The IRI the documents these cases serve map `speed` to.
const REMOTE_SPEED_IRI: &str = "https://example.org/remote#speed";

/// A context document mapping `speed` to [`REMOTE_SPEED_IRI`].
fn speed_context() -> String {
    json!({ "@context": { "speed": REMOTE_SPEED_IRI } }).to_string()
}

/// Create a `Vehicle` whose `speed` is named by the remote `context`, at
/// `entities_url`. Returns the status and, on success, the Entity as stored
/// (every term under the IRI the context gave it), or the problem detail.
async fn create_under(client: &Client, entities_url: &str, context: &str) -> (StatusCode, Value) {
    let id = unique_id("urn:ngsi-ld:Vehicle:remote");
    let response = client
        .post(entities_url)
        .header("Content-Type", "application/ld+json")
        .json(&json!({ "id": id, "type": "Vehicle", "speed": { "type": "Property", "value": 1 }, "@context": [context] }))
        .send()
        .await
        .expect("create request failed");
    let status = response.status();
    if !status.is_success() {
        return (status, response.json().await.unwrap_or_default());
    }
    (status, stored_entity(client, &id).await)
}

/// Create under `context` on the broker under test.
async fn create_on_broker(client: &Client, context: &str) -> (StatusCode, Value) {
    create_under(client, &broker_url("entities"), context).await
}
