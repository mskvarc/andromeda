use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

const PREFIX: &str = "entity_map";

fn ctx_headers() -> RequestHeaders {
    RequestHeaders::new().link_context(fixtures::test_suite_context_url())
}

fn ld_json_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 12.1 Retrieve EntityMap (§5.14.1) ────────────────────────────

    trials.push(trial(&format!("{PREFIX}::retrieve::existing_map_200"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:em-retr");
        let entity = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id);
        assert_eq!(ctx.client.create_entity(&entity).await.status, StatusCode::CREATED);

        let params = QueryParams::new().type_("Vehicle");
        let resp = ctx.client.create_entity_map(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let body: serde_json::Value = resp.json();
        let map_id = body["id"].as_str().expect("map should have id");
        ctx.track_entity_map(map_id);

        assert_eq!(body["type"], "EntityMap");
        assert!(body["expiresAt"].is_string());
        assert!(body["entityMap"].is_object());
        assert!(body["linkedMaps"].is_object());

        // All entity IDs map to ["@none"]
        let entity_map = body["entityMap"].as_object().unwrap();
        for (_eid, brokers) in entity_map {
            let arr = brokers.as_array().unwrap();
            assert_eq!(arr.len(), 1);
            assert_eq!(arr[0], "@none");
        }

        // linkedMaps is empty
        assert!(body["linkedMaps"].as_object().unwrap().is_empty());

        // Retrieve the map
        let resp = ctx.client.get_entity_map(map_id, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body2: serde_json::Value = resp.json();
        assert_eq!(body2["id"], map_id);
        assert_eq!(body2["type"], "EntityMap");
    }));

    trials.push(trial(&format!("{PREFIX}::retrieve::nonexistent_404"), |_ctx| async move {
        let resp = _ctx.client.get_entity_map("urn:ngsi-ld:EntityMap:nonexistent", ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    trials.push(trial(&format!("{PREFIX}::retrieve::invalid_uri_400"), |_ctx| async move {
        let resp = _ctx.client.get_entity_map("not-a-valid-uri", ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // ── 12.2 Update EntityMap (§5.14.2) ──────────────────────────────

    trials.push(trial(&format!("{PREFIX}::update::patch_expires_at_204"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:em-upd");
        let entity = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id);
        assert_eq!(ctx.client.create_entity(&entity).await.status, StatusCode::CREATED);

        let params = QueryParams::new().type_("Vehicle");
        let resp = ctx.client.create_entity_map(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let body: serde_json::Value = resp.json();
        let map_id = body["id"].as_str().unwrap();
        ctx.track_entity_map(map_id);

        let new_expires = chrono::Utc::now() + chrono::Duration::hours(2);
        let update_body = serde_json::json!({
            "expiresAt": new_expires.to_rfc3339()
        });
        let resp = ctx.client.update_entity_map(map_id, &update_body).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        // Verify updated
        let resp = ctx.client.get_entity_map(map_id, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
    }));

    trials.push(trial(&format!("{PREFIX}::update::output_only_fields_ignored"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:em-upd-oof");
        let entity = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id);
        assert_eq!(ctx.client.create_entity(&entity).await.status, StatusCode::CREATED);

        let params = QueryParams::new().type_("Vehicle");
        let resp = ctx.client.create_entity_map(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let body: serde_json::Value = resp.json();
        let map_id = body["id"].as_str().unwrap();
        ctx.track_entity_map(map_id);

        let new_expires = chrono::Utc::now() + chrono::Duration::hours(3);
        let update_body = serde_json::json!({
            "expiresAt": new_expires.to_rfc3339(),
            "entityMap": { "urn:fake": ["@none"] },
            "linkedMaps": { "fake": "fake" }
        });
        let resp = ctx.client.update_entity_map(map_id, &update_body).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);
    }));

    trials.push(trial(&format!("{PREFIX}::update::nonexistent_404"), |_ctx| async move {
        let update_body = serde_json::json!({
            "expiresAt": (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339()
        });
        let resp = _ctx.client.update_entity_map("urn:ngsi-ld:EntityMap:nonexistent", &update_body).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    trials.push(trial(&format!("{PREFIX}::update::invalid_expires_at_400"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:em-upd-bad");
        let entity = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id);
        assert_eq!(ctx.client.create_entity(&entity).await.status, StatusCode::CREATED);

        let params = QueryParams::new().type_("Vehicle");
        let resp = ctx.client.create_entity_map(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let body: serde_json::Value = resp.json();
        let map_id = body["id"].as_str().unwrap();
        ctx.track_entity_map(map_id);

        let update_body = serde_json::json!({
            "expiresAt": "not-a-date"
        });
        let resp = ctx.client.update_entity_map(map_id, &update_body).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // ── 12.3 Delete EntityMap (§5.14.3) ──────────────────────────────

    trials.push(trial(&format!("{PREFIX}::delete::existing_204_then_404"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:em-del");
        let entity = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id);
        assert_eq!(ctx.client.create_entity(&entity).await.status, StatusCode::CREATED);

        let params = QueryParams::new().type_("Vehicle");
        let resp = ctx.client.create_entity_map(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let body: serde_json::Value = resp.json();
        let map_id = body["id"].as_str().unwrap().to_string();

        let resp = ctx.client.delete_entity_map(&map_id).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        let resp = ctx.client.get_entity_map(&map_id, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    trials.push(trial(&format!("{PREFIX}::delete::nonexistent_404"), |_ctx| async move {
        let resp = _ctx.client.delete_entity_map("urn:ngsi-ld:EntityMap:nonexistent").await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // ── 12.4 Create EntityMap for Query Entities (§5.14.4) ───────────

    trials.push(trial(&format!("{PREFIX}::create::get_by_type_201"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Vehicle:em-create1");
        let id2 = ctx.entity_id("urn:ngsi-ld:Vehicle:em-create2");

        for id in [&id1, &id2] {
            let entity = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", id);
            assert_eq!(ctx.client.create_entity(&entity).await.status, StatusCode::CREATED);
        }

        let params = QueryParams::new().type_("Vehicle");
        let resp = ctx.client.create_entity_map(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let body: serde_json::Value = resp.json();
        let map_id = body["id"].as_str().unwrap();
        ctx.track_entity_map(map_id);

        assert_eq!(body["type"], "EntityMap");
        assert!(body["expiresAt"].is_string());
        let entity_map = body["entityMap"].as_object().unwrap();
        assert_eq!(entity_map.len(), 2);
        assert!(entity_map.contains_key(id1.as_str()));
        assert!(entity_map.contains_key(id2.as_str()));

        // Location header present
        assert!(resp.location().is_some());
        let location = resp.location().unwrap();
        assert!(location.contains("entityMaps/"));

        // NGSILD-EntityMap header present
        assert!(resp.headers.get("NGSILD-EntityMap").is_some());

        // Map is retrievable
        let resp = ctx.client.get_entity_map(map_id, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
    }));

    trials.push(trial(&format!("{PREFIX}::create::with_q_filter_only_matching_ids"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Vehicle:em-q1");
        let id2 = ctx.entity_id("urn:ngsi-ld:Vehicle:em-q2");

        let mut entity1 = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id1);
        entity1["speed"] = serde_json::json!({"type": "Property", "value": 100});
        assert_eq!(ctx.client.create_entity(&entity1).await.status, StatusCode::CREATED);

        let mut entity2 = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id2);
        entity2["speed"] = serde_json::json!({"type": "Property", "value": 30});
        assert_eq!(ctx.client.create_entity(&entity2).await.status, StatusCode::CREATED);

        let params = QueryParams::new().type_("Vehicle").q("speed>50");
        let resp = ctx.client.create_entity_map(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let body: serde_json::Value = resp.json();
        let map_id = body["id"].as_str().unwrap();
        ctx.track_entity_map(map_id);

        let entity_map = body["entityMap"].as_object().unwrap();
        assert_eq!(entity_map.len(), 1, "only the entity with speed>50 should be in the map");
        assert!(entity_map.contains_key(id1.as_str()));
    }));

    trials.push(trial(&format!("{PREFIX}::create::entity_map_lifetime_pt5m"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:em-lt");
        let entity = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id);
        assert_eq!(ctx.client.create_entity(&entity).await.status, StatusCode::CREATED);

        let params = QueryParams::new().type_("Vehicle").entity_map_lifetime("PT5M");
        let resp = ctx.client.create_entity_map(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let body: serde_json::Value = resp.json();
        let map_id = body["id"].as_str().unwrap();
        ctx.track_entity_map(map_id);

        // Verify expiresAt is approximately 5 minutes from now
        let expires_at = body["expiresAt"].as_str().unwrap();
        let dt = chrono::DateTime::parse_from_rfc3339(expires_at).unwrap();
        let diff = dt.signed_duration_since(chrono::Utc::now());
        assert!(diff.num_seconds() > 240 && diff.num_seconds() <= 300);
    }));

    trials.push(trial(&format!("{PREFIX}::create::no_filters_400"), |_ctx| async move {
        let params = QueryParams::new().local(true);
        let resp = _ctx.client.create_entity_map(&params, ctx_headers()).await;
        // EntityMap creation requires at least one filter
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    trials.push(trial(&format!("{PREFIX}::create::post_body_201"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:em-post");
        let entity = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id);
        assert_eq!(ctx.client.create_entity(&entity).await.status, StatusCode::CREATED);

        let query_body = serde_json::json!({
            "entities": [{ "type": "Vehicle" }]
        });
        let resp = ctx.client.create_entity_map_post(&query_body, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let body: serde_json::Value = resp.json();
        let map_id = body["id"].as_str().unwrap();
        ctx.track_entity_map(map_id);
        assert_eq!(body["type"], "EntityMap");
    }));

    trials.push(trial(&format!("{PREFIX}::create::empty_result_201"), |ctx| async move {
        let params = QueryParams::new().type_("NonExistentType12345");
        let resp = ctx.client.create_entity_map(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let body: serde_json::Value = resp.json();
        let map_id = body["id"].as_str().unwrap();
        ctx.track_entity_map(map_id);

        let entity_map = body["entityMap"].as_object().unwrap();
        assert!(entity_map.is_empty(), "entityMap should be empty when no entities match");
    }));

    trials.push(trial(&format!("{PREFIX}::create::many_entities_captured"), |ctx| async move {
        let mut ids = Vec::new();
        for i in 0..50 {
            let id = ctx.entity_id(&format!("urn:ngsi-ld:Vehicle:em-many-{i}"));
            let entity = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id);
            assert_eq!(ctx.client.create_entity(&entity).await.status, StatusCode::CREATED);
            ids.push(id);
        }

        let params = QueryParams::new().type_("Vehicle");
        let resp = ctx.client.create_entity_map(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let body: serde_json::Value = resp.json();
        let map_id = body["id"].as_str().unwrap();
        ctx.track_entity_map(map_id);

        let entity_map = body["entityMap"].as_object().unwrap();
        assert_eq!(entity_map.len(), 50);
    }));

    // ── 12.5 Create EntityMap for Temporal Query (§5.14.5) ───────────

    trials.push(trial(&format!("{PREFIX}::temporal::get_201"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:em-temp");
        ctx.track_temporal_entity(&id);
        let entity = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id);
        assert_eq!(ctx.client.create_entity(&entity).await.status, StatusCode::CREATED);

        let params = QueryParams::new().type_("Vehicle");
        let resp = ctx.client.create_temporal_entity_map(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let body: serde_json::Value = resp.json();
        let map_id = body["id"].as_str().unwrap();
        ctx.track_entity_map(map_id);
        assert_eq!(body["type"], "EntityMap");
    }));

    trials.push(trial(&format!("{PREFIX}::temporal::post_201"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:em-temp-post");
        ctx.track_temporal_entity(&id);
        let entity = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id);
        assert_eq!(ctx.client.create_entity(&entity).await.status, StatusCode::CREATED);

        let query_body = serde_json::json!({
            "entities": [{ "type": "Vehicle" }]
        });
        let resp = ctx.client.create_temporal_entity_map_post(&query_body, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let body: serde_json::Value = resp.json();
        let map_id = body["id"].as_str().unwrap();
        ctx.track_entity_map(map_id);
        assert_eq!(body["type"], "EntityMap");
    }));

    // ── 12.6 EntityMap integration with Query Entities (§5.7.2) ──────

    trials.push(trial(&format!("{PREFIX}::integration::entity_map_true_201_with_header"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:em-int");
        let entity = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id);
        assert_eq!(ctx.client.create_entity(&entity).await.status, StatusCode::CREATED);

        let params = QueryParams::new().type_("Vehicle").entity_map(true);
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED, "entityMap=true should return 201");

        // NGSILD-EntityMap header present
        let map_header = resp.headers.get("NGSILD-EntityMap");
        assert!(map_header.is_some(), "NGSILD-EntityMap header should be present");

        let map_id = map_header.unwrap().to_str().unwrap();
        ctx.track_entity_map(map_id);

        // Verify map is retrievable
        let resp = ctx.client.get_entity_map(map_id, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let entity_map = body["entityMap"].as_object().unwrap();
        assert!(entity_map.contains_key(id.as_str()));
    }));

    trials.push(trial(&format!("{PREFIX}::integration::entity_map_false_200_no_header"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:em-int-false");
        let entity = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id);
        assert_eq!(ctx.client.create_entity(&entity).await.status, StatusCode::CREATED);

        let params = QueryParams::new().type_("Vehicle");
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
        assert!(resp.headers.get("NGSILD-EntityMap").is_none());
    }));

    // ── 12.7 Pagination with EntityMap (§5.5.9.3) ───────────────────

    trials.push(trial(&format!("{PREFIX}::pagination::paginate_through_entity_map"), |ctx| async move {
        // Create 10 entities
        let mut created_ids: Vec<String> = Vec::new();
        for i in 0..10 {
            let id = ctx.entity_id(&format!("urn:ngsi-ld:Vehicle:em-pg-{i}"));
            let entity = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id);
            assert_eq!(ctx.client.create_entity(&entity).await.status, StatusCode::CREATED);
            created_ids.push(id);
        }

        // Create entity map
        let params = QueryParams::new().type_("Vehicle");
        let resp = ctx.client.create_entity_map(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let body: serde_json::Value = resp.json();
        let map_id = body["id"].as_str().unwrap().to_string();
        ctx.track_entity_map(&map_id);

        let entity_map = body["entityMap"].as_object().unwrap();
        assert_eq!(entity_map.len(), 10);

        // Paginate: page 1 (limit=3, offset=0)
        let params1 = QueryParams::new().type_("Vehicle").limit(3).offset(0);
        let resp1 = ctx.client.query_entities_with_entity_map(&params1, &map_id, ctx_headers()).await;
        assert_eq!(resp1.status, StatusCode::OK);
        let body1: serde_json::Value = resp1.json();
        let arr1 = body1.as_array().unwrap();
        assert_eq!(arr1.len(), 3);

        // NGSILD-EntityMap header echoed back
        assert!(resp1.headers.get("NGSILD-EntityMap").is_some());

        // Page 2 (limit=3, offset=3)
        let params2 = QueryParams::new().type_("Vehicle").limit(3).offset(3);
        let resp2 = ctx.client.query_entities_with_entity_map(&params2, &map_id, ctx_headers()).await;
        assert_eq!(resp2.status, StatusCode::OK);
        let body2: serde_json::Value = resp2.json();
        let arr2 = body2.as_array().unwrap();
        assert_eq!(arr2.len(), 3);

        // Page 3 (limit=3, offset=6)
        let params3 = QueryParams::new().type_("Vehicle").limit(3).offset(6);
        let resp3 = ctx.client.query_entities_with_entity_map(&params3, &map_id, ctx_headers()).await;
        assert_eq!(resp3.status, StatusCode::OK);
        let body3: serde_json::Value = resp3.json();
        let arr3 = body3.as_array().unwrap();
        assert_eq!(arr3.len(), 3);

        // Page 4 (remainder: limit=3, offset=9)
        let params4 = QueryParams::new().type_("Vehicle").limit(3).offset(9);
        let resp4 = ctx.client.query_entities_with_entity_map(&params4, &map_id, ctx_headers()).await;
        assert_eq!(resp4.status, StatusCode::OK);
        let body4: serde_json::Value = resp4.json();
        let arr4 = body4.as_array().unwrap();
        assert_eq!(arr4.len(), 1, "last page should have 1 remaining entity");

        // Collect all paginated IDs and verify full coverage
        let mut all_ids: Vec<String> = Vec::new();
        for arr in [arr1, arr2, arr3, arr4] {
            for val in arr {
                all_ids.push(val["id"].as_str().unwrap().to_string());
            }
        }
        assert_eq!(all_ids.len(), 10);

        // No duplicates
        let id_set: std::collections::HashSet<String> = all_ids.iter().cloned().collect();
        assert_eq!(id_set.len(), 10, "should have no duplicate IDs");
    }));

    trials.push(trial(&format!("{PREFIX}::pagination::new_entity_not_in_map"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Vehicle:em-new1");
        let entity1 = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id1);
        assert_eq!(ctx.client.create_entity(&entity1).await.status, StatusCode::CREATED);

        // Create entity map
        let params = QueryParams::new().type_("Vehicle");
        let resp = ctx.client.create_entity_map(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let body: serde_json::Value = resp.json();
        let map_id = body["id"].as_str().unwrap().to_string();
        ctx.track_entity_map(&map_id);

        // Create a new entity AFTER map creation
        let id2 = ctx.entity_id("urn:ngsi-ld:Vehicle:em-new2");
        let entity2 = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id2);
        assert_eq!(ctx.client.create_entity(&entity2).await.status, StatusCode::CREATED);

        // Query with entity map — new entity should NOT appear
        let params = QueryParams::new().type_("Vehicle").limit(100);
        let resp = ctx.client.query_entities_with_entity_map(&params, &map_id, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().unwrap();
        let ids: Vec<&str> = arr.iter().filter_map(|v| v["id"].as_str()).collect();
        assert!(ids.contains(&id1.as_str()), "original entity should be in results");
        assert!(!ids.contains(&id2.as_str()), "new entity should NOT be in results");
    }));

    trials.push(trial(&format!("{PREFIX}::pagination::deleted_entity_skipped"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Vehicle:em-dsk1");
        let id2 = ctx.entity_id("urn:ngsi-ld:Vehicle:em-dsk2");

        for id in [&id1, &id2] {
            let entity = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", id);
            assert_eq!(ctx.client.create_entity(&entity).await.status, StatusCode::CREATED);
        }

        // Create entity map with both entities
        let params = QueryParams::new().type_("Vehicle");
        let resp = ctx.client.create_entity_map(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let body: serde_json::Value = resp.json();
        let map_id = body["id"].as_str().unwrap().to_string();
        ctx.track_entity_map(&map_id);

        let entity_map = body["entityMap"].as_object().unwrap();
        assert_eq!(entity_map.len(), 2);

        // Delete one entity
        assert_eq!(ctx.client.delete_entity(&id1).await.status, StatusCode::NO_CONTENT);

        // Query with entity map — deleted entity should be skipped
        let params = QueryParams::new().type_("Vehicle").limit(100);
        let resp = ctx.client.query_entities_with_entity_map(&params, &map_id, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().unwrap();
        assert_eq!(arr.len(), 1, "only remaining entity should be returned");
        assert_eq!(arr[0]["id"], id2);
    }));

    trials.push(trial(&format!("{PREFIX}::pagination::count_with_entity_map"), |ctx| async move {
        for i in 0..5 {
            let id = ctx.entity_id(&format!("urn:ngsi-ld:Vehicle:em-cnt-{i}"));
            let entity = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id);
            assert_eq!(ctx.client.create_entity(&entity).await.status, StatusCode::CREATED);
        }

        // Create entity map
        let params = QueryParams::new().type_("Vehicle");
        let resp = ctx.client.create_entity_map(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let body: serde_json::Value = resp.json();
        let map_id = body["id"].as_str().unwrap().to_string();
        ctx.track_entity_map(&map_id);

        // Query with count
        let params = QueryParams::new().type_("Vehicle").count(true).limit(2);
        let resp = ctx.client.query_entities_with_entity_map(&params, &map_id, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let count_header = resp.headers.get("NGSILD-Results-Count");
        assert!(count_header.is_some());
        let count: usize = count_header.unwrap().to_str().unwrap().parse().unwrap();
        assert_eq!(count, 5);
    }));

    // ── 12.8 Content negotiation ─────────────────────────────────────

    trials.push(trial(&format!("{PREFIX}::content_negotiation::json_link_header"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:em-cn-json");
        let entity = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id);
        assert_eq!(ctx.client.create_entity(&entity).await.status, StatusCode::CREATED);

        let params = QueryParams::new().type_("Vehicle");
        let resp = ctx.client.create_entity_map(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let body: serde_json::Value = resp.json();
        let map_id = body["id"].as_str().unwrap();
        ctx.track_entity_map(map_id);

        // GET with Accept: application/json → Link header
        let resp = ctx
            .client
            .get_entity_map(
                map_id,
                RequestHeaders::new()
                    .accept("application/json")
                    .link_context(fixtures::test_suite_context_url()),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);
        assert!(resp.headers.get("Link").is_some());

        let body: serde_json::Value = resp.json();
        assert!(body.get("@context").is_none(), "@context should not be in body for application/json");
    }));

    trials.push(trial(&format!("{PREFIX}::content_negotiation::ld_json_context_in_body"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:em-cn-ldjson");
        let entity = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id);
        assert_eq!(ctx.client.create_entity(&entity).await.status, StatusCode::CREATED);

        let params = QueryParams::new().type_("Vehicle");
        let resp = ctx.client.create_entity_map(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let body: serde_json::Value = resp.json();
        let map_id = body["id"].as_str().unwrap();
        ctx.track_entity_map(map_id);

        // GET with Accept: application/ld+json → @context in body
        let resp = ctx.client.get_entity_map(map_id, ld_json_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert!(body.get("@context").is_some(), "@context should be in body for application/ld+json");
    }));
}
