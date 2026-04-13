//! Standalone i18n and l10n integration tests.
//!
//! This suite uses `tokio::test` + raw `reqwest`, with inline payloads and no
//! shared fixture files or abstracted client wrappers.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use uuid::Uuid;

const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";
const IN_LANGUAGE_IRI: &str = "https://schema.org/inLanguage";
const SAME_AS_IRI: &str = "https://schema.org/sameAs";
const BOOKING_URL_IRI: &str = "https://uri.etsi.org/ngsi-ld/default-context/bookingUrl";

fn client() -> Client {
    Client::new()
}

fn unique_id(prefix: &str) -> String {
    format!("urn:ngsi-ld:{}:{}", prefix, Uuid::new_v4())
}

async fn create_entity(client: &Client, body: &Value) -> StatusCode {
    client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create entity request failed")
        .status()
}

async fn get_entity(client: &Client, id: &str) -> Value {
    client
        .get(format!("{BASE_URL}/entities/{}", urlencoding::encode(id)))
        .header("Accept", "application/json")
        .send()
        .await
        .expect("get entity request failed")
        .json()
        .await
        .expect("entity json")
}

async fn query_entities(client: &Client, query: &str) -> Value {
    client
        .get(format!("{BASE_URL}/entities?{query}"))
        .header("Accept", "application/json")
        .send()
        .await
        .expect("query entities request failed")
        .json()
        .await
        .expect("query json")
}

async fn get_localized_entity(client: &Client, id: &str, query: &str, accept: &str) -> reqwest::Response {
    client
        .get(format!("{BASE_URL}/i18n/entities/{}?{query}", urlencoding::encode(id)))
        .header("Accept", accept)
        .send()
        .await
        .expect("localized get request failed")
}

async fn query_localized_entities(client: &Client, query: &str) -> Value {
    client
        .get(format!("{BASE_URL}/i18n/entities?{query}"))
        .header("Accept", "application/json")
        .send()
        .await
        .expect("localized query request failed")
        .json()
        .await
        .expect("localized query json")
}

async fn delete_entity(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/entities/{}", urlencoding::encode(id))).send().await;
}

fn read_id(value: &Value) -> Option<&str> {
    value.get("id").and_then(Value::as_str).or_else(|| value.get("@id").and_then(Value::as_str))
}

fn property_value<'a>(value: &'a Value, compact_key: &str, expanded_key: &str) -> Option<&'a Value> {
    value
        .get(compact_key)
        .or_else(|| value.get(expanded_key))
        .and_then(|property| property.get("value"))
}

#[tokio::test]
async fn i18n_entity_language_association_round_trips() {
    let client = client();
    let id = unique_id("Event-i18n-language");
    let entity = json!({
        "id": id,
        "type": "Event",
        "name": {"type": "Property", "value": "Bonjour le Monde"},
        "description": {"type": "Property", "value": "Simple text"},
        "inLanguage": {"type": "Property", "value": "fr"},
        "@context": [{"inLanguage": "https://schema.org/inLanguage"}, CORE_CONTEXT]
    });

    pretty_assertions::assert_eq!(create_entity(&client, &entity).await, StatusCode::CREATED);
    let retrieved = get_entity(&client, &id).await;
    pretty_assertions::assert_eq!(property_value(&retrieved, "inLanguage", IN_LANGUAGE_IRI), Some(&json!("fr")));
    delete_entity(&client, &id).await;
}

#[tokio::test]
async fn i18n_property_language_metadata_is_queryable() {
    let client = client();
    let id = unique_id("Hotel-i18n-property-language");
    let entity = json!({
        "id": id,
        "type": "Hotel",
        "bookingUrl": {
            "type": "Property",
            "value": ["http://example.org/de", "http://example.org/en"],
            "inLanguage": {"type": "Property", "value": ["de", "en"]}
        },
        "@context": [{"inLanguage": "https://schema.org/inLanguage"}, CORE_CONTEXT]
    });

    pretty_assertions::assert_eq!(create_entity(&client, &entity).await, StatusCode::CREATED);
    let q = format!("\"{BOOKING_URL_IRI}\".\"{IN_LANGUAGE_IRI}\"==\"de\"");
    let query = format!("type=Hotel&q={}", urlencoding::encode(&q));
    let queried = query_entities(&client, &query).await;
    assert!(
        queried
            .as_array()
            .is_some_and(|items| items.iter().any(|item| read_id(item) == Some(id.as_str())))
    );
    delete_entity(&client, &id).await;
}

