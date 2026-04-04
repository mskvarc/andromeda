/// Feature: NGSILD-Snapshot header scoping (spec 5.5.15, 6.3.22)
///
/// Verifies that the NGSILD-Snapshot header scopes entity and temporal
/// queries to the frozen data in a snapshot. The header must be echoed
/// in responses and operations are implicitly local-scope.
use crate::harness::{
    DATABASE_URL,
    client::{QueryParams, RequestHeaders},
    trial::trial,
};
use libtest_mimic::Trial;
use reqwest::StatusCode;
use sqlx::PgPool;

const PREFIX: &str = "spec_features::snapshot_scoping";

/// Insert entity data directly into snapshot_entities and set snapshot status to success.
async fn seed_snapshot_entities(pool: &PgPool, snapshot_id: &str, entities: &[(String, serde_json::Value)]) {
    for (entity_id, entity_data) in entities {
        sqlx::query("INSERT INTO snapshot_entities (snapshot_id, entity_id, entity_data) VALUES ($1, $2, $3)")
            .bind(snapshot_id)
            .bind(entity_id)
            .bind(entity_data)
            .execute(pool)
            .await
            .expect("failed to insert snapshot entity");
    }
    sqlx::query("UPDATE snapshots SET status = 'success' WHERE id = $1")
        .bind(snapshot_id)
        .execute(pool)
        .await
        .expect("failed to update snapshot status");
}

