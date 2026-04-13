//! Language negotiation integration tests for the NGSI-LD context broker.
//!
//! Covers the `lang` query parameter (spec 6.3.5, 5.7.2.5), the
//! `NGSILD-Language` request header (broker extension), response body
//! structure after negotiation, edge cases (no match, @none, range matching,
//! zero quality, invalid input), and interactions with keyValues, batch query,
//! and mixed attribute types.
//!
//! Naming convention: `language_<area>_<scenario>_<expected_outcome>`.

// Test infrastructure legitimately uses expect/unwrap to fail fast on transport errors.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use uuid::Uuid;

const BASE_URL: &str = "http://127.0.0.1:1056/ngsi-ld/v1";
const DATABASE_URL: &str = "postgres://postgres:postgres@127.0.0.1:5432/postgres";
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";

const TRUNCATE_ALL: &str = "\
    TRUNCATE \
        entities, \
        jsonld_contexts, \
        temporal_entity_attributes, \
        attribute_history, \
        subscriptions, \
        context_source_registrations, \
        snapshots \
    CASCADE";

// ---------------------------------------------------------------------------
// Infrastructure
// ---------------------------------------------------------------------------

fn client() -> Client {
    Client::new()
}

fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

async fn clean_database() {
    let pool = PgPool::connect(DATABASE_URL).await.expect("connect test database");
    sqlx::query(TRUNCATE_ALL).execute(&pool).await.expect("truncate test tables");
    pool.close().await;
}

