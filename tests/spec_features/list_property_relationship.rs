/// Feature 6: ListProperty and ListRelationship in core processing (spec 5.2.36, 5.2.37)
///
/// Verifies that ListProperty/ListRelationship attribute types are correctly
/// handled through create, retrieve, simplify, and join operations.
use crate::harness::{
    self,
    client::{NgsiLdOption, QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use reqwest::StatusCode;

const PREFIX: &str = "spec_features::list_property_relationship";

fn ctx_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── Create and retrieve entity with ListProperty ──────────────
    trials.push(trial(&format!("{PREFIX}::list_property_create_retrieve"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:lp1");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "tags": {
                "type": "ListProperty",
                "valueList": ["residential", "commercial", "mixed-use"]
            },
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Retrieve normalized
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let tags = &body["tags"];
        assert_eq!(tags["type"], "ListProperty", "type should be ListProperty");
        let value_list = tags["valueList"].as_array().expect("valueList should be array");
        assert_eq!(value_list.len(), 3);
        assert_eq!(value_list[0], "residential");
    }));

    // ── ListProperty in keyValues format ──────────────────────────
    trials.push(trial(&format!("{PREFIX}::list_property_simplified"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:lp2");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "tags": {
                "type": "ListProperty",
                "valueList": [1, 2, 3]
            },
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Retrieve simplified
        let params = QueryParams::new().options([NgsiLdOption::KeyValues]);
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let tags = body["tags"].as_array().expect("simplified tags should be array");
        assert_eq!(tags.len(), 3);
        assert_eq!(tags[0], 1);
        assert_eq!(tags[1], 2);
        assert_eq!(tags[2], 3);
    }));

    // ── Create and retrieve entity with ListRelationship ──────────
    trials.push(trial(&format!("{PREFIX}::list_relationship_create_retrieve"), |ctx| async move {
        let target_a = "urn:ngsi-ld:Room:A";
        let target_b = "urn:ngsi-ld:Room:B";
        ctx.track_entity(target_a);
        ctx.track_entity(target_b);

        for (tid, name) in [(target_a, "Room A"), (target_b, "Room B")] {
            let e = serde_json::json!({
                "id": tid, "type": "Room",
                "name": {"type": "Property", "value": name},
                "@context": [fixtures::test_suite_context_url()]
            });
            let resp = ctx.client.create_entity(&e).await;
            assert_eq!(resp.status, StatusCode::CREATED);
        }

        let id = ctx.entity_id("urn:ngsi-ld:Building:lr1");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "hasRooms": {
                "type": "ListRelationship",
                "objectList": [target_a, target_b]
            },
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Retrieve normalized
        let resp = ctx.client.get_entity(&id, &QueryParams::new(), ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let has_rooms = &body["hasRooms"];
        assert_eq!(has_rooms["type"], "ListRelationship");
        let obj_list = has_rooms["objectList"].as_array().expect("objectList should be array");
        assert_eq!(obj_list.len(), 2);
    }));

    // ── ListRelationship with join=flat ───────────────────────────
    trials.push(trial(&format!("{PREFIX}::list_relationship_join_flat"), |ctx| async move {
        let target_a = "urn:ngsi-ld:Sensor:A";
        let target_b = "urn:ngsi-ld:Sensor:B";
        let target_c = "urn:ngsi-ld:Sensor:C";
        ctx.track_entity(target_a);
        ctx.track_entity(target_b);
        ctx.track_entity(target_c);

        for (tid, name) in [(target_a, "A"), (target_b, "B"), (target_c, "C")] {
            let e = serde_json::json!({
                "id": tid, "type": "Sensor",
                "name": {"type": "Property", "value": name},
                "@context": [fixtures::test_suite_context_url()]
            });
            let resp = ctx.client.create_entity(&e).await;
            assert_eq!(resp.status, StatusCode::CREATED);
        }

        let id = ctx.entity_id("urn:ngsi-ld:Building:lrjf");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "hasSensors": {
                "type": "ListRelationship",
                "objectList": [target_a, target_b, target_c]
            },
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Query with join=flat
        let params = QueryParams::new().id(id.clone()).type_("Building").join("flat").join_level(1);
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("flat join returns array");
        assert!(arr.len() >= 4, "should include building + 3 linked sensors, got {}", arr.len());

        let ids: Vec<&str> = arr.iter().filter_map(|e| e["id"].as_str()).collect();
        assert!(ids.contains(&target_a));
        assert!(ids.contains(&target_b));
        assert!(ids.contains(&target_c));
    }));

    // ── ListRelationship with join=inline ─────────────────────────
    trials.push(trial(&format!("{PREFIX}::list_relationship_join_inline"), |ctx| async move {
        let target_a = "urn:ngsi-ld:Device:inlA";
        let target_b = "urn:ngsi-ld:Device:inlB";
        ctx.track_entity(target_a);
        ctx.track_entity(target_b);

        for (tid, name) in [(target_a, "DevA"), (target_b, "DevB")] {
            let e = serde_json::json!({
                "id": tid, "type": "Device",
                "name": {"type": "Property", "value": name},
                "@context": [fixtures::test_suite_context_url()]
            });
            let resp = ctx.client.create_entity(&e).await;
            assert_eq!(resp.status, StatusCode::CREATED);
        }

        let id = ctx.entity_id("urn:ngsi-ld:Building:lrinl");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "hasDevices": {
                "type": "ListRelationship",
                "objectList": [target_a, target_b]
            },
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // GET with join=inline
        let params = QueryParams::new().join("inline").join_level(1);
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let has_devices = &body["hasDevices"];
        assert_eq!(has_devices["type"], "ListRelationship");

        // Should have entityList with inlined entities
        let entity_list = has_devices["entityList"]
            .as_array()
            .expect("ListRelationship with inline join should have entityList");
        assert_eq!(entity_list.len(), 2, "entityList should contain both linked devices");
    }));

    // ── ListRelationship in simplified format ─────────────────────
    trials.push(trial(&format!("{PREFIX}::list_relationship_simplified"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:lrs");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "hasRooms": {
                "type": "ListRelationship",
                "objectList": ["urn:ngsi-ld:Room:X", "urn:ngsi-ld:Room:Y"]
            },
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let params = QueryParams::new().options([NgsiLdOption::KeyValues]);
        let resp = ctx.client.get_entity(&id, &params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let has_rooms = body["hasRooms"].as_array().expect("simplified objectList should be array");
        assert_eq!(has_rooms.len(), 2);
        assert_eq!(has_rooms[0], "urn:ngsi-ld:Room:X");
        assert_eq!(has_rooms[1], "urn:ngsi-ld:Room:Y");
    }));
}
