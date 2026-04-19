//! Language negotiation integration tests (NGSI-LD 6.3.5, 5.7.2.5).
//!
//! Naming convention: `language_<area>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use andromeda_test_common::prelude::*;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

async fn create(client: &NgsiLdClient, entity: &Value) {
    let response = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED);
}

async fn get_entity(client: &NgsiLdClient, id: &str, lang: Option<&str>, language_header: Option<&str>, format: Option<&str>) -> (StatusCode, Value) {
    let mut parts: Vec<String> = Vec::new();
    if let Some(l) = lang {
        parts.push(format!("lang={l}"));
    }
    if let Some(f) = format {
        parts.push(format!("options={f}"));
    }
    let qs = if parts.is_empty() { String::new() } else { format!("?{}", parts.join("&")) };
    let path = format!("/ngsi-ld/v1/entities/{}{qs}", urlencoding::encode(id));
    let mut request = client
        .raw(Method::GET, &path)
        .header("Accept", "application/ld+json")
        .header("Link", &jsonld_link_header(CORE_CONTEXT));
    if let Some(header) = language_header {
        request = request.header("NGSILD-Language", header);
    }
    let response = request.send().await.expect("get_entity failed");
    (response.status, response.body)
}

async fn query_entities(
    client: &NgsiLdClient,
    type_filter: &str,
    id_filter: Option<&str>,
    lang: Option<&str>,
    language_header: Option<&str>,
    format: Option<&str>,
) -> (StatusCode, Value) {
    let mut parts = vec![format!("type={type_filter}")];
    if let Some(i) = id_filter {
        parts.push(format!("id={}", urlencoding::encode(i)));
    }
    if let Some(l) = lang {
        parts.push(format!("lang={l}"));
    }
    if let Some(f) = format {
        parts.push(format!("options={f}"));
    }
    let path = format!("/ngsi-ld/v1/entities?{}", parts.join("&"));
    let mut request = client
        .raw(Method::GET, &path)
        .header("Accept", "application/ld+json")
        .header("Link", &jsonld_link_header(CORE_CONTEXT));
    if let Some(header) = language_header {
        request = request.header("NGSILD-Language", header);
    }
    let response = request.send().await.expect("query_entities failed");
    (response.status, response.body)
}

async fn batch_query(client: &NgsiLdClient, body: &Value, language_header: Option<&str>) -> (StatusCode, Value) {
    let mut request = client
        .raw(Method::POST, "/ngsi-ld/v1/entityOperations/query")
        .header("Content-Type", "application/ld+json")
        .header("Accept", "application/ld+json")
        .json(body);
    if let Some(header) = language_header {
        request = request.header("NGSILD-Language", header);
    }
    let response = request.send().await.expect("batch_query failed");
    (response.status, response.body)
}

fn building_fr_nl(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "street": {
            "type": "LanguageProperty",
            "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}
        },
        "@context": [CORE_CONTEXT]
    })
}

// --- Retrieve entity: basic negotiation (spec 6.3.5, 5.7.2.5) ---