fn link_header() -> String {
    format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

async fn create_entity(client: &Client, body: &Value) -> StatusCode {
    client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create_entity request failed")
        .status()
}

async fn delete_entity(client: &Client, id: &str) -> StatusCode {
    client
        .delete(format!("{BASE_URL}/entities/{id}"))
        .send()
        .await
        .expect("delete_entity request failed")
        .status()
}

/// GET /entities/{id} with optional `lang` query parameter and optional
/// `NGSILD-Language` header. Always sends `Accept: application/ld+json` and
/// the Link context header.
async fn get_entity(client: &Client, id: &str, lang: Option<&str>, language_header: Option<&str>, format: Option<&str>) -> (StatusCode, Value) {
    let mut qs_parts: Vec<String> = Vec::new();
    if let Some(l) = lang {
        qs_parts.push(format!("lang={l}"));
    }
    if let Some(f) = format {
        qs_parts.push(format!("options={f}"));
    }
    let qs = if qs_parts.is_empty() {
        String::new()
    } else {
        format!("?{}", qs_parts.join("&"))
    };

    let mut req = client
        .get(format!("{BASE_URL}/entities/{id}{qs}"))
        .header("Accept", "application/ld+json")
        .header("Link", link_header());
    if let Some(lh) = language_header {
        req = req.header("NGSILD-Language", lh);
    }

    let resp = req.send().await.expect("get_entity request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

/// GET /entities with type filter and optional `lang` / `NGSILD-Language` /
/// `options` parameters.
async fn query_entities(
    client: &Client,
    type_filter: &str,
    id_filter: Option<&str>,
    lang: Option<&str>,
    language_header: Option<&str>,
    format: Option<&str>,
) -> (StatusCode, Value) {
    let mut qs_parts = vec![format!("type={type_filter}")];
    if let Some(i) = id_filter {
        qs_parts.push(format!("id={i}"));
    }
    if let Some(l) = lang {
        qs_parts.push(format!("lang={l}"));
    }
    if let Some(f) = format {
        qs_parts.push(format!("options={f}"));
    }

    let mut req = client
        .get(format!("{BASE_URL}/entities?{}", qs_parts.join("&")))
        .header("Accept", "application/ld+json")
        .header("Link", link_header());
    if let Some(lh) = language_header {
        req = req.header("NGSILD-Language", lh);
    }

    let resp = req.send().await.expect("query_entities request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

/// POST /entityOperations/query with optional `NGSILD-Language` header.
async fn batch_query(client: &Client, body: &Value, language_header: Option<&str>) -> (StatusCode, Value) {
    let mut req = client
        .post(format!("{BASE_URL}/entityOperations/query"))
        .header("Content-Type", "application/ld+json")
        .header("Accept", "application/ld+json")
        .json(body);
    if let Some(lh) = language_header {
        req = req.header("NGSILD-Language", lh);
    }

    let resp = req.send().await.expect("batch_query request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

// ---------------------------------------------------------------------------
// Shared fixture builders
// ---------------------------------------------------------------------------

/// Building entity with a LanguageProperty `street` containing `fr` and `nl`.
fn building_fr_nl(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "street": {
            "type": "LanguageProperty",
            "languageMap": {
                "fr": "Grand Place",
                "nl": "Grote Markt"
            }
        },
        "@context": [CORE_CONTEXT]
    })
}

// ---------------------------------------------------------------------------
// Retrieve entity: basic negotiation (spec 6.3.5, 5.7.2.5)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn language_retrieve_exact_match_returns_property_with_lang_tag() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-exact");

    pretty_assertions::assert_eq!(create_entity(&c, &building_fr_nl(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, Some("fr"), None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["street"]["type"], "Property", "LanguageProperty must be projected as Property");
    pretty_assertions::assert_eq!(body["street"]["value"], "Grand Place");
    pretty_assertions::assert_eq!(body["street"]["lang"], "fr");
    assert!(body["street"].get("languageMap").is_none(), "languageMap must not be present after negotiation");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn language_retrieve_wildcard_selects_first_tagged_language() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-wildcard");

    pretty_assertions::assert_eq!(create_entity(&c, &building_fr_nl(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, Some("*"), None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["street"]["type"], "Property");
    // Wildcard picks first tagged entry; the map has `fr` first.
    pretty_assertions::assert_eq!(body["street"]["value"], "Grand Place");
    pretty_assertions::assert_eq!(body["street"]["lang"], "fr");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn language_retrieve_ranked_preferences_select_highest_quality_match() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-ranked");

    pretty_assertions::assert_eq!(create_entity(&c, &building_fr_nl(&id)).await, StatusCode::CREATED);

    // nl;q=1.0 > fr;q=0.5, so nl must win.
    let (status, body) = get_entity(&c, &id, Some("fr;q=0.5,nl"), None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["street"]["value"], "Grote Markt");
    pretty_assertions::assert_eq!(body["street"]["lang"], "nl");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn language_retrieve_no_match_returns_language_property_unchanged() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-nomatch");

    pretty_assertions::assert_eq!(create_entity(&c, &building_fr_nl(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, Some("ja"), None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(
        body["street"]["type"],
        "LanguageProperty",
        "unmatched LanguageProperty must stay as LanguageProperty"
    );
    assert!(body["street"]["languageMap"].is_object(), "languageMap must still be present");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn language_retrieve_wildcard_falls_back_to_at_none() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-atnone");

    let entity = json!({
        "id": id,
        "type": "Building",
        "street": {
            "type": "LanguageProperty",
            "languageMap": {"@none": "Default Street"}
        },
        "@context": [CORE_CONTEXT]
    });
    pretty_assertions::assert_eq!(create_entity(&c, &entity).await, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, Some("*"), None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["street"]["type"], "Property");
    pretty_assertions::assert_eq!(body["street"]["value"], "Default Street");
    pretty_assertions::assert_eq!(body["street"]["lang"], "@none");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn language_retrieve_range_match_prefix_selects_dialect() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-range");

    let entity = json!({
        "id": id,
        "type": "Building",
        "street": {
            "type": "LanguageProperty",
            "languageMap": {
                "fr-CA": "Rue Principale",
                "de": "Hauptstrasse"
            }
        },
        "@context": [CORE_CONTEXT]
    });
    pretty_assertions::assert_eq!(create_entity(&c, &entity).await, StatusCode::CREATED);

    // `lang=fr` must match `fr-CA` via prefix range matching per BCP47.
    let (status, body) = get_entity(&c, &id, Some("fr"), None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["street"]["value"], "Rue Principale");
    pretty_assertions::assert_eq!(body["street"]["lang"], "fr-CA");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn language_retrieve_exact_match_preferred_over_range_match() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-exact-vs-range");

    let entity = json!({
        "id": id,
        "type": "Building",
        "street": {
            "type": "LanguageProperty",
            "languageMap": {
                "fr-CA": "Rue Principale",
                "fr": "Grand Place"
            }
        },
        "@context": [CORE_CONTEXT]
    });
    pretty_assertions::assert_eq!(create_entity(&c, &entity).await, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, Some("fr"), None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["street"]["value"], "Grand Place", "`fr` exact match must win over `fr-CA`");
    pretty_assertions::assert_eq!(body["street"]["lang"], "fr");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn language_retrieve_zero_quality_excludes_preference() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-q0");

    pretty_assertions::assert_eq!(create_entity(&c, &building_fr_nl(&id)).await, StatusCode::CREATED);

    // fr;q=0 means "do not want French"; nl must be selected.
    let (status, body) = get_entity(&c, &id, Some("fr;q=0,nl"), None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["street"]["value"], "Grote Markt");
    pretty_assertions::assert_eq!(body["street"]["lang"], "nl");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn language_retrieve_empty_lang_returns_400() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-empty");

    pretty_assertions::assert_eq!(create_entity(&c, &building_fr_nl(&id)).await, StatusCode::CREATED);

    let (status, _) = get_entity(&c, &id, Some(""), None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn language_retrieve_key_values_with_lang_returns_scalar_value() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-kv");

    pretty_assertions::assert_eq!(create_entity(&c, &building_fr_nl(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, Some("nl"), None, Some("keyValues")).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["street"], "Grote Markt", "keyValues must return the negotiated scalar");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn language_retrieve_key_values_without_lang_returns_language_map() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-kv-nofilter");

    pretty_assertions::assert_eq!(create_entity(&c, &building_fr_nl(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, None, None, Some("keyValues")).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(body["street"].is_object(), "without lang, keyValues must return the languageMap object");
    pretty_assertions::assert_eq!(body["street"]["fr"], "Grand Place");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn language_retrieve_no_lang_returns_original_language_property() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-noparam");

    pretty_assertions::assert_eq!(create_entity(&c, &building_fr_nl(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, None, None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["street"]["type"], "LanguageProperty");
    pretty_assertions::assert_eq!(body["street"]["languageMap"]["fr"], "Grand Place");
    pretty_assertions::assert_eq!(body["street"]["languageMap"]["nl"], "Grote Markt");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn language_retrieve_sub_attributes_preserved_after_negotiation() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-subattr");

    let entity = json!({
        "id": id,
        "type": "Building",
        "street": {
            "type": "LanguageProperty",
            "languageMap": {
                "fr": "Grand Place",
                "nl": "Grote Markt"
            },
            "source": {"type": "Property", "value": "OpenStreetMap"}
        },
        "@context": [CORE_CONTEXT]
    });
    pretty_assertions::assert_eq!(create_entity(&c, &entity).await, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, Some("fr"), None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["street"]["type"], "Property");
    pretty_assertions::assert_eq!(body["street"]["value"], "Grand Place");
    pretty_assertions::assert_eq!(body["street"]["lang"], "fr");
    pretty_assertions::assert_eq!(body["street"]["source"]["type"], "Property");
    pretty_assertions::assert_eq!(body["street"]["source"]["value"], "OpenStreetMap");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn language_retrieve_metadata_fields_preserved_after_negotiation() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-meta");

    let entity = json!({
        "id": id,
        "type": "Building",
        "street": {
            "type": "LanguageProperty",
            "languageMap": {"fr": "Grand Place"},
            "observedAt": "2024-01-01T12:00:00Z",
            "datasetId": "urn:ngsi-ld:Dataset:street-source"
        },
        "@context": [CORE_CONTEXT]
    });
    pretty_assertions::assert_eq!(create_entity(&c, &entity).await, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, Some("fr"), None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["street"]["type"], "Property");
    pretty_assertions::assert_eq!(body["street"]["value"], "Grand Place");
    pretty_assertions::assert_eq!(body["street"]["observedAt"], "2024-01-01T12:00:00Z");
    pretty_assertions::assert_eq!(body["street"]["datasetId"], "urn:ngsi-ld:Dataset:street-source");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn language_retrieve_mixed_types_only_language_property_negotiated() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-mixed");

    let entity = json!({
        "id": id,
        "type": "Building",
        "name": {"type": "Property", "value": "Eiffel Tower"},
        "street": {
            "type": "LanguageProperty",
            "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}
        },
        "locatedIn": {"type": "Relationship", "object": "urn:ngsi-ld:City:Paris"},
        "@context": [CORE_CONTEXT]
    });
    pretty_assertions::assert_eq!(create_entity(&c, &entity).await, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, Some("fr"), None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    // Plain Property unchanged.
    pretty_assertions::assert_eq!(body["name"]["type"], "Property");
    pretty_assertions::assert_eq!(body["name"]["value"], "Eiffel Tower");
    // LanguageProperty negotiated.
    pretty_assertions::assert_eq!(body["street"]["type"], "Property");
    pretty_assertions::assert_eq!(body["street"]["value"], "Grand Place");
    pretty_assertions::assert_eq!(body["street"]["lang"], "fr");
    // Relationship unchanged.
    pretty_assertions::assert_eq!(body["locatedIn"]["type"], "Relationship");
    pretty_assertions::assert_eq!(body["locatedIn"]["object"], "urn:ngsi-ld:City:Paris");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn language_retrieve_multiple_language_properties_negotiated_independently() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-multi-lp");

    let entity = json!({
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
    });
    pretty_assertions::assert_eq!(create_entity(&c, &entity).await, StatusCode::CREATED);

    // `nl` is available in both; each LanguageProperty must be negotiated independently.
    let (status, body) = get_entity(&c, &id, Some("nl"), None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["street"]["value"], "Grote Markt");
    pretty_assertions::assert_eq!(body["street"]["lang"], "nl");
    pretty_assertions::assert_eq!(body["description"]["value"], "Een beroemd plein");
    pretty_assertions::assert_eq!(body["description"]["lang"], "nl");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn language_retrieve_array_value_in_language_map_preserved() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-array");

    let entity = json!({
        "id": id,
        "type": "Building",
        "street": {
            "type": "LanguageProperty",
            "languageMap": {
                "fr": ["Grand Place", "Place Royale"],
                "nl": "Grote Markt"
            }
        },
        "@context": [CORE_CONTEXT]
    });
    pretty_assertions::assert_eq!(create_entity(&c, &entity).await, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, Some("fr"), None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["street"]["type"], "Property");

    let value = &body["street"]["value"];
    assert!(value.is_array(), "array languageMap values must be preserved as arrays");
    let arr = value.as_array().unwrap();
    pretty_assertions::assert_eq!(arr.len(), 2);
    pretty_assertions::assert_eq!(arr[0], "Grand Place");
    pretty_assertions::assert_eq!(arr[1], "Place Royale");

    delete_entity(&c, &id).await;
}

// ---------------------------------------------------------------------------
// NGSILD-Language header (broker extension)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn language_header_fallback_when_lang_query_absent() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-header");

    pretty_assertions::assert_eq!(create_entity(&c, &building_fr_nl(&id)).await, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, None, Some("nl"), None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["street"]["type"], "Property");
    pretty_assertions::assert_eq!(body["street"]["value"], "Grote Markt");
    pretty_assertions::assert_eq!(body["street"]["lang"], "nl");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn language_header_query_param_overrides_ngsild_language_header() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-precedence");

    pretty_assertions::assert_eq!(create_entity(&c, &building_fr_nl(&id)).await, StatusCode::CREATED);

    // `lang=fr` must override the `NGSILD-Language: nl` header.
    let (status, body) = get_entity(&c, &id, Some("fr"), Some("nl"), None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["street"]["value"], "Grand Place", "`lang` query param must override header");
    pretty_assertions::assert_eq!(body["street"]["lang"], "fr");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn language_header_ranked_preferences_in_ngsild_language_header() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-header-ranked");

    pretty_assertions::assert_eq!(create_entity(&c, &building_fr_nl(&id)).await, StatusCode::CREATED);

    // `fr` has quality 1.0, `nl` has 0.5; `fr` must win.
    let (status, body) = get_entity(&c, &id, None, Some("nl;q=0.5,fr"), None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["street"]["value"], "Grand Place", "fr has higher quality (1.0 vs 0.5)");
    pretty_assertions::assert_eq!(body["street"]["lang"], "fr");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn language_header_invalid_empty_value_returns_400() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-header-invalid");

    pretty_assertions::assert_eq!(create_entity(&c, &building_fr_nl(&id)).await, StatusCode::CREATED);

    let (status, _) = get_entity(&c, &id, None, Some(""), None).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);

    delete_entity(&c, &id).await;
}

// ---------------------------------------------------------------------------
// Query entities (GET /entities) with lang (spec 6.3.5)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn language_query_lang_param_applied_to_query_results() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-query");

    pretty_assertions::assert_eq!(create_entity(&c, &building_fr_nl(&id)).await, StatusCode::CREATED);

    let (status, body) = query_entities(&c, "Building", Some(&id), Some("nl"), None, None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("query result must be array");
    pretty_assertions::assert_eq!(arr.len(), 1);
    pretty_assertions::assert_eq!(arr[0]["street"]["type"], "Property");
    pretty_assertions::assert_eq!(arr[0]["street"]["value"], "Grote Markt");
    pretty_assertions::assert_eq!(arr[0]["street"]["lang"], "nl");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn language_query_ngsild_language_header_applied_to_query_results() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-query-header");

    pretty_assertions::assert_eq!(create_entity(&c, &building_fr_nl(&id)).await, StatusCode::CREATED);

    let (status, body) = query_entities(&c, "Building", Some(&id), None, Some("fr"), None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("query result must be array");
    pretty_assertions::assert_eq!(arr.len(), 1);
    pretty_assertions::assert_eq!(arr[0]["street"]["value"], "Grand Place");
    pretty_assertions::assert_eq!(arr[0]["street"]["lang"], "fr");

    delete_entity(&c, &id).await;
}

#[tokio::test]
#[serial]
async fn language_query_key_values_with_lang_returns_scalar_value() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-query-kv");

    pretty_assertions::assert_eq!(create_entity(&c, &building_fr_nl(&id)).await, StatusCode::CREATED);

    let (status, body) = query_entities(&c, "Building", Some(&id), Some("nl"), None, Some("keyValues")).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("query result must be array");
    pretty_assertions::assert_eq!(arr.len(), 1);
    pretty_assertions::assert_eq!(arr[0]["street"], "Grote Markt", "keyValues with lang must return negotiated scalar");

    delete_entity(&c, &id).await;
}

// ---------------------------------------------------------------------------
// Batch query (POST /entityOperations/query) with lang (spec 6.3.5)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn language_batch_query_ngsild_language_header_applied_to_results() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:lang-batch");

    pretty_assertions::assert_eq!(create_entity(&c, &building_fr_nl(&id)).await, StatusCode::CREATED);

    let query_body = json!({
        "type": "Query",
        "entities": [{"type": "Building"}],
        "@context": [CORE_CONTEXT]
    });
    let (status, body) = batch_query(&c, &query_body, Some("nl")).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let arr = body.as_array().expect("batch query result must be array");
    let our_entity = arr
        .iter()
        .find(|e| e["id"].as_str() == Some(&id))
        .expect("created entity must appear in batch query results");
    pretty_assertions::assert_eq!(our_entity["street"]["type"], "Property");
    pretty_assertions::assert_eq!(our_entity["street"]["value"], "Grote Markt");
    pretty_assertions::assert_eq!(our_entity["street"]["lang"], "nl");

    delete_entity(&c, &id).await;
}
