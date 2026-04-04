/// Attribute-level expiresAt (ETSI GS CIM 009 clause 4.22).
///
/// Tests that attribute-level expiresAt is preserved through create/retrieve,
/// independent of entity-level expiresAt, and that the background worker
/// removes expired attribute instances.
use crate::harness::{
    client::{NgsiLdOption, QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use reqwest::StatusCode;

const PREFIX: &str = "spec_features::attribute_expiration";

fn ctx_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    // -- Preserve expiresAt on create/retrieve for all 8 attribute types --

    trials.push(trial(
        &format!("{PREFIX}::property_with_expires_at_preserved_on_create_retrieve"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Vehicle:attrexp-prop1");
            let entity = serde_json::json!({
                "id": id,
                "type": "Vehicle",
                "speed": {
                    "type": "Property",
                    "value": 80,
                    "expiresAt": "2099-12-31T23:59:59Z"
                },
                "@context": [fixtures::test_suite_context_url()]
            });
            ctx.client.create_entity(&entity).await;

            let params = QueryParams::new().options([NgsiLdOption::SysAttrs]);
            let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);
            let body: serde_json::Value = resp.json();
            assert_eq!(body["speed"]["expiresAt"], "2099-12-31T23:59:59Z");
        },
    ));

    trials.push(trial(
        &format!("{PREFIX}::relationship_with_expires_at_preserved_on_create_retrieve"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Vehicle:attrexp-rel1");
            let entity = serde_json::json!({
                "id": id,
                "type": "Vehicle",
                "isParkedIn": {
                    "type": "Relationship",
                    "object": "urn:ngsi-ld:ParkingSpot:1",
                    "expiresAt": "2099-06-15T12:00:00Z"
                },
                "@context": [fixtures::test_suite_context_url()]
            });
            ctx.client.create_entity(&entity).await;

            let params = QueryParams::new().options([NgsiLdOption::SysAttrs]);
            let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);
            let body: serde_json::Value = resp.json();
            assert_eq!(body["isParkedIn"]["expiresAt"], "2099-06-15T12:00:00Z");
        },
    ));

    trials.push(trial(&format!("{PREFIX}::geo_property_with_expires_at_preserved"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:attrexp-geo1");
        let entity = serde_json::json!({
            "id": id,
            "type": "Vehicle",
            "location": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [13.0, 52.0]},
                "expiresAt": "2099-01-01T00:00:00Z"
            },
            "@context": [fixtures::test_suite_context_url()]
        });
        ctx.client.create_entity(&entity).await;

        let params = QueryParams::new().options([NgsiLdOption::SysAttrs]);
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["location"]["expiresAt"], "2099-01-01T00:00:00Z");
    }));

    trials.push(trial(&format!("{PREFIX}::vocab_property_with_expires_at_preserved"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:attrexp-vocab1");
        let entity = serde_json::json!({
            "id": id,
            "type": "Vehicle",
            "category": {
                "type": "VocabProperty",
                "vocab": "bus",
                "expiresAt": "2099-03-15T08:00:00Z"
            },
            "@context": [fixtures::test_suite_context_url()]
        });
        ctx.client.create_entity(&entity).await;

        let params = QueryParams::new().options([NgsiLdOption::SysAttrs]);
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["category"]["expiresAt"], "2099-03-15T08:00:00Z");
    }));

    trials.push(trial(&format!("{PREFIX}::language_property_with_expires_at_preserved"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:attrexp-lang1");
        let entity = serde_json::json!({
            "id": id,
            "type": "Vehicle",
            "name": {
                "type": "LanguageProperty",
                "languageMap": {"en": "bus", "de": "Bus"},
                "expiresAt": "2099-07-01T00:00:00Z"
            },
            "@context": [fixtures::test_suite_context_url()]
        });
        ctx.client.create_entity(&entity).await;

        let params = QueryParams::new().options([NgsiLdOption::SysAttrs]);
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["name"]["expiresAt"], "2099-07-01T00:00:00Z");
    }));

    trials.push(trial(&format!("{PREFIX}::json_property_with_expires_at_preserved"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:attrexp-json1");
        let entity = serde_json::json!({
            "id": id,
            "type": "Vehicle",
            "diagnostics": {
                "type": "JsonProperty",
                "json": {"engine": "ok", "brakes": "warning"},
                "expiresAt": "2099-09-01T12:00:00Z"
            },
            "@context": [fixtures::test_suite_context_url()]
        });
        ctx.client.create_entity(&entity).await;

        let params = QueryParams::new().options([NgsiLdOption::SysAttrs]);
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["diagnostics"]["expiresAt"], "2099-09-01T12:00:00Z");
    }));

    trials.push(trial(&format!("{PREFIX}::list_property_with_expires_at_preserved"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:attrexp-listp1");
        let entity = serde_json::json!({
            "id": id,
            "type": "Vehicle",
            "tags": {
                "type": "ListProperty",
                "valueList": ["a", "b"],
                "expiresAt": "2099-11-20T06:30:00Z"
            },
            "@context": [fixtures::test_suite_context_url()]
        });
        ctx.client.create_entity(&entity).await;

        let params = QueryParams::new().options([NgsiLdOption::SysAttrs]);
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["tags"]["expiresAt"], "2099-11-20T06:30:00Z");
    }));

    trials.push(trial(&format!("{PREFIX}::list_relationship_with_expires_at_preserved"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:attrexp-listr1");
        let entity = serde_json::json!({
            "id": id,
            "type": "Vehicle",
            "connectedTo": {
                "type": "ListRelationship",
                "objectList": ["urn:ngsi-ld:Vehicle:2", "urn:ngsi-ld:Vehicle:3"],
                "expiresAt": "2099-04-10T18:00:00Z"
            },
            "@context": [fixtures::test_suite_context_url()]
        });
        ctx.client.create_entity(&entity).await;

        let params = QueryParams::new().options([NgsiLdOption::SysAttrs]);
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["connectedTo"]["expiresAt"], "2099-04-10T18:00:00Z");
    }));

    // -- Independence and edge cases --

    trials.push(trial(
        &format!("{PREFIX}::attribute_expires_at_independent_of_entity_expires_at"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Vehicle:attrexp-indep1");
            let entity = serde_json::json!({
                "id": id,
                "type": "Vehicle",
                "expiresAt": "2099-01-01T00:00:00Z",
                "speed": {
                    "type": "Property",
                    "value": 60,
                    "expiresAt": "2098-06-01T00:00:00Z"
                },
                "@context": [fixtures::test_suite_context_url()]
            });
            ctx.client.create_entity(&entity).await;

            let params = QueryParams::new().options([NgsiLdOption::SysAttrs]);
            let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);
            let body: serde_json::Value = resp.json();
            // Entity-level expiresAt
            assert_eq!(body["expiresAt"], "2099-01-01T00:00:00Z");
            // Attribute-level expiresAt (different value)
            assert_eq!(body["speed"]["expiresAt"], "2098-06-01T00:00:00Z");
        },
    ));

    trials.push(trial(
        &format!("{PREFIX}::multi_instance_attribute_with_different_expires_at"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Vehicle:attrexp-multi1");
            let entity = serde_json::json!({
                "id": id,
                "type": "Vehicle",
                "speed": [
                    {
                        "type": "Property",
                        "value": 80,
                        "datasetId": "urn:ngsi-ld:dataset:speedometer",
                        "expiresAt": "2099-12-31T23:59:59Z"
                    },
                    {
                        "type": "Property",
                        "value": 78,
                        "datasetId": "urn:ngsi-ld:dataset:gps",
                        "expiresAt": "2098-06-15T00:00:00Z"
                    }
                ],
                "@context": [fixtures::test_suite_context_url()]
            });
            ctx.client.create_entity(&entity).await;

            let params = QueryParams::new().options([NgsiLdOption::SysAttrs]);
            let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);
            let body: serde_json::Value = resp.json();
            let speed = body["speed"].as_array().expect("speed should be multi-instance array");
            assert_eq!(speed.len(), 2);
            // Both instances should have their respective expiresAt
            let expires_values: Vec<&str> = speed.iter().filter_map(|s| s["expiresAt"].as_str()).collect();
            assert!(expires_values.contains(&"2099-12-31T23:59:59Z"));
            assert!(expires_values.contains(&"2098-06-15T00:00:00Z"));
        },
    ));

    trials.push(trial(&format!("{PREFIX}::attribute_expires_at_survives_entity_update"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:attrexp-surv1");
        let entity = serde_json::json!({
            "id": id,
            "type": "Vehicle",
            "speed": {
                "type": "Property",
                "value": 50,
                "expiresAt": "2099-12-31T23:59:59Z"
            },
            "@context": [fixtures::test_suite_context_url()]
        });
        ctx.client.create_entity(&entity).await;

        // Merge-patch with a different attribute
        let patch = serde_json::json!({
            "id": id,
            "type": "Vehicle",
            "fuel": {
                "type": "Property",
                "value": 75
            },
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.merge_entity(&id, &patch).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let params = QueryParams::new().options([NgsiLdOption::SysAttrs]);
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["speed"]["expiresAt"], "2099-12-31T23:59:59Z");
    }));

    trials.push(trial(
        &format!("{PREFIX}::attribute_expires_at_present_without_sys_attrs_option"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Vehicle:attrexp-nosys1");
            let entity = serde_json::json!({
                "id": id,
                "type": "Vehicle",
                "speed": {
                    "type": "Property",
                    "value": 90,
                    "expiresAt": "2099-12-31T23:59:59Z"
                },
                "@context": [fixtures::test_suite_context_url()]
            });
            ctx.client.create_entity(&entity).await;

            // Retrieve WITHOUT sysAttrs -- expiresAt is user-settable, should still appear
            let resp = ctx.client.get_entity(&id, &QueryParams::new(), ctx_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);
            let body: serde_json::Value = resp.json();
            assert_eq!(body["speed"]["expiresAt"], "2099-12-31T23:59:59Z");
        },
    ));

    // -- Worker-driven expiration tests --

    trials.push(trial(&format!("{PREFIX}::partial_multi_instance_expiration"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:attrexp-part1");
        let entity = serde_json::json!({
            "id": id,
            "type": "Vehicle",
            "speed": [
                {
                    "type": "Property",
                    "value": 80,
                    "datasetId": "urn:ngsi-ld:dataset:a",
                    "expiresAt": "2020-01-01T00:00:00Z"
                },
                {
                    "type": "Property",
                    "value": 78,
                    "datasetId": "urn:ngsi-ld:dataset:b",
                    "expiresAt": "2099-12-31T23:59:59Z"
                },
                {
                    "type": "Property",
                    "value": 75,
                    "datasetId": "urn:ngsi-ld:dataset:c"
                }
            ],
            "@context": [fixtures::test_suite_context_url()]
        });
        ctx.client.create_entity(&entity).await;

        // Wait for worker cycle
        tokio::time::sleep(std::time::Duration::from_secs(12)).await;

        let params = QueryParams::new().options([NgsiLdOption::SysAttrs]);
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        let speed = body["speed"].as_array().expect("speed should be multi-instance array");
        assert_eq!(speed.len(), 2, "expired instance should be removed, 2 remaining");
    }));

    trials.push(trial(
        &format!("{PREFIX}::all_instances_of_one_attribute_expire_others_remain"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Vehicle:attrexp-allone1");
            let entity = serde_json::json!({
                "id": id,
                "type": "Vehicle",
                "speed": {
                    "type": "Property",
                    "value": 80,
                    "expiresAt": "2020-01-01T00:00:00Z"
                },
                "fuel": {
                    "type": "Property",
                    "value": 55
                },
                "@context": [fixtures::test_suite_context_url()]
            });
            ctx.client.create_entity(&entity).await;

            tokio::time::sleep(std::time::Duration::from_secs(12)).await;

            let params = QueryParams::new().options([NgsiLdOption::SysAttrs]);
            let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);
            let body: serde_json::Value = resp.json();
            assert!(body.get("speed").is_none(), "expired speed attribute should be removed");
            assert!(body.get("fuel").is_some(), "non-expired fuel attribute should remain");
        },
    ));

    trials.push(trial(
        &format!("{PREFIX}::location_geo_column_nulled_when_location_attribute_expires"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Vehicle:attrexp-geo-null1");
            let entity = serde_json::json!({
                "id": id,
                "type": "Vehicle",
                "location": {
                    "type": "GeoProperty",
                    "value": {"type": "Point", "coordinates": [13.0, 52.0]},
                    "expiresAt": "2020-01-01T00:00:00Z"
                },
                "speed": {
                    "type": "Property",
                    "value": 60
                },
                "@context": [fixtures::test_suite_context_url()]
            });
            ctx.client.create_entity(&entity).await;

            tokio::time::sleep(std::time::Duration::from_secs(12)).await;

            // Entity should still exist but location should be gone
            let params = QueryParams::new().options([NgsiLdOption::SysAttrs]);
            let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);
            let body: serde_json::Value = resp.json();
            assert!(body.get("location").is_none(), "expired location should be removed");
            assert!(body.get("speed").is_some(), "non-expired speed should remain");

            // Geo query should no longer match this entity
            let geo_params = QueryParams::new()
                .type_("Vehicle")
                .georel("near;maxDistance==1000")
                .geometry("Point")
                .coordinates("[13.0,52.0]");
            let resp = ctx.client.query_entities(&geo_params, ctx_headers()).await;
            let body: serde_json::Value = resp.json();
            let arr = body.as_array().unwrap_or(&Vec::new()).to_vec();
            let matched_ids: Vec<&str> = arr.iter().filter_map(|e| e["id"].as_str()).collect();
            assert!(!matched_ids.contains(&id.as_str()), "expired location should not match geo queries");
        },
    ));

    trials.push(trial(
        &format!("{PREFIX}::entity_level_and_attribute_level_expire_independently"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Vehicle:attrexp-entattr1");
            let entity = serde_json::json!({
                "id": id,
                "type": "Vehicle",
                "expiresAt": "2099-12-31T23:59:59Z",
                "speed": {
                    "type": "Property",
                    "value": 80,
                    "expiresAt": "2020-01-01T00:00:00Z"
                },
                "fuel": {
                    "type": "Property",
                    "value": 50
                },
                "@context": [fixtures::test_suite_context_url()]
            });
            ctx.client.create_entity(&entity).await;

            tokio::time::sleep(std::time::Duration::from_secs(12)).await;

            // Entity should still exist (entity-level expiresAt is far future)
            let params = QueryParams::new().options([NgsiLdOption::SysAttrs]);
            let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);
            let body: serde_json::Value = resp.json();
            assert!(body.get("speed").is_none(), "attribute with past expiresAt should be removed");
            assert!(body.get("fuel").is_some(), "attribute without expiresAt should remain");
            assert_eq!(body["expiresAt"], "2099-12-31T23:59:59Z", "entity-level expiresAt should be unaffected");
        },
    ));
}