#[tokio::test]
async fn language_retrieve_exact_match_returns_property_with_lang_tag() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-exact");
    create(&client, &building_fr_nl(&id)).await;
    let (status, body) = get_entity(&client, &id, Some("fr"), None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["street"]["type"], "Property");
    assert_eq!(body["street"]["value"], "Grand Place");
    assert_eq!(body["street"]["lang"], "fr");
    assert!(body["street"].get("languageMap").is_none());
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn language_retrieve_wildcard_selects_first_tagged_language() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-wildcard");
    create(&client, &building_fr_nl(&id)).await;
    let (status, body) = get_entity(&client, &id, Some("*"), None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["street"]["value"], "Grand Place");
    assert_eq!(body["street"]["lang"], "fr");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn language_retrieve_ranked_preferences_select_highest_quality_match() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-ranked");
    create(&client, &building_fr_nl(&id)).await;
    let (status, body) = get_entity(&client, &id, Some("fr;q=0.5,nl"), None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["street"]["value"], "Grote Markt");
    assert_eq!(body["street"]["lang"], "nl");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn language_retrieve_no_match_returns_language_property_unchanged() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-nomatch");
    create(&client, &building_fr_nl(&id)).await;
    let (status, body) = get_entity(&client, &id, Some("ja"), None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["street"]["type"], "LanguageProperty");
    assert!(body["street"]["languageMap"].is_object());
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn language_retrieve_wildcard_falls_back_to_at_none() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-atnone");
    create(
        &client,
        &json!({
            "id": id,
            "type": "Building",
            "street": {"type": "LanguageProperty", "languageMap": {"@none": "Default Street"}},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;
    let (status, body) = get_entity(&client, &id, Some("*"), None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["street"]["type"], "Property");
    assert_eq!(body["street"]["value"], "Default Street");
    assert_eq!(body["street"]["lang"], "@none");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn language_retrieve_range_match_prefix_selects_dialect() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-range");
    create(
        &client,
        &json!({
            "id": id,
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {"fr-CA": "Rue Principale", "de": "Hauptstrasse"}
            },
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;
    let (status, body) = get_entity(&client, &id, Some("fr"), None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["street"]["value"], "Rue Principale");
    assert_eq!(body["street"]["lang"], "fr-CA");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn language_retrieve_exact_match_preferred_over_range_match() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-exact-vs-range");
    create(
        &client,
        &json!({
            "id": id,
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {"fr-CA": "Rue Principale", "fr": "Grand Place"}
            },
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;
    let (status, body) = get_entity(&client, &id, Some("fr"), None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["street"]["value"], "Grand Place");
    assert_eq!(body["street"]["lang"], "fr");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn language_retrieve_zero_quality_excludes_preference() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-q0");
    create(&client, &building_fr_nl(&id)).await;
    let (status, body) = get_entity(&client, &id, Some("fr;q=0,nl"), None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["street"]["value"], "Grote Markt");
    assert_eq!(body["street"]["lang"], "nl");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn language_retrieve_empty_lang_returns_400() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-empty");
    create(&client, &building_fr_nl(&id)).await;
    let (status, _) = get_entity(&client, &id, Some(""), None, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn language_retrieve_key_values_with_lang_returns_scalar_value() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-kv");
    create(&client, &building_fr_nl(&id)).await;
    let (status, body) = get_entity(&client, &id, Some("nl"), None, Some("keyValues")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["street"], "Grote Markt");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn language_retrieve_key_values_without_lang_returns_language_map() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-kv-nofilter");
    create(&client, &building_fr_nl(&id)).await;
    let (status, body) = get_entity(&client, &id, None, None, Some("keyValues")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["street"].is_object());
    assert_eq!(body["street"]["fr"], "Grand Place");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn language_retrieve_no_lang_returns_original_language_property() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-noparam");
    create(&client, &building_fr_nl(&id)).await;
    let (status, body) = get_entity(&client, &id, None, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["street"]["type"], "LanguageProperty");
    assert_eq!(body["street"]["languageMap"]["fr"], "Grand Place");
    assert_eq!(body["street"]["languageMap"]["nl"], "Grote Markt");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn language_retrieve_sub_attributes_preserved_after_negotiation() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-subattr");
    create(
        &client,
        &json!({
            "id": id,
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"},
                "source": {"type": "Property", "value": "OpenStreetMap"}
            },
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;
    let (status, body) = get_entity(&client, &id, Some("fr"), None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["street"]["type"], "Property");
    assert_eq!(body["street"]["value"], "Grand Place");
    assert_eq!(body["street"]["lang"], "fr");
    assert_eq!(body["street"]["source"]["type"], "Property");
    assert_eq!(body["street"]["source"]["value"], "OpenStreetMap");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn language_retrieve_metadata_fields_preserved_after_negotiation() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-meta");
    create(
        &client,
        &json!({
            "id": id,
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {"fr": "Grand Place"},
                "observedAt": "2024-01-01T12:00:00Z",
                "datasetId": "urn:ngsi-ld:Dataset:street-source"
            },
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;
    let (status, body) = get_entity(&client, &id, Some("fr"), None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["street"]["type"], "Property");
    assert_eq!(body["street"]["value"], "Grand Place");
    assert_eq!(body["street"]["observedAt"], "2024-01-01T12:00:00Z");
    assert_eq!(body["street"]["datasetId"], "urn:ngsi-ld:Dataset:street-source");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn language_retrieve_mixed_types_only_language_property_negotiated() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-mixed");
    create(
        &client,
        &json!({
            "id": id,
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "street": {
                "type": "LanguageProperty",
                "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}
            },
            "locatedIn": {"type": "Relationship", "object": "urn:ngsi-ld:City:Paris"},
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;
    let (status, body) = get_entity(&client, &id, Some("fr"), None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"]["type"], "Property");
    assert_eq!(body["name"]["value"], "Eiffel Tower");
    assert_eq!(body["street"]["type"], "Property");
    assert_eq!(body["street"]["value"], "Grand Place");
    assert_eq!(body["street"]["lang"], "fr");
    assert_eq!(body["locatedIn"]["type"], "Relationship");
    assert_eq!(body["locatedIn"]["object"], "urn:ngsi-ld:City:Paris");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn language_retrieve_multiple_language_properties_negotiated_independently() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-multi-lp");
    create(
        &client,
        &json!({
            "id": id,
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}
            },
            "description": {
                "type": "LanguageProperty",
                "languageMap": {"en": "A famous square", "nl": "Een beroemd plein"}
            },
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;
    let (status, body) = get_entity(&client, &id, Some("nl"), None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["street"]["value"], "Grote Markt");
    assert_eq!(body["street"]["lang"], "nl");
    assert_eq!(body["description"]["value"], "Een beroemd plein");
    assert_eq!(body["description"]["lang"], "nl");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn language_retrieve_array_value_in_language_map_preserved() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-array");
    create(
        &client,
        &json!({
            "id": id,
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {"fr": ["Grand Place", "Place Royale"], "nl": "Grote Markt"}
            },
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;
    let (status, body) = get_entity(&client, &id, Some("fr"), None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["street"]["type"], "Property");
    let value = &body["street"]["value"];
    assert!(value.is_array());
    let arr = value.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0], "Grand Place");
    assert_eq!(arr[1], "Place Royale");
    scope.cleanup().await.expect("cleanup failed");
}

// --- NGSILD-Language header ---

#[tokio::test]
async fn language_header_fallback_when_lang_query_absent() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-header");
    create(&client, &building_fr_nl(&id)).await;
    let (status, body) = get_entity(&client, &id, None, Some("nl"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["street"]["type"], "Property");
    assert_eq!(body["street"]["value"], "Grote Markt");
    assert_eq!(body["street"]["lang"], "nl");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn language_header_query_param_overrides_ngsild_language_header() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-precedence");
    create(&client, &building_fr_nl(&id)).await;
    let (status, body) = get_entity(&client, &id, Some("fr"), Some("nl"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["street"]["value"], "Grand Place");
    assert_eq!(body["street"]["lang"], "fr");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn language_header_ranked_preferences_in_ngsild_language_header() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-header-ranked");
    create(&client, &building_fr_nl(&id)).await;
    let (status, body) = get_entity(&client, &id, None, Some("nl;q=0.5,fr"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["street"]["value"], "Grand Place");
    assert_eq!(body["street"]["lang"], "fr");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn language_header_invalid_empty_value_returns_400() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-header-invalid");
    create(&client, &building_fr_nl(&id)).await;
    let (status, _) = get_entity(&client, &id, None, Some(""), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    scope.cleanup().await.expect("cleanup failed");
}

// --- Query entities ---

#[tokio::test]
async fn language_query_lang_param_applied_to_query_results() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-query");
    create(&client, &building_fr_nl(&id)).await;
    let (status, body) = query_entities(&client, "Building", Some(&id), Some("nl"), None, None).await;
    assert_eq!(status, StatusCode::OK);
    let arr = body.as_array().expect("query result must be array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["street"]["type"], "Property");
    assert_eq!(arr[0]["street"]["value"], "Grote Markt");
    assert_eq!(arr[0]["street"]["lang"], "nl");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn language_query_ngsild_language_header_applied_to_query_results() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-query-header");
    create(&client, &building_fr_nl(&id)).await;
    let (status, body) = query_entities(&client, "Building", Some(&id), None, Some("fr"), None).await;
    assert_eq!(status, StatusCode::OK);
    let arr = body.as_array().expect("query result must be array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["street"]["value"], "Grand Place");
    assert_eq!(arr[0]["street"]["lang"], "fr");
    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
async fn language_query_key_values_with_lang_returns_scalar_value() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-query-kv");
    create(&client, &building_fr_nl(&id)).await;
    let (status, body) = query_entities(&client, "Building", Some(&id), Some("nl"), None, Some("keyValues")).await;
    assert_eq!(status, StatusCode::OK);
    let arr = body.as_array().expect("query result must be array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["street"], "Grote Markt");
    scope.cleanup().await.expect("cleanup failed");
}

// --- Batch query ---

#[tokio::test]
async fn language_batch_query_ngsild_language_header_applied_to_results() {
    let scope = TestScope::new("language");
    let client = NgsiLdClient::new(&scope).await;
    let id = scope.id("Building:lang-batch");
    create(&client, &building_fr_nl(&id)).await;

    let query_body = json!({
        "type": "Query",
        "entities": [{"type": "Building", "id": id}],
        "@context": [CORE_CONTEXT]
    });
    let (status, body) = batch_query(&client, &query_body, Some("nl")).await;
    assert_eq!(status, StatusCode::OK);
    let arr = body.as_array().expect("batch query result must be array");
    let ours = arr
        .iter()
        .find(|e| e["id"].as_str() == Some(&id))
        .expect("created entity must appear in batch query results");
    assert_eq!(ours["street"]["type"], "Property");
    assert_eq!(ours["street"]["value"], "Grote Markt");
    assert_eq!(ours["street"]["lang"], "nl");
    scope.cleanup().await.expect("cleanup failed");
}
