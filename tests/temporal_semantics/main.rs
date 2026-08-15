//! Verification tests for the redesigned temporal storage semantics.
//!
//! Every test here implements one row of the verification matrix in
//! `docs/proposals/database-schema-redesign.md`: the four independent semantic
//! time axes, the operational write clock and its ordinals, the split instance
//! identity, the tombstone encoding, the three deletion operations clauses
//! 5.6.6, 5.6.15 and 5.6.16 define, operator retention, and the timestamp
//! precision policy every stored instant is normalised under.
//!
//! Assertions reach two surfaces. What a client can observe is asserted through
//! the HTTP API, and what only storage can answer — which column a timestamp
//! landed in, which instance-identity column is populated, whether a chunk was
//! dropped — is asserted against the tenant database through the harness
//! connection. A row that the matrix states as a storage property is checked in
//! storage rather than inferred from a rendering.
//!
//! Requests carry no `Link` header and use `application/json`, so every
//! attribute term expands through the NGSI-LD Core Context alone and the suite
//! needs no reachable remote context document.
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly.
//!
//! Naming convention: `temporal_semantics_<row>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod deletion;
mod instance_identity;
mod kind_history;
mod late_observations;
mod plural_metadata;
mod retention;
mod seeded_temporal;
mod stored_history;
mod time_axes;
mod timestamp_precision;
mod write_idempotency;
mod write_ordering;

use andromeda_test_harness::{
    configuration::RUNTIME_BROKER_BASE as BASE_URL,
    database_access::connect_test_database,
    serialized_environment::SerializedEnvironment,
};
use reqwest::{Client, StatusCode};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

/// A plain HTTP client; every request declares its own headers.
fn client() -> Client {
    Client::new()
}

/// A fixture identifier no other test in the suite can collide with.
fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

/// Take exclusive ownership of the tenant and empty its mutable tables.
async fn clean_database() -> SerializedEnvironment {
    SerializedEnvironment::from_environment_reset()
        .await
        .expect("serialized test database environment")
}

/// Connect to the tenant database for the assertions only storage can answer.
async fn database() -> PgPool {
    connect_test_database().await.expect("tenant database connection")
}

/// The IRI a Core-Context-only term expands to.
///
/// Requests in this suite carry no user context, so clause 5.5.7's default
/// context is what every attribute name is expanded through, and that is the
/// name `attribute_names` stores.
fn attribute_iri(term: &str) -> String {
    format!("https://uri.etsi.org/ngsi-ld/default-context/{term}")
}

// ---------------------------------------------------------------------------
// Current-state operations
// ---------------------------------------------------------------------------

/// Create one Entity through clause 5.6.1.
async fn create_entity(http: &Client, entity: &Value) -> StatusCode {
    http.post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/json")
        .json(entity)
        .send()
        .await
        .expect("entity creation request")
        .status()
}

/// Create a batch of Entities through clause 5.6.7.
async fn create_entity_batch(http: &Client, entities: &Value) -> StatusCode {
    http.post(format!("{BASE_URL}/entityOperations/create"))
        .header("Content-Type", "application/json")
        .json(entities)
        .send()
        .await
        .expect("batch creation request")
        .status()
}

/// Upsert a batch of Entities through clause 5.6.8, updating what exists.
async fn upsert_entity_batch(http: &Client, entities: &Value) -> StatusCode {
    http.post(format!("{BASE_URL}/entityOperations/upsert?options=update"))
        .header("Content-Type", "application/json")
        .json(entities)
        .send()
        .await
        .expect("batch upsert request")
        .status()
}

/// Update Entity Attributes through clause 5.6.2.
async fn update_entity_attributes(http: &Client, entity_id: &str, fragment: &Value) -> StatusCode {
    http.patch(format!("{BASE_URL}/entities/{entity_id}/attrs"))
        .header("Content-Type", "application/json")
        .json(fragment)
        .send()
        .await
        .expect("attribute update request")
        .status()
}

/// Delete one Entity Attribute through clause 5.6.5.
async fn delete_entity_attribute(http: &Client, entity_id: &str, attribute: &str) -> StatusCode {
    http.delete(format!("{BASE_URL}/entities/{entity_id}/attrs/{attribute}"))
        .send()
        .await
        .expect("attribute deletion request")
        .status()
}

/// Delete one Entity through clause 5.6.6.
async fn delete_entity(http: &Client, entity_id: &str) -> StatusCode {
    http.delete(format!("{BASE_URL}/entities/{entity_id}"))
        .send()
        .await
        .expect("entity deletion request")
        .status()
}

/// Retrieve one Entity through clause 5.6.5.
async fn get_entity(http: &Client, entity_id: &str) -> (StatusCode, Value) {
    let response = http
        .get(format!("{BASE_URL}/entities/{entity_id}"))
        .header("Accept", "application/json")
        .send()
        .await
        .expect("entity retrieval request");
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

// ---------------------------------------------------------------------------
// Temporal operations
// ---------------------------------------------------------------------------

/// Create or update a Temporal Representation through clause 5.6.11.
async fn create_temporal_entity(http: &Client, entity: &Value) -> (StatusCode, Value) {
    let response = http
        .post(format!("{BASE_URL}/temporal/entities"))
        .header("Content-Type", "application/json")
        .json(entity)
        .send()
        .await
        .expect("temporal creation request");
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

/// Retrieve a Temporal Representation through clause 5.6.13.
async fn get_temporal_entity(http: &Client, entity_id: &str, query: &str) -> (StatusCode, Value) {
    let suffix = if query.is_empty() { String::new() } else { format!("?{query}") };
    let response = http
        .get(format!("{BASE_URL}/temporal/entities/{entity_id}{suffix}"))
        .header("Accept", "application/json")
        .send()
        .await
        .expect("temporal retrieval request");
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

/// Delete one temporal attribute instance through clause 5.6.15.
async fn delete_temporal_instance(http: &Client, entity_id: &str, attribute: &str, instance_id: &str) -> StatusCode {
    let instance_id = urlencoding::encode(instance_id);
    http.delete(format!("{BASE_URL}/temporal/entities/{entity_id}/attrs/{attribute}/{instance_id}"))
        .send()
        .await
        .expect("instance deletion request")
        .status()
}

/// Delete a Temporal Evolution through clause 5.6.16.
async fn delete_temporal_entity(http: &Client, entity_id: &str) -> StatusCode {
    http.delete(format!("{BASE_URL}/temporal/entities/{entity_id}"))
        .send()
        .await
        .expect("temporal evolution deletion request")
        .status()
}

// ---------------------------------------------------------------------------
// Rendering accessors
// ---------------------------------------------------------------------------

/// The rendered instances of one attribute, or an empty slice when the
/// attribute is absent from the answer.
fn rendered_instances<'a>(body: &'a Value, term: &str) -> &'a [Value] {
    body.get(term).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}