#[tokio::test]
async fn i18n_same_as_relationship_language_metadata_is_queryable() {
    let client = client();
    let id = unique_id("Event-i18n-same-as");
    let entity = json!({
        "id": id,
        "type": "Event",
        "sameAs": [{
            "type": "Relationship",
            "datasetId": "urn:ngsi-ld:Relationship:1",
            "object": "urn:ngsi-ld:Event:helloWorld",
            "inLanguage": {"type": "Property", "value": "en"}
        }],
        "@context": [{
            "sameAs": "https://schema.org/sameAs",
            "inLanguage": "https://schema.org/inLanguage"
        }, CORE_CONTEXT]
    });

    pretty_assertions::assert_eq!(create_entity(&client, &entity).await, StatusCode::CREATED);
    let q = format!("\"{SAME_AS_IRI}\".\"{IN_LANGUAGE_IRI}\"==\"en\"");
    let query = format!("type=Event&q={}", urlencoding::encode(&q));
    let queried = query_entities(&client, &query).await;
    assert!(
        queried
            .as_array()
            .is_some_and(|items| items.iter().any(|item| read_id(item) == Some(id.as_str())))
    );
    delete_entity(&client, &id).await;
}

#[tokio::test]
async fn i18n_collate_metadata_is_generated_for_string_properties() {
    let client = client();
    let id = unique_id("Building-i18n-collate");
    let entity = json!({
        "id": id,
        "type": "Building",
        "name": {"type": "Property", "value": "Schone Grusse"},
        "@context": [CORE_CONTEXT]
    });

    pretty_assertions::assert_eq!(create_entity(&client, &entity).await, StatusCode::CREATED);
    let retrieved = get_entity(&client, &id).await;
    // Andromeda core context is always included in compaction, so `collate`
    // appears as a short name.  In compact normalized form the sub-attribute
    // is an object (not array-wrapped).
    let collate_value = retrieved
        .get("name")
        .and_then(|name| name.get("collate"))
        .and_then(|collate| collate.get("value"));
    pretty_assertions::assert_eq!(collate_value, Some(&json!("schone grusse")));
    delete_entity(&client, &id).await;
}

#[tokio::test]
async fn i18n_collate_query_matches_normalized_rhs() {
    let client = client();
    let id = unique_id("Building-i18n-collate-query");
    let entity = json!({
        "id": id,
        "type": "Building",
        "name": {"type": "Property", "value": "Schone Grusse"},
        "@context": [CORE_CONTEXT]
    });

    pretty_assertions::assert_eq!(create_entity(&client, &entity).await, StatusCode::CREATED);
    let queried = query_entities(&client, "type=Building&q=name==\"Schone Grusse\"&collate=name").await;
    assert!(
        queried
            .as_array()
            .is_some_and(|items| items.iter().any(|item| read_id(item) == Some(id.as_str())))
    );
    delete_entity(&client, &id).await;
}

#[tokio::test]
async fn l10n_localized_entity_formats_date_and_currency() {
    let client = client();
    let id = unique_id("Offer-l10n-format");
    let entity = json!({
        "id": id,
        "type": "Offer",
        "date": {"type": "Property", "value": "2020-09-28T17:13:39Z"},
        "price": {"type": "Property", "value": 1234.5, "unitCode": "EUR"},
        "@context": [CORE_CONTEXT]
    });

    pretty_assertions::assert_eq!(create_entity(&client, &entity).await, StatusCode::CREATED);
    let response = get_localized_entity(&client, &id, "locale=de-DE&localizedAttrs=date,price", "application/json").await;
    pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.expect("localized body");
    pretty_assertions::assert_eq!(body["date"], "28.09.2020 17:13:39 UTC");
    pretty_assertions::assert_eq!(body["price"], "1.234,50 EUR");
    delete_entity(&client, &id).await;
}

#[tokio::test]
async fn l10n_localized_query_transforms_only_requested_attributes() {
    let client = client();
    let id = unique_id("Offer-l10n-query");
    let entity = json!({
        "id": id,
        "type": "Offer",
        "date": {"type": "Property", "value": "2020-09-28T17:13:39Z"},
        "name": {"type": "Property", "value": "Standard Offer"},
        "@context": [CORE_CONTEXT]
    });

    pretty_assertions::assert_eq!(create_entity(&client, &entity).await, StatusCode::CREATED);
    let response = query_localized_entities(&client, "type=Offer&locale=en-US&localizedAttrs=date").await;
    let results = response["results"].as_array().expect("results");
    let entry = results.iter().find(|item| read_id(item) == Some(id.as_str())).expect("entry");
    pretty_assertions::assert_eq!(entry["date"], "2020-09-28 05:13:39 PM UTC");
    pretty_assertions::assert_eq!(entry["name"]["value"], "Standard Offer");
    delete_entity(&client, &id).await;
}

#[tokio::test]
async fn l10n_localized_endpoint_rejects_ld_json_accept() {
    let client = client();
    let id = unique_id("Offer-l10n-accept");
    let entity = json!({
        "id": id,
        "type": "Offer",
        "date": {"type": "Property", "value": "2020-09-28T17:13:39Z"},
        "@context": [CORE_CONTEXT]
    });

    pretty_assertions::assert_eq!(create_entity(&client, &entity).await, StatusCode::CREATED);
    let response = get_localized_entity(&client, &id, "locale=en-US", "application/ld+json").await;
    pretty_assertions::assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    delete_entity(&client, &id).await;
}
