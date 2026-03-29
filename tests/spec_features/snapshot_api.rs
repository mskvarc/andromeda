/// Feature 10: Snapshot API (spec 5.16, 6.36-6.38)
///
/// Verifies the full Snapshot lifecycle: create, retrieve status, update,
/// clone, delete, and purge.
use crate::harness::{self, trial::trial};
use libtest_mimic::Trial;
use reqwest::StatusCode;

const PREFIX: &str = "spec_features::snapshot_api";

pub fn tests(trials: &mut Vec<Trial>) {
    // ── Create snapshot returns 201 with Location header ──────────
    trials.push(trial(&format!("{PREFIX}::create_snapshot"), |ctx| async move {
        let body = serde_json::json!({
            "snapshotLifetime": "PT1H",
            "snapshotPriority": 5
        });
        let resp = ctx.client.create_snapshot(&body).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let location = resp.location().expect("should have Location header");
        assert!(location.contains("/snapshots/"), "Location should contain /snapshots/");

        let body: serde_json::Value = resp.json();
        assert_eq!(body["status"], "preparing");
        assert_eq!(body["snapshotPriority"], 5);
        let snap_id = body["id"].as_str().unwrap();
        ctx.track_snapshot(snap_id);
    }));

    // ── Create snapshot with custom ID ────────────────────────────
    trials.push(trial(&format!("{PREFIX}::create_snapshot_with_custom_id"), |ctx| async move {
        let custom_id = format!("urn:ngsi-ld:Snapshot:custom-{}", uuid::Uuid::new_v4());
        let body = serde_json::json!({
            "id": custom_id,
            "snapshotLifetime": "PT30M"
        });
        let resp = ctx.client.create_snapshot(&body).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp_body: serde_json::Value = resp.json();
        assert_eq!(resp_body["id"], custom_id);
        ctx.track_snapshot(&custom_id);
    }));

    // ── Create duplicate returns AlreadyExists ────────────────────
    trials.push(trial(&format!("{PREFIX}::create_duplicate_snapshot_rejected"), |ctx| async move {
        let custom_id = format!("urn:ngsi-ld:Snapshot:dup-{}", uuid::Uuid::new_v4());
        let body = serde_json::json!({
            "id": custom_id,
            "snapshotLifetime": "PT1H"
        });
        let resp = ctx.client.create_snapshot(&body).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_snapshot(&custom_id);

        // Second creation with same ID
        let resp = ctx.client.create_snapshot(&body).await;
        assert_eq!(resp.status, StatusCode::CONFLICT);
    }));

    // ── Get snapshot status ───────────────────────────────────────
    trials.push(trial(&format!("{PREFIX}::get_snapshot_status"), |ctx| async move {
        let body = serde_json::json!({"snapshotLifetime": "PT1H"});
        let resp = ctx.client.create_snapshot(&body).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let created: serde_json::Value = resp.json();
        let snap_id = created["id"].as_str().unwrap().to_string();
        ctx.track_snapshot(&snap_id);

        let resp = ctx.client.get_snapshot(&snap_id).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["id"], snap_id);
        assert!(body.get("createdAt").is_some(), "should have createdAt");
        assert!(body.get("expiresAt").is_some(), "should have expiresAt");
    }));

    // ── Get non-existent snapshot returns 404 ─────────────────────
    trials.push(trial(&format!("{PREFIX}::get_nonexistent_snapshot"), |ctx| async move {
        let resp = ctx.client.get_snapshot("urn:ngsi-ld:Snapshot:doesnotexist").await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // ── Update snapshot lifetime/priority ─────────────────────────
    trials.push(trial(&format!("{PREFIX}::update_snapshot"), |ctx| async move {
        let body = serde_json::json!({
            "snapshotLifetime": "PT1H",
            "snapshotPriority": 3
        });
        let resp = ctx.client.create_snapshot(&body).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let created: serde_json::Value = resp.json();
        let snap_id = created["id"].as_str().unwrap().to_string();
        ctx.track_snapshot(&snap_id);

        let patch = serde_json::json!({
            "snapshotLifetime": "PT2H",
            "snapshotPriority": 8
        });
        let resp = ctx.client.update_snapshot(&snap_id, &patch).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["snapshotPriority"], 8);
    }));

    // ── Update with invalid priority rejected ─────────────────────
    trials.push(trial(&format!("{PREFIX}::update_invalid_priority_rejected"), |ctx| async move {
        let body = serde_json::json!({"snapshotLifetime": "PT1H"});
        let resp = ctx.client.create_snapshot(&body).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let created: serde_json::Value = resp.json();
        let snap_id = created["id"].as_str().unwrap().to_string();
        ctx.track_snapshot(&snap_id);

        let patch = serde_json::json!({"snapshotPriority": 99});
        let resp = ctx.client.update_snapshot(&snap_id, &patch).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // ── Delete snapshot ───────────────────────────────────────────
    trials.push(trial(&format!("{PREFIX}::delete_snapshot"), |ctx| async move {
        let body = serde_json::json!({"snapshotLifetime": "PT1H"});
        let resp = ctx.client.create_snapshot(&body).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let created: serde_json::Value = resp.json();
        let snap_id = created["id"].as_str().unwrap().to_string();

        let resp = ctx.client.delete_snapshot(&snap_id).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        // Should be gone
        let resp = ctx.client.get_snapshot(&snap_id).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // ── Delete non-existent returns 404 ───────────────────────────
    trials.push(trial(&format!("{PREFIX}::delete_nonexistent_returns_404"), |ctx| async move {
        let resp = ctx.client.delete_snapshot("urn:ngsi-ld:Snapshot:ghost").await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));

    // ── Clone snapshot ────────────────────────────────────────────
    trials.push(trial(&format!("{PREFIX}::clone_snapshot"), |ctx| async move {
        let body = serde_json::json!({
            "snapshotLifetime": "PT1H",
            "snapshotPriority": 7
        });
        let resp = ctx.client.create_snapshot(&body).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let created: serde_json::Value = resp.json();
        let snap_id = created["id"].as_str().unwrap().to_string();
        ctx.track_snapshot(&snap_id);

        let resp = ctx.client.clone_snapshot(&snap_id).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let cloned: serde_json::Value = resp.json();
        let cloned_id = cloned["id"].as_str().unwrap().to_string();
        ctx.track_snapshot(&cloned_id);

        assert_ne!(cloned_id, snap_id, "cloned should have different ID");
        assert_eq!(cloned["snapshotPriority"], 7, "should preserve priority");

        // Both should be retrievable
        let resp = ctx.client.get_snapshot(&snap_id).await;
        assert_eq!(resp.status, StatusCode::OK);
        let resp = ctx.client.get_snapshot(&cloned_id).await;
        assert_eq!(resp.status, StatusCode::OK);
    }));

    // ── Purge all snapshots ───────────────────────────────────────
    trials.push(trial(&format!("{PREFIX}::purge_snapshots"), |ctx| async move {
        // Create two snapshots
        for _ in 0..2 {
            let body = serde_json::json!({"snapshotLifetime": "PT1H"});
            let resp = ctx.client.create_snapshot(&body).await;
            assert_eq!(resp.status, StatusCode::CREATED);
        }

        let resp = ctx.client.purge_snapshots().await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        // No cleanup needed since we purged
    }));

    // ── Default priority is 5 ─────────────────────────────────────
    trials.push(trial(&format!("{PREFIX}::default_priority_is_5"), |ctx| async move {
        let body = serde_json::json!({"snapshotLifetime": "PT1H"});
        let resp = ctx.client.create_snapshot(&body).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let created: serde_json::Value = resp.json();
        assert_eq!(created["snapshotPriority"], 5);
        let snap_id = created["id"].as_str().unwrap();
        ctx.track_snapshot(snap_id);
    }));

    // ── Full lifecycle: create → get → update → clone → delete ───
    trials.push(trial(&format!("{PREFIX}::full_lifecycle"), |ctx| async move {
        // 1. Create
        let body = serde_json::json!({
            "snapshotLifetime": "PT1H",
            "snapshotPriority": 3
        });
        let resp = ctx.client.create_snapshot(&body).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let created: serde_json::Value = resp.json();
        let snap_id = created["id"].as_str().unwrap().to_string();

        // 2. Get
        let resp = ctx.client.get_snapshot(&snap_id).await;
        assert_eq!(resp.status, StatusCode::OK);
        let got: serde_json::Value = resp.json();
        assert_eq!(got["snapshotPriority"], 3);

        // 3. Update
        let patch = serde_json::json!({"snapshotPriority": 9});
        let resp = ctx.client.update_snapshot(&snap_id, &patch).await;
        assert_eq!(resp.status, StatusCode::OK);
        let updated: serde_json::Value = resp.json();
        assert_eq!(updated["snapshotPriority"], 9);

        // 4. Clone
        let resp = ctx.client.clone_snapshot(&snap_id).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let cloned: serde_json::Value = resp.json();
        let cloned_id = cloned["id"].as_str().unwrap().to_string();
        assert_eq!(cloned["snapshotPriority"], 9, "clone inherits updated priority");

        // 5. Delete both
        let resp = ctx.client.delete_snapshot(&snap_id).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);
        let resp = ctx.client.delete_snapshot(&cloned_id).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        // 6. Verify gone
        let resp = ctx.client.get_snapshot(&snap_id).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
        let resp = ctx.client.get_snapshot(&cloned_id).await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND);
    }));
}