/// Insert temporal entity data directly into snapshot_temporal_entities.
async fn seed_snapshot_temporal_entities(pool: &PgPool, snapshot_id: &str, entities: &[(String, serde_json::Value)]) {
    for (entity_id, temporal_data) in entities {
        sqlx::query("INSERT INTO snapshot_temporal_entities (snapshot_id, entity_id, temporal_data) VALUES ($1, $2, $3)")
            .bind(snapshot_id)
            .bind(entity_id)
            .bind(temporal_data)
            .execute(pool)
            .await
            .expect("failed to insert snapshot temporal entity");
    }
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 1. Query entities scoped to snapshot ─────────────────────────
    trials.push(trial(&format!("{PREFIX}::query_entities_scoped_to_snapshot"), |ctx| async move {
        let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

        // Create a snapshot via the API
        let body = serde_json::json!({"snapshotLifetime": "PT1H"});
        let resp = ctx.client.create_snapshot(&body).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let created: serde_json::Value = resp.json();
        let snap_id = created["id"].as_str().unwrap().to_string();
        ctx.track_snapshot(&snap_id);

        // Seed two entities into snapshot_entities
        let eid1 = format!("urn:ngsi-ld:Vehicle:snap-q-{}", uuid::Uuid::new_v4());
        let eid2 = format!("urn:ngsi-ld:Vehicle:snap-q-{}", uuid::Uuid::new_v4());
        let entity1 = serde_json::json!({
            "id": eid1,
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 80}
        });
        let entity2 = serde_json::json!({
            "id": eid2,
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 120}
        });
        seed_snapshot_entities(&pool, &snap_id, &[(eid1.clone(), entity1), (eid2.clone(), entity2)]).await;

        // Query with NGSILD-Snapshot header
        let params = QueryParams::new().type_("Vehicle");
        let headers = RequestHeaders::new().snapshot(&snap_id);
        let resp = ctx.client.query_entities(&params, headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        // Verify NGSILD-Snapshot header is echoed
        let snap_header = resp.headers.get("NGSILD-Snapshot").and_then(|v| v.to_str().ok());
        assert_eq!(snap_header, Some(snap_id.as_str()), "NGSILD-Snapshot header must be echoed in response");

        let entities: Vec<serde_json::Value> = resp.json();
        assert_eq!(entities.len(), 2, "should return exactly 2 snapshot entities");

        let ids: Vec<&str> = entities.iter().filter_map(|e| e["id"].as_str()).collect();
        assert!(ids.contains(&eid1.as_str()));
        assert!(ids.contains(&eid2.as_str()));

        pool.close().await;
    }));

    // ── 2. Retrieve entity scoped to snapshot ────────────────────────
    trials.push(trial(&format!("{PREFIX}::retrieve_entity_scoped_to_snapshot"), |ctx| async move {
        let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

        let body = serde_json::json!({"snapshotLifetime": "PT1H"});
        let resp = ctx.client.create_snapshot(&body).await;
        let created: serde_json::Value = resp.json();
        let snap_id = created["id"].as_str().unwrap().to_string();
        ctx.track_snapshot(&snap_id);

        let eid = format!("urn:ngsi-ld:Vehicle:snap-get-{}", uuid::Uuid::new_v4());
        let entity = serde_json::json!({
            "id": eid,
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 55}
        });
        seed_snapshot_entities(&pool, &snap_id, &[(eid.clone(), entity)]).await;

        let params = QueryParams::new();
        let headers = RequestHeaders::new().snapshot(&snap_id);
        let resp = ctx.client.get_entity(&eid, &params, headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let snap_header = resp.headers.get("NGSILD-Snapshot").and_then(|v| v.to_str().ok());
        assert_eq!(snap_header, Some(snap_id.as_str()));

        let body: serde_json::Value = resp.json();
        assert_eq!(body["id"], eid);
        assert_eq!(body["type"], "Vehicle");

        pool.close().await;
    }));

    // ── 3. Retrieve entity not in snapshot returns 404 ───────────────
    trials.push(trial(&format!("{PREFIX}::retrieve_entity_not_in_snapshot_returns_404"), |ctx| async move {
        let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

        let body = serde_json::json!({"snapshotLifetime": "PT1H"});
        let resp = ctx.client.create_snapshot(&body).await;
        let created: serde_json::Value = resp.json();
        let snap_id = created["id"].as_str().unwrap().to_string();
        ctx.track_snapshot(&snap_id);

        // Seed one entity, but query for a different one
        let eid = format!("urn:ngsi-ld:Vehicle:snap-exists-{}", uuid::Uuid::new_v4());
        let entity = serde_json::json!({"id": eid, "type": "Vehicle"});
        seed_snapshot_entities(&pool, &snap_id, &[(eid, entity)]).await;

        // Also create a live entity that should NOT be visible via snapshot
        let live_eid = ctx.entity_id("urn:ngsi-ld:Vehicle");
        let live_entity = serde_json::json!({
            "id": live_eid,
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 99}
        });
        ctx.client.create_entity(&live_entity).await;

        let params = QueryParams::new();
        let headers = RequestHeaders::new().snapshot(&snap_id);
        let resp = ctx.client.get_entity(&live_eid, &params, headers).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND, "live entity should not be found in snapshot");

        pool.close().await;
    }));

    // ── 4. Nonexistent snapshot returns 404 ──────────────────────────
    trials.push(trial(&format!("{PREFIX}::nonexistent_snapshot_returns_404"), |ctx| async move {
        let params = QueryParams::new().type_("Vehicle");
        let headers = RequestHeaders::new().snapshot("urn:ngsi-ld:Snapshot:doesnotexist");
        let resp = ctx.client.query_entities(&params, headers).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // ── 5. Snapshot forces local scope ───────────────────────────────
    trials.push(trial(&format!("{PREFIX}::snapshot_forces_local_scope"), |ctx| async move {
        let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

        // Create snapshot with one entity
        let body = serde_json::json!({"snapshotLifetime": "PT1H"});
        let resp = ctx.client.create_snapshot(&body).await;
        let created: serde_json::Value = resp.json();
        let snap_id = created["id"].as_str().unwrap().to_string();
        ctx.track_snapshot(&snap_id);

        let snap_eid = format!("urn:ngsi-ld:Vehicle:snap-local-{}", uuid::Uuid::new_v4());
        let snap_entity = serde_json::json!({
            "id": snap_eid,
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 42}
        });
        seed_snapshot_entities(&pool, &snap_id, &[(snap_eid.clone(), snap_entity)]).await;

        // Create a live entity NOT in the snapshot
        let live_eid = ctx.entity_id("urn:ngsi-ld:Vehicle");
        let live_entity = serde_json::json!({
            "id": live_eid,
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 200}
        });
        ctx.client.create_entity(&live_entity).await;

        // Query with snapshot header: only snapshot entities should be returned
        let params = QueryParams::new().type_("Vehicle");
        let headers = RequestHeaders::new().snapshot(&snap_id);
        let resp = ctx.client.query_entities(&params, headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let entities: Vec<serde_json::Value> = resp.json();
        let ids: Vec<&str> = entities.iter().filter_map(|e| e["id"].as_str()).collect();
        assert!(ids.contains(&snap_eid.as_str()), "snapshot entity should be present");
        assert!(!ids.contains(&live_eid.as_str()), "live entity should NOT be present in snapshot-scoped query");

        pool.close().await;
    }));

    // ── 6. Snapshot query with type filter ───────────────────────────
    trials.push(trial(&format!("{PREFIX}::snapshot_query_with_type_filter"), |ctx| async move {
        let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

        let body = serde_json::json!({"snapshotLifetime": "PT1H"});
        let resp = ctx.client.create_snapshot(&body).await;
        let created: serde_json::Value = resp.json();
        let snap_id = created["id"].as_str().unwrap().to_string();
        ctx.track_snapshot(&snap_id);

        let vehicle_id = format!("urn:ngsi-ld:Vehicle:snap-type-{}", uuid::Uuid::new_v4());
        let building_id = format!("urn:ngsi-ld:Building:snap-type-{}", uuid::Uuid::new_v4());
        let vehicle = serde_json::json!({"id": vehicle_id, "type": "Vehicle", "speed": {"type": "Property", "value": 60}});
        let building = serde_json::json!({"id": building_id, "type": "Building", "name": {"type": "Property", "value": "HQ"}});
        seed_snapshot_entities(&pool, &snap_id, &[(vehicle_id.clone(), vehicle), (building_id.clone(), building)]).await;

        // Query only Vehicle type
        let params = QueryParams::new().type_("Vehicle");
        let headers = RequestHeaders::new().snapshot(&snap_id);
        let resp = ctx.client.query_entities(&params, headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let entities: Vec<serde_json::Value> = resp.json();
        assert_eq!(entities.len(), 1, "should return only Vehicle entities");
        assert_eq!(entities[0]["id"], vehicle_id);

        pool.close().await;
    }));

    // ── 7. Snapshot query with attrs filter ──────────────────────────
    trials.push(trial(&format!("{PREFIX}::snapshot_query_with_attrs_filter"), |ctx| async move {
        let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

        let body = serde_json::json!({"snapshotLifetime": "PT1H"});
        let resp = ctx.client.create_snapshot(&body).await;
        let created: serde_json::Value = resp.json();
        let snap_id = created["id"].as_str().unwrap().to_string();
        ctx.track_snapshot(&snap_id);

        let eid = format!("urn:ngsi-ld:Vehicle:snap-attrs-{}", uuid::Uuid::new_v4());
        let entity = serde_json::json!({
            "id": eid,
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 80},
            "fuelLevel": {"type": "Property", "value": 50}
        });
        seed_snapshot_entities(&pool, &snap_id, &[(eid.clone(), entity)]).await;

        // Retrieve with attrs=speed
        let params = QueryParams::new().attrs(vec!["speed"]);
        let headers = RequestHeaders::new().snapshot(&snap_id);
        let resp = ctx.client.get_entity(&eid, &params, headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert!(body.get("speed").is_some(), "speed attribute should be present");
        assert!(body.get("fuelLevel").is_none(), "fuelLevel should be filtered out by attrs");
        assert_eq!(body["id"], eid, "id must always be present");
        assert_eq!(body["type"], "Vehicle", "type must always be present");

        pool.close().await;
    }));

    // ── 8. Query temporal entities scoped to snapshot ─────────────────
    trials.push(trial(&format!("{PREFIX}::query_temporal_scoped_to_snapshot"), |ctx| async move {
        let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

        let body = serde_json::json!({"snapshotLifetime": "PT1H"});
        let resp = ctx.client.create_snapshot(&body).await;
        let created: serde_json::Value = resp.json();
        let snap_id = created["id"].as_str().unwrap().to_string();
        ctx.track_snapshot(&snap_id);

        // Set snapshot to success for scoping
        sqlx::query("UPDATE snapshots SET status = 'success' WHERE id = $1")
            .bind(&snap_id)
            .execute(&pool)
            .await
            .unwrap();

        let eid = format!("urn:ngsi-ld:Vehicle:snap-temp-{}", uuid::Uuid::new_v4());
        let temporal = serde_json::json!({
            "id": eid,
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 80, "observedAt": "2024-01-01T00:00:00Z"},
                {"type": "Property", "value": 90, "observedAt": "2024-01-01T01:00:00Z"}
            ]
        });
        seed_snapshot_temporal_entities(&pool, &snap_id, &[(eid.clone(), temporal)]).await;

        let params = QueryParams::new().type_("Vehicle");
        let headers = RequestHeaders::new().snapshot(&snap_id);
        let resp = ctx.client.query_temporal_entities(&params, headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let snap_header = resp.headers.get("NGSILD-Snapshot").and_then(|v| v.to_str().ok());
        assert_eq!(snap_header, Some(snap_id.as_str()), "NGSILD-Snapshot header must be echoed");

        let entities: Vec<serde_json::Value> = resp.json();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0]["id"], eid);

        pool.close().await;
    }));

    // ── 9. Retrieve temporal entity scoped to snapshot ────────────────
    trials.push(trial(&format!("{PREFIX}::retrieve_temporal_entity_scoped_to_snapshot"), |ctx| async move {
        let pool = PgPool::connect(DATABASE_URL).await.expect("db connect");

        let body = serde_json::json!({"snapshotLifetime": "PT1H"});
        let resp = ctx.client.create_snapshot(&body).await;
        let created: serde_json::Value = resp.json();
        let snap_id = created["id"].as_str().unwrap().to_string();
        ctx.track_snapshot(&snap_id);

        sqlx::query("UPDATE snapshots SET status = 'success' WHERE id = $1")
            .bind(&snap_id)
            .execute(&pool)
            .await
            .unwrap();

        let eid = format!("urn:ngsi-ld:Vehicle:snap-tget-{}", uuid::Uuid::new_v4());
        let temporal = serde_json::json!({
            "id": eid,
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 70, "observedAt": "2024-06-01T12:00:00Z"}
            ]
        });
        seed_snapshot_temporal_entities(&pool, &snap_id, &[(eid.clone(), temporal)]).await;

        let params = QueryParams::new();
        let headers = RequestHeaders::new().snapshot(&snap_id);
        let resp = ctx.client.get_temporal_entity(&eid, &params, headers).await;
        assert_eq!(resp.status, StatusCode::OK);

        let snap_header = resp.headers.get("NGSILD-Snapshot").and_then(|v| v.to_str().ok());
        assert_eq!(snap_header, Some(snap_id.as_str()));

        let body: serde_json::Value = resp.json();
        assert_eq!(body["id"], eid);

        pool.close().await;
    }));

    // ── 10. Temporal nonexistent snapshot returns 404 ─────────────────
    trials.push(trial(&format!("{PREFIX}::temporal_nonexistent_snapshot_returns_404"), |ctx| async move {
        let params = QueryParams::new().type_("Vehicle");
        let headers = RequestHeaders::new().snapshot("urn:ngsi-ld:Snapshot:nope");
        let resp = ctx.client.query_temporal_entities(&params, headers).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // ── 11. Empty snapshot header returns 400 ────────────────────────
    trials.push(trial(&format!("{PREFIX}::empty_snapshot_header_returns_400"), |ctx| async move {
        // Use raw reqwest to send an empty NGSILD-Snapshot header (the builder rejects empty values)
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{}/ngsi-ld/v1/entities?type=Vehicle", ctx.client.base_url))
            .header("NGSILD-Snapshot", "")
            .send()
            .await
            .expect("request failed");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }));

    // ── 12. Snapshot in preparing status returns 404 ──────────────────
    trials.push(trial(&format!("{PREFIX}::snapshot_in_preparing_status_returns_404"), |ctx| async move {
        // Create a snapshot (status defaults to "preparing")
        let body = serde_json::json!({"snapshotLifetime": "PT1H"});
        let resp = ctx.client.create_snapshot(&body).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let created: serde_json::Value = resp.json();
        let snap_id = created["id"].as_str().unwrap().to_string();
        ctx.track_snapshot(&snap_id);

        // Query with snapshot header -- should fail because status is "preparing"
        let params = QueryParams::new().type_("Vehicle");
        let headers = RequestHeaders::new().snapshot(&snap_id);
        let resp = ctx.client.query_entities(&params, headers).await;
        assert_eq!(
            resp.status,
            StatusCode::NOT_FOUND,
            "snapshot in preparing status should not be available for scoping"
        );
    }));
}
