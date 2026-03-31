/// Feature 3: containedBy parameter (spec 6.4.3.2, 6.5.3.1)
///
/// Verifies that containedBy excludes specified entity IDs from linked entity
/// resolution, and prevents cycles in join traversal.
use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use reqwest::StatusCode;

const PREFIX: &str = "spec_features::contained_by";

fn ctx_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── containedBy excludes specified entities from join results ──
    trials.push(trial(&format!("{PREFIX}::excludes_contained_by_from_join"), |ctx| async move {
        // Create A → B → C
        let id_c = "urn:ngsi-ld:Entity:cb-C";
        ctx.track_entity(id_c);
        let c = serde_json::json!({
            "id": id_c, "type": "Building",
            "name": {"type": "Property", "value": "C"},
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&c).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let id_b = "urn:ngsi-ld:Entity:cb-B";
        ctx.track_entity(id_b);
        let b = serde_json::json!({
            "id": id_b, "type": "Building",
            "linkedTo": {"type": "Relationship", "object": id_c},
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&b).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let id_a = ctx.entity_id("urn:ngsi-ld:Entity:cb-A");
        let a = serde_json::json!({
            "id": id_a, "type": "Building",
            "linkedTo": {"type": "Relationship", "object": id_b},
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&a).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Query A with join=flat, joinLevel=2, containedBy=B → B excluded
        let params = QueryParams::new()
            .id(id_a.clone())
            .type_("Building")
            .join("flat")
            .join_level(2)
            .contained_by(id_b);
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("response should be array");
        let ids: Vec<&str> = arr.iter().filter_map(|e| e["id"].as_str()).collect();

        assert!(ids.contains(&id_a.as_str()), "should contain A");
        assert!(!ids.contains(&id_b), "B should be excluded by containedBy");
        // C may or may not be present (B was excluded so no link to C traversed)
    }));

    // ── join with cycle does not infinite loop ────────────────────
    trials.push(trial(&format!("{PREFIX}::cycle_does_not_loop"), |ctx| async move {
        // Create cycle: X → Y → X
        let id_x = "urn:ngsi-ld:Entity:cyc-X";
        let id_y = "urn:ngsi-ld:Entity:cyc-Y";
        ctx.track_entity(id_x);
        ctx.track_entity(id_y);

        let x = serde_json::json!({
            "id": id_x, "type": "Building",
            "linkedTo": {"type": "Relationship", "object": id_y},
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&x).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let y = serde_json::json!({
            "id": id_y, "type": "Building",
            "linkedTo": {"type": "Relationship", "object": id_x},
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&y).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Should complete without hanging (cycle protection already existed)
        let params = QueryParams::new().id(id_x.to_string()).type_("Building").join("flat").join_level(5);
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let arr = resp.json_value();
        let arr = arr.as_array().expect("response should be array");
        assert!(arr.len() >= 2, "should return both entities");
    }));

    // ── single entity GET with containedBy ────────────────────────
    trials.push(trial(&format!("{PREFIX}::single_entity_get_with_contained_by"), |ctx| async move {
        let id_target = "urn:ngsi-ld:Entity:sgcb-target";
        ctx.track_entity(id_target);
        let target = serde_json::json!({
            "id": id_target, "type": "Building",
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&target).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let id_main = ctx.entity_id("urn:ngsi-ld:Entity:sgcb-main");
        let main_e = serde_json::json!({
            "id": id_main, "type": "Building",
            "linkedTo": {"type": "Relationship", "object": id_target},
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&main_e).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // GET single entity with join=flat, containedBy=target → target excluded
        let params = QueryParams::new().join("flat").join_level(1).contained_by(id_target);
        let resp = ctx.client.get_entity(&id_main, &params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        // With flat join, response is array [main, ...linked]
        let arr = body.as_array().expect("flat join returns array");
        let ids: Vec<&str> = arr.iter().filter_map(|e| e["id"].as_str()).collect();
        assert!(ids.contains(&id_main.as_str()), "should contain main entity");
        assert!(!ids.contains(&id_target), "target should be excluded by containedBy");
    }));
}
