/// Language negotiation integration tests.
///
/// Covers the NGSI-LD `lang` query parameter (6.3.5, 5.7.2.5), the custom
/// `NGSILD-Language` header (broker extension), response body structure after
/// negotiation, edge cases (no match, @none, range matching, zero quality,
/// invalid input), and interactions with other features (keyValues,
/// sub-attributes, multiple attributes, batch query).
use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

const PREFIX: &str = "spec_features::lang";
const CTX: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";

fn ctx_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

/// Create a building entity with a LanguageProperty `street` containing `fr` and `nl`.
fn building_fr_nl(id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "type": "Building",
        "street": {
            "type": "LanguageProperty",
            "languageMap": {
                "fr": "Grand Place",
                "nl": "Grote Markt"
            }
        },
        "@context": [CTX]
    })
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── Retrieve entity: basic negotiation ─────────────────────────

    // Exact language match returns Property with correct value and lang tag.
    trials.push(trial(
        &format!("{PREFIX}::retrieve::exact_match_returns_property_with_lang_tag"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:lang-exact");
            let resp = ctx.client.create_entity(&building_fr_nl(&id)).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let params = QueryParams::new().lang("fr");
            let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);

            let body: serde_json::Value = resp.json();
            assert_eq!(body["street"]["type"], "Property", "LanguageProperty must be projected as Property");
            assert_eq!(body["street"]["value"], "Grand Place");
            assert_eq!(body["street"]["lang"], "fr");
            assert!(body["street"].get("languageMap").is_none(), "languageMap must not be present after negotiation");
        },
    ));

    // Wildcard selects first available tagged language.
    trials.push(trial(
        &format!("{PREFIX}::retrieve::wildcard_selects_first_tagged_language"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:lang-wildcard");
            let resp = ctx.client.create_entity(&building_fr_nl(&id)).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let params = QueryParams::new().lang("*");
            let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);

            let body: serde_json::Value = resp.json();
            assert_eq!(body["street"]["type"], "Property");
            // Wildcard picks first tagged entry; the map has `fr` first
            assert_eq!(body["street"]["value"], "Grand Place");
            assert_eq!(body["street"]["lang"], "fr");
        },
    ));

    // Ranked preferences select the highest-quality available match.
    trials.push(trial(
        &format!("{PREFIX}::retrieve::ranked_preferences_select_highest_quality"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:lang-ranked");
            let resp = ctx.client.create_entity(&building_fr_nl(&id)).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            // nl;q=1.0 > fr;q=0.5, so nl should win
            let params = QueryParams::new().lang("fr;q=0.5,nl");
            let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);

            let body: serde_json::Value = resp.json();
            assert_eq!(body["street"]["value"], "Grote Markt");
            assert_eq!(body["street"]["lang"], "nl");
        },
    ));

    // ── No match: LanguageProperty returned unmodified ─────────────

    // When no requested language matches, the LanguageProperty is returned as-is.
    trials.push(trial(
        &format!("{PREFIX}::retrieve::no_match_returns_language_property_unchanged"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:lang-nomatch");
            let resp = ctx.client.create_entity(&building_fr_nl(&id)).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let params = QueryParams::new().lang("ja");
            let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);

            let body: serde_json::Value = resp.json();
            assert_eq!(
                body["street"]["type"], "LanguageProperty",
                "unmatched LanguageProperty stays as LanguageProperty"
            );
            assert!(body["street"]["languageMap"].is_object(), "languageMap must still be present");
        },
    ));

    // ── @none (default language) ───────────────────────────────────

    // Wildcard falls back to @none when no tagged language exists.
    trials.push(trial(&format!("{PREFIX}::retrieve::wildcard_falls_back_to_at_none"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:lang-atnone");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {
                    "@none": "Default Street"
                }
            },
            "@context": [CTX]
        });
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().lang("*");
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["street"]["type"], "Property");
        assert_eq!(body["street"]["value"], "Default Street");
        assert_eq!(body["street"]["lang"], "@none");
    }));

    // ── Language range matching ────────────────────────────────────

    // `lang=fr` matches `fr-CA` via prefix range matching per BCP47.
    trials.push(trial(&format!("{PREFIX}::retrieve::range_match_prefix"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:lang-range");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {
                    "fr-CA": "Rue Principale",
                    "de": "Hauptstrasse"
                }
            },
            "@context": [CTX]
        });
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().lang("fr");
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["street"]["value"], "Rue Principale");
        assert_eq!(body["street"]["lang"], "fr-CA");
    }));

    // Exact match wins over range match: `lang=fr` prefers `fr` over `fr-CA`.
    trials.push(trial(&format!("{PREFIX}::retrieve::exact_match_preferred_over_range"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:lang-exact-vs-range");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {
                    "fr-CA": "Rue Principale",
                    "fr": "Grand Place"
                }
            },
            "@context": [CTX]
        });
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().lang("fr");
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["street"]["value"], "Grand Place");
        assert_eq!(body["street"]["lang"], "fr");
    }));

    // ── Zero quality exclusion ─────────────────────────────────────

    // Preference with q=0 is excluded; next matching preference used.
    trials.push(trial(&format!("{PREFIX}::retrieve::zero_quality_excludes_preference"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:lang-q0");
        let resp = ctx.client.create_entity(&building_fr_nl(&id)).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // fr;q=0 means "do not want French"; nl should be selected
        let params = QueryParams::new().lang("fr;q=0,nl");
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["street"]["value"], "Grote Markt");
        assert_eq!(body["street"]["lang"], "nl");
    }));

    // ── Invalid lang parameter ─────────────────────────────────────

    // Empty lang parameter returns 400 BadRequestData.
    trials.push(trial(&format!("{PREFIX}::retrieve::empty_lang_returns_400"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:lang-empty");
        let resp = ctx.client.create_entity(&building_fr_nl(&id)).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().lang("");
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // ── Simplified (keyValues) representation ──────────────────────

    // keyValues with lang negotiation returns the scalar value directly.
    trials.push(trial(&format!("{PREFIX}::retrieve::key_values_returns_scalar_value"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:lang-kv");
        let resp = ctx.client.create_entity(&building_fr_nl(&id)).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().lang("nl").format("keyValues");
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["street"], "Grote Markt", "keyValues should return the negotiated scalar");
    }));

    // keyValues without lang returns the languageMap directly.
    trials.push(trial(
        &format!("{PREFIX}::retrieve::key_values_without_lang_returns_language_map"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:lang-kv-nofilter");
            let resp = ctx.client.create_entity(&building_fr_nl(&id)).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let params = QueryParams::new().format("keyValues");
            let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);

            let body: serde_json::Value = resp.json();
            assert!(body["street"].is_object(), "without lang, keyValues returns the languageMap object");
            assert_eq!(body["street"]["fr"], "Grand Place");
        },
    ));

    // ── Without lang: LanguageProperty is unchanged ────────────────

    // No lang parameter returns LanguageProperty in original form.
    trials.push(trial(
        &format!("{PREFIX}::retrieve::no_lang_returns_original_language_property"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:lang-noparam");
            let resp = ctx.client.create_entity(&building_fr_nl(&id)).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let resp = ctx.client.get_entity(&id, &QueryParams::new(), ctx_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);

            let body: serde_json::Value = resp.json();
            assert_eq!(body["street"]["type"], "LanguageProperty");
            assert_eq!(body["street"]["languageMap"]["fr"], "Grand Place");
            assert_eq!(body["street"]["languageMap"]["nl"], "Grote Markt");
        },
    ));

    // ── Sub-attributes preserved ───────────────────────────────────

    // Negotiated response preserves sub-attributes from the original LanguageProperty.
    trials.push(trial(
        &format!("{PREFIX}::retrieve::sub_attributes_preserved_after_negotiation"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:lang-subattr");
            let entity = serde_json::json!({
                "id": id,
                "type": "Building",
                "street": {
                    "type": "LanguageProperty",
                    "languageMap": {
                        "fr": "Grand Place",
                        "nl": "Grote Markt"
                    },
                    "source": {
                        "type": "Property",
                        "value": "OpenStreetMap"
                    }
                },
                "@context": [CTX]
            });
            let resp = ctx.client.create_entity(&entity).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let params = QueryParams::new().lang("fr");
            let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);

            let body: serde_json::Value = resp.json();
            assert_eq!(body["street"]["type"], "Property");
            assert_eq!(body["street"]["value"], "Grand Place");
            assert_eq!(body["street"]["lang"], "fr");
            assert_eq!(body["street"]["source"]["type"], "Property");
            assert_eq!(body["street"]["source"]["value"], "OpenStreetMap");
        },
    ));

    // ── observedAt and datasetId preserved ─────────────────────────

    // Metadata fields are carried through negotiation.
    trials.push(trial(&format!("{PREFIX}::retrieve::observed_at_and_dataset_id_preserved"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:lang-meta");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {
                    "fr": "Grand Place"
                },
                "observedAt": "2024-01-01T12:00:00Z",
                "datasetId": "urn:ngsi-ld:Dataset:street-source"
            },
            "@context": [CTX]
        });
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().lang("fr");
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["street"]["type"], "Property");
        assert_eq!(body["street"]["value"], "Grand Place");
        assert_eq!(body["street"]["observedAt"], "2024-01-01T12:00:00Z");
        assert_eq!(body["street"]["datasetId"], "urn:ngsi-ld:Dataset:street-source");
    }));

    // ── Mixed attribute types ──────────────────────────────────────

    // Only LanguageProperty attributes are affected; other types pass through.
    trials.push(trial(
        &format!("{PREFIX}::retrieve::mixed_types_only_language_property_negotiated"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:lang-mixed");
            let entity = serde_json::json!({
                "id": id,
                "type": "Building",
                "name": {
                    "type": "Property",
                    "value": "Eiffel Tower"
                },
                "street": {
                    "type": "LanguageProperty",
                    "languageMap": {
                        "fr": "Grand Place",
                        "nl": "Grote Markt"
                    }
                },
                "locatedIn": {
                    "type": "Relationship",
                    "object": "urn:ngsi-ld:City:Paris"
                },
                "@context": [CTX]
            });
            let resp = ctx.client.create_entity(&entity).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let params = QueryParams::new().lang("fr");
            let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);

            let body: serde_json::Value = resp.json();
            // Property unchanged
            assert_eq!(body["name"]["type"], "Property");
            assert_eq!(body["name"]["value"], "Eiffel Tower");
            // LanguageProperty negotiated
            assert_eq!(body["street"]["type"], "Property");
            assert_eq!(body["street"]["value"], "Grand Place");
            assert_eq!(body["street"]["lang"], "fr");
            // Relationship unchanged
            assert_eq!(body["locatedIn"]["type"], "Relationship");
            assert_eq!(body["locatedIn"]["object"], "urn:ngsi-ld:City:Paris");
        },
    ));

    // ── Multiple LanguageProperty attributes ───────────────────────

    // All LanguageProperty attributes in an entity are negotiated independently.
    trials.push(trial(
        &format!("{PREFIX}::retrieve::multiple_language_properties_negotiated_independently"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:lang-multi-lp");
            let entity = serde_json::json!({
                "id": id,
                "type": "Building",
                "street": {
                    "type": "LanguageProperty",
                    "languageMap": {
                        "fr": "Grand Place",
                        "nl": "Grote Markt"
                    }
                },
                "description": {
                    "type": "LanguageProperty",
                    "languageMap": {
                        "en": "A famous square",
                        "nl": "Een beroemd plein"
                    }
                },
                "@context": [CTX]
            });
            let resp = ctx.client.create_entity(&entity).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            // nl is available in both; fr only in street
            let params = QueryParams::new().lang("nl");
            let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);

            let body: serde_json::Value = resp.json();
            assert_eq!(body["street"]["value"], "Grote Markt");
            assert_eq!(body["street"]["lang"], "nl");
            assert_eq!(body["description"]["value"], "Een beroemd plein");
            assert_eq!(body["description"]["lang"], "nl");
        },
    ));

    // ── Array values in languageMap (spec 5.7.2.5) ─────────────────

    // Language map values can be arrays of strings per the spec.
    trials.push(trial(&format!("{PREFIX}::retrieve::array_value_in_language_map_preserved"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:lang-array");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": {
                    "fr": ["Grand Place", "Place Royale"],
                    "nl": "Grote Markt"
                }
            },
            "@context": [CTX]
        });
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().lang("fr");
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["street"]["type"], "Property");
        // Array value should be preserved as-is
        let value = &body["street"]["value"];
        assert!(value.is_array(), "array languageMap values must be preserved as arrays");
        let arr = value.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0], "Grand Place");
        assert_eq!(arr[1], "Place Royale");
    }));

    // ── NGSILD-Language header ─────────────────────────────────────

    // NGSILD-Language header is used when lang query parameter is absent.
    trials.push(trial(&format!("{PREFIX}::header::language_header_used_as_fallback"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:lang-header");
        let resp = ctx.client.create_entity(&building_fr_nl(&id)).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let headers = ctx_headers().language("nl");
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["street"]["type"], "Property");
        assert_eq!(body["street"]["value"], "Grote Markt");
        assert_eq!(body["street"]["lang"], "nl");
    }));

    // lang query parameter takes precedence over NGSILD-Language header.
    trials.push(trial(&format!("{PREFIX}::header::query_param_overrides_header"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:lang-precedence");
        let resp = ctx.client.create_entity(&building_fr_nl(&id)).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().lang("fr");
        let headers = ctx_headers().language("nl");
        let resp = ctx.client.get_entity(&id, &params, headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["street"]["value"], "Grand Place", "query param fr must override header nl");
        assert_eq!(body["street"]["lang"], "fr");
    }));

    // NGSILD-Language header with ranked preferences.
    trials.push(trial(&format!("{PREFIX}::header::ranked_preferences_in_header"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:lang-header-ranked");
        let resp = ctx.client.create_entity(&building_fr_nl(&id)).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let headers = ctx_headers().language("nl;q=0.5,fr");
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["street"]["value"], "Grand Place", "fr has higher quality (1.0 vs 0.5)");
        assert_eq!(body["street"]["lang"], "fr");
    }));

    // Invalid NGSILD-Language header returns 400.
    trials.push(trial(&format!("{PREFIX}::header::invalid_header_returns_400"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:lang-header-invalid");
        let resp = ctx.client.create_entity(&building_fr_nl(&id)).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let headers = ctx_headers().language("");
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), headers).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // ── Query entities (GET /entities) with lang ──────────────────���

    // Language negotiation applies to query results.
    trials.push(trial(&format!("{PREFIX}::query::lang_applied_to_query_results"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:lang-query");
        let resp = ctx.client.create_entity(&building_fr_nl(&id)).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().type_("Building").id(id.clone()).lang("nl");
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("query result should be array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["street"]["type"], "Property");
        assert_eq!(arr[0]["street"]["value"], "Grote Markt");
        assert_eq!(arr[0]["street"]["lang"], "nl");
    }));

    // NGSILD-Language header works for query endpoint too.
    trials.push(trial(&format!("{PREFIX}::query::header_applied_to_query_results"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:lang-query-header");
        let resp = ctx.client.create_entity(&building_fr_nl(&id)).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().type_("Building").id(id.clone());
        let headers = ctx_headers().language("fr");
        let resp = ctx.client.query_entities(&params, headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("query result should be array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["street"]["value"], "Grand Place");
        assert_eq!(arr[0]["street"]["lang"], "fr");
    }));

    // Query with keyValues and lang returns simplified negotiated values.
    trials.push(trial(&format!("{PREFIX}::query::key_values_with_lang"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:lang-query-kv");
        let resp = ctx.client.create_entity(&building_fr_nl(&id)).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().type_("Building").id(id.clone()).lang("nl").format("keyValues");
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("query result should be array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["street"], "Grote Markt");
    }));

    // ── Batch query (POST /entityOperations/query) with lang ───────

    // Language negotiation applies to batch query results via NGSILD-Language header.
    trials.push(trial(&format!("{PREFIX}::batch_query::header_applied_to_batch_query"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:lang-batch");
        let resp = ctx.client.create_entity(&building_fr_nl(&id)).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let query_body = serde_json::json!({
            "type": "Query",
            "entities": [{"type": "Building"}],
            "@context": [CTX]
        });
        let headers = RequestHeaders::new().accept_ld_json().language("nl");
        let resp = ctx.client.batch_query(&query_body, headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("batch query result should be array");
        // Find our entity in the results
        let our_entity = arr.iter().find(|e| e["id"].as_str() == Some(&id)).expect("entity should be in results");
        assert_eq!(our_entity["street"]["type"], "Property");
        assert_eq!(our_entity["street"]["value"], "Grote Markt");
        assert_eq!(our_entity["street"]["lang"], "nl");
    }));
}
