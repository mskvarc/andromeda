//! i18n and l10n integration tests.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use andromeda_test_common::prelude::*;
use pretty_assertions::assert_eq;
use reqwest::{Method, Response, StatusCode};
use serde_json::{Value, json};

const IN_LANGUAGE_IRI: &str = "https://schema.org/inLanguage";
const SAME_AS_IRI: &str = "https://schema.org/sameAs";
const BOOKING_URL_IRI: &str = "https://uri.etsi.org/ngsi-ld/default-context/bookingUrl";

async fn create(client: &NgsiLdClient, entity: &Value) {
    let response = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED);
}

async fn fetch_json(client: &NgsiLdClient, id: &str) -> Value {
    let response = client.get_entity(id, "", "application/json", None).await.expect("get_entity failed");
    assert_eq!(response.status, StatusCode::OK);
    response.body
}

async fn query_json(client: &NgsiLdClient, query: &str) -> Value {
    let response = client.query_entities(query, "application/json", None).await.expect("query_entities failed");
    assert_eq!(response.status, StatusCode::OK);
    response.body
}

async fn get_localized_raw(client: &NgsiLdClient, id: &str, query: &str, accept: &str) -> Response {
    let path = format!("/ngsi-ld/v1/i18n/entities/{}?{query}", urlencoding::encode(id));
    // Use a fresh reqwest request through the scope's client so the tenant
    // header is applied consistently.
    let url = format!("{}{path}", BASE_URL.trim_end_matches("/ngsi-ld/v1"));
    client
        .http()
        .get(url)
        .header("Accept", accept)
        .send()
        .await
        .expect("localized get request failed")
}

async fn query_localized(client: &NgsiLdClient, query: &str) -> Value {
    let response = client
        .raw(Method::GET, &format!("/ngsi-ld/v1/i18n/entities?{query}"))
        .header("Accept", "application/json")
        .send()
        .await
        .expect("localized query request failed");
    response.body
}

fn read_id(value: &Value) -> Option<&str> {
    value.get("id").and_then(Value::as_str).or_else(|| value.get("@id").and_then(Value::as_str))
}

fn property_value<'a>(value: &'a Value, compact: &str, expanded: &str) -> Option<&'a Value> {
    value.get(compact).or_else(|| value.get(expanded)).and_then(|property| property.get("value"))
}

#[tokio::test]
async fn i18n_entity_language_association_round_trips() {
    let scope = TestScope::new("i18n");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Event:language");
    let entity = json!({
        "id": id,
        "type": "Event",
        "name": {"type": "Property", "value": "Bonjour le Monde"},
        "description": {"type": "Property", "value": "Simple text"},
        "inLanguage": {"type": "Property", "value": "fr"},
        "@context": [{"inLanguage": "https://schema.org/inLanguage"}, CORE_CONTEXT]
    });
    create(&client, &entity).await;
    let retrieved = fetch_json(&client, &id).await;
    assert_eq!(property_value(&retrieved, "inLanguage", IN_LANGUAGE_IRI), Some(&json!("fr")));
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn i18n_property_language_metadata_is_queryable() {
    let scope = TestScope::new("i18n");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Hotel:prop-lang");
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
    create(&client, &entity).await;
    let q = format!("\"{BOOKING_URL_IRI}\".\"{IN_LANGUAGE_IRI}\"==\"de\"");
    let query = format!("type=Hotel&q={}", urlencoding::encode(&q));
    let queried = query_json(&client, &query).await;
    assert!(
        queried
            .as_array()
            .is_some_and(|items| items.iter().any(|item| read_id(item) == Some(id.as_str())))
    );
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn i18n_same_as_relationship_language_metadata_is_queryable() {
    let scope = TestScope::new("i18n");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Event:same-as");
    let entity = json!({
        "id": id,
        "type": "Event",
        "sameAs": [{
            "type": "Relationship",
            "datasetId": "urn:ngsi-ld:Relationship:1",
            "object": "urn:ngsi-ld:Event:helloWorld",
            "inLanguage": {"type": "Property", "value": "en"}
        }],
        "@context": [
            {"sameAs": "https://schema.org/sameAs", "inLanguage": "https://schema.org/inLanguage"},
            CORE_CONTEXT
        ]
    });
    create(&client, &entity).await;
    let q = format!("\"{SAME_AS_IRI}\".\"{IN_LANGUAGE_IRI}\"==\"en\"");
    let query = format!("type=Event&q={}", urlencoding::encode(&q));
    let queried = query_json(&client, &query).await;
    assert!(
        queried
            .as_array()
            .is_some_and(|items| items.iter().any(|item| read_id(item) == Some(id.as_str())))
    );
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn i18n_collate_metadata_is_generated_for_string_properties() {
    let scope = TestScope::new("i18n");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:collate");
    create(
        &client,
        &json!({
            "id": id,
            "type": "Building",
            "name": {"type": "Property", "value": "Schone Grusse"},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let retrieved = fetch_json(&client, &id).await;
    let collate_value = retrieved
        .get("name")
        .and_then(|name| name.get("collate"))
        .and_then(|collate| collate.get("value"));
    assert_eq!(collate_value, Some(&json!("schone grusse")));
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn i18n_collate_query_matches_normalized_rhs() {
    let scope = TestScope::new("i18n");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:collate-query");
    create(
        &client,
        &json!({
            "id": id,
            "type": "Building",
            "name": {"type": "Property", "value": "Schone Grusse"},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;
    let queried = query_json(&client, "type=Building&q=name==\"Schone Grusse\"&collate=name").await;
    assert!(
        queried
            .as_array()
            .is_some_and(|items| items.iter().any(|item| read_id(item) == Some(id.as_str())))
    );
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn l10n_localized_entity_formats_date_and_currency() {
    let scope = TestScope::new("i18n");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Offer:l10n-format");
    create(
        &client,
        &json!({
            "id": id,
            "type": "Offer",
            "date": {"type": "Property", "value": "2020-09-28T17:13:39Z"},
            "price": {"type": "Property", "value": 1234.5, "unitCode": "EUR"},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let response = get_localized_raw(&client, &id, "locale=de-DE&localizedAttrs=date,price", "application/json").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.expect("localized body");
    assert_eq!(body["date"], "28.09.2020 17:13:39 UTC");
    assert_eq!(body["price"], "1.234,50 EUR");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn l10n_localized_query_transforms_only_requested_attributes() {
    let scope = TestScope::new("i18n");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Offer:l10n-query");
    create(
        &client,
        &json!({
            "id": id,
            "type": "Offer",
            "date": {"type": "Property", "value": "2020-09-28T17:13:39Z"},
            "name": {"type": "Property", "value": "Standard Offer"},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;

    let body = query_localized(&client, "type=Offer&locale=en-US&localizedAttrs=date").await;
    let results = body["results"].as_array().expect("results");
    let entry = results.iter().find(|item| read_id(item) == Some(id.as_str())).expect("entry");
    assert_eq!(entry["date"], "2020-09-28 05:13:39 PM UTC");
    assert_eq!(entry["name"]["value"], "Standard Offer");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn l10n_localized_endpoint_rejects_ld_json_accept() {
    let scope = TestScope::new("i18n");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Offer:l10n-accept");
    create(
        &client,
        &json!({
            "id": id,
            "type": "Offer",
            "date": {"type": "Property", "value": "2020-09-28T17:13:39Z"},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;
    let response = get_localized_raw(&client, &id, "locale=en-US", "application/ld+json").await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    scope.cleanup().await.expect("cleanup failed");
}
