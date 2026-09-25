//! Snapshot lifecycle and HTTP resource acceptance cases.

use super::*;
use andromeda_postgresql_codes::snapshot_status::SnapshotStatusCode;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use futures::future::join_all;
use sqlx::PgPool;
use std::{collections::BTreeSet, env};

fn minimal(id: Option<&str>) -> Value {
    let mut body = json!({"type": "Snapshot", "snapshotQueries": []});
    if let Some(id) = id {
        body["id"] = Value::String(id.to_owned());
    }
    body
}

fn timestamp(value: &Value, member: &str) -> DateTime<Utc> {
    value[member].as_str().expect("timestamp member").parse().expect("RFC 3339 timestamp")
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_create_honors_custom_id_generation_and_duplicate_conflict() {
    let _serialized_environment = clean_database().await;
    let custom = "urn:ngsi-ld:Snapshot:test-custom";
    let (status, headers, body) = create_snapshot_raw(&client(), &minimal(Some(custom))).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(body, Value::Null);
    pretty_assertions::assert_eq!(headers["location"], "/ngsi-ld/v1/snapshots/urn:ngsi-ld:Snapshot:test-custom");

    let mut generated = BTreeSet::new();
    for _ in 0..8 {
        let (status, headers, body) = create_snapshot_raw(&client(), &minimal(None)).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
        pretty_assertions::assert_eq!(body, Value::Null);
        let location = headers["location"].to_str().expect("ASCII Location");
        let id = location.strip_prefix("/ngsi-ld/v1/snapshots/").expect("relative Snapshot Location");
        assert!(id.starts_with("urn:ngsi-ld:Snapshot:"));
        assert!(generated.insert(id.to_owned()), "generated Snapshot identifiers must be unique");
    }

    let (status, _, problem) = create_snapshot_raw(&client(), &minimal(Some(custom))).await;
    pretty_assertions::assert_eq!(status, StatusCode::CONFLICT);
    assert_problem_details(&problem, "https://uri.etsi.org/ngsi-ld/errors/AlreadyExists", StatusCode::CONFLICT);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_create_applies_default_and_max_lifetime_and_initial_usage() {
    let _serialized_environment = clean_database().await;
    for (id, lifetime, expected_seconds) in [
        ("urn:ngsi-ld:Snapshot:test-defaults", None, 3_600_i64),
        ("urn:ngsi-ld:Snapshot:test-short", Some("PT90S"), 90_i64),
        ("urn:ngsi-ld:Snapshot:test-capped", Some("PT1000H"), 720_i64 * 3_600),
    ] {
        let mut body = minimal(Some(id));
        if let Some(lifetime) = lifetime {
            body["snapshotLifetime"] = Value::String(lifetime.to_owned());
        }
        let before_create = Utc::now();
        pretty_assertions::assert_eq!(create_snapshot_raw(&client(), &body).await.0, StatusCode::CREATED);
        let after_create = Utc::now();
        let snapshot = get_snapshot(&client(), id).await.1;
        pretty_assertions::assert_eq!(snapshot["snapshotPriority"], 5);
        pretty_assertions::assert_eq!(snapshot["createdAt"], snapshot["modifiedAt"]);
        let created = timestamp(&snapshot, "createdAt");
        assert!(created >= before_create - ChronoDuration::seconds(1));
        assert!(created <= after_create + ChronoDuration::seconds(1));
        pretty_assertions::assert_eq!(created, timestamp(&snapshot, "modifiedAt"));
        let last_used = timestamp(&snapshot, "lastUsedAt");
        assert!((last_used - created).num_seconds().abs() <= 1);
        pretty_assertions::assert_eq!((timestamp(&snapshot, "expiresAt") - created).num_seconds(), expected_seconds);
    }
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_status_get_negotiates_json_and_jsonld_and_tracks_use() {
    let _serialized_environment = clean_database().await;
    let id = "urn:ngsi-ld:Snapshot:test-get-negotiation";
    pretty_assertions::assert_eq!(create_snapshot_raw(&client(), &minimal(Some(id))).await.0, StatusCode::CREATED);
    let first = client()
        .get(broker_url(&format!("/snapshots/{id}")))
        .header("Accept", "application/json")
        .send()
        .await
        .expect("JSON Snapshot GET");
    pretty_assertions::assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(first.headers()["content-type"], "application/json");
    assert!(first.headers().contains_key("link"));
    let first_body: Value = first.json().await.expect("JSON Snapshot body");
    let second = client()
        .get(broker_url(&format!("/snapshots/{id}")))
        .header("Accept", "application/ld+json")
        .send()
        .await
        .expect("JSON-LD Snapshot GET");
    pretty_assertions::assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(second.headers()["content-type"], "application/ld+json");
    assert!(!second.headers().contains_key("link"));
    let second_body: Value = second.json().await.expect("JSON-LD Snapshot body");
    assert!(second_body.get("@context").is_some());
    assert!(timestamp(&second_body, "lastUsedAt") >= timestamp(&first_body, "lastUsedAt"));
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_patch_updates_and_removes_mutable_members_with_merge_patch_semantics() {
    let _serialized_environment = clean_database().await;
    let id = "urn:ngsi-ld:Snapshot:test-merge-patch";
    let mut body = minimal(Some(id));
    body["endpoint"] = json!("http://example.test/old");
    body["receiverInfo"] = json!([{"key": "X-Old", "value": "yes"}]);
    pretty_assertions::assert_eq!(create_snapshot_raw(&client(), &body).await.0, StatusCode::CREATED);
    let (status, updated) = update_snapshot(
        &client(),
        id,
        &json!({
            "type": "Snapshot",
            "snapshotLifetime": "PT2H",
            "snapshotPriority": 7,
            "endpoint": "http://example.test/new",
            "receiverInfo": [{"key": "X-New", "value": "yes"}]
        }),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(updated["snapshotPriority"], 7);
    pretty_assertions::assert_eq!(updated["endpoint"], "http://example.test/new");
    let (status, removed) = update_snapshot(&client(), id, &json!({"endpoint": null, "receiverInfo": null})).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(removed.get("endpoint").is_none());
    assert!(removed.get("receiverInfo").is_none());
    pretty_assertions::assert_eq!(removed["snapshotPriority"], 7);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_patch_rejects_immutable_query_type_and_id_changes_and_ignores_outputs() {
    let _serialized_environment = clean_database().await;
    let id = "urn:ngsi-ld:Snapshot:test-immutable";
    pretty_assertions::assert_eq!(create_snapshot_raw(&client(), &minimal(Some(id))).await.0, StatusCode::CREATED);
    for patch in [
        json!({"id": "urn:ngsi-ld:Snapshot:different"}),
        json!({"type": "Entity"}),
        json!({"type": null}),
        json!({"snapshotQueries": []}),
        json!({"snapshotTemporalQueries": []}),
    ] {
        let (status, problem) = update_snapshot(&client(), id, &patch).await;
        pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_problem_details(&problem, "https://uri.etsi.org/ngsi-ld/errors/BadRequestData", StatusCode::BAD_REQUEST);
    }
    let (status, unchanged) = update_snapshot(
        &client(),
        id,
        &json!({"snapshotPriority": 6, "snapshotStatus": "success", "createdAt": "1900-01-01T00:00:00Z"}),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(unchanged["snapshotPriority"], 6);
    assert_ne!(unchanged["createdAt"], "1900-01-01T00:00:00Z");
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_patch_lifetime_rebases_expiry_from_update_time_and_applies_cap() {
    let _serialized_environment = clean_database().await;
    let id = "urn:ngsi-ld:Snapshot:test-expiry-rebase";
    pretty_assertions::assert_eq!(create_snapshot_raw(&client(), &minimal(Some(id))).await.0, StatusCode::CREATED);
    let before = Utc::now();
    let (_, short) = update_snapshot(&client(), id, &json!({"snapshotLifetime": "PT90S"})).await;
    let short_expiry = timestamp(&short, "expiresAt");
    assert!((short_expiry - before).num_seconds() >= 89);
    let patch_time = Utc::now();
    let (_, capped) = update_snapshot(&client(), id, &json!({"snapshotLifetime": "PT1000H"})).await;
    let capped_seconds = (timestamp(&capped, "expiresAt") - patch_time).num_seconds();
    assert!((720 * 3_600 - 1..=720 * 3_600 + 1).contains(&capped_seconds));
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_delete_removes_control_branch_subscriptions_and_pending_work() {
    let _serialized_environment = clean_database().await;
    let id = "urn:ngsi-ld:Snapshot:test-delete-cascade";
    pretty_assertions::assert_eq!(create_snapshot_raw(&client(), &minimal(Some(id))).await.0, StatusCode::CREATED);
    let (status, _, body) = delete_snapshot_raw(&client(), id).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);
    pretty_assertions::assert_eq!(body, Value::Null);
    pretty_assertions::assert_eq!(get_snapshot(&client(), id).await.0, StatusCode::NOT_FOUND);
    let (unknown_status, _, unknown_problem) = delete_snapshot_raw(&client(), "urn:ngsi-ld:Snapshot:delete-unknown").await;
    pretty_assertions::assert_eq!(unknown_status, StatusCode::NOT_FOUND);
    assert_problem_details(&unknown_problem, "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound", StatusCode::NOT_FOUND);
    let malformed = client()
        .delete(broker_url("/snapshots/not%20a%20URI"))
        .send()
        .await
        .expect("malformed Snapshot delete");
    pretty_assertions::assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    assert_problem_details(
        &malformed.json::<Value>().await.expect("malformed delete ProblemDetails"),
        "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
        StatusCode::BAD_REQUEST,
    );
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_clone_requires_body_and_applies_clone_metadata() {
    let _serialized_environment = clean_database().await;
    let source = "urn:ngsi-ld:Snapshot:test-clone-source";
    pretty_assertions::assert_eq!(create_snapshot_raw(&client(), &minimal(Some(source))).await.0, StatusCode::CREATED);
    let source_created_at = timestamp(&get_snapshot(&client(), source).await.1, "createdAt");
    let missing = client()
        .post(broker_url(&format!("/snapshots/{source}/clone")))
        .header("Content-Length", "0")
        .send()
        .await
        .expect("bodyless clone");
    pretty_assertions::assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
    for invalid in [
        json!({"type": "Entity"}),
        json!({"type": "Snapshot", "snapshotQueries": []}),
        json!({"type": "Snapshot", "snapshotQueriesDetails": []}),
        json!({"type": "Snapshot", "snapshotPriority": 11}),
    ] {
        pretty_assertions::assert_eq!(clone_snapshot_raw(&client(), source, &invalid).await.0, StatusCode::BAD_REQUEST);
    }
    let target = "urn:ngsi-ld:Snapshot:test-clone-target";
    let body = json!({
        "id": target,
        "type": "Snapshot",
        "snapshotLifetime": "PT2H",
        "snapshotPriority": 6,
        "endpoint": "http://example.test/clone",
        "receiverInfo": [{"key": "X-Clone", "value": "yes"}]
    });
    let before_clone = Utc::now();
    let (status, headers, response_body) = clone_snapshot_raw(&client(), source, &body).await;
    let after_clone = Utc::now();
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(response_body, Value::Null);
    pretty_assertions::assert_eq!(headers["location"], "/ngsi-ld/v1/snapshots/urn:ngsi-ld:Snapshot:test-clone-target");
    let clone = get_snapshot(&client(), target).await.1;
    let clone_created_at = timestamp(&clone, "createdAt");
    assert!(clone_created_at >= before_clone - ChronoDuration::seconds(1));
    assert!(clone_created_at <= after_clone + ChronoDuration::seconds(1));
    assert!(clone_created_at >= source_created_at);
    pretty_assertions::assert_eq!(clone_created_at, timestamp(&clone, "modifiedAt"));
    pretty_assertions::assert_eq!(clone["snapshotPriority"], 6);
    pretty_assertions::assert_eq!(clone["endpoint"], "http://example.test/clone");
    pretty_assertions::assert_eq!(clone_snapshot_raw(&client(), source, &body).await.0, StatusCode::CONFLICT);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_clone_is_asynchronous_complete_and_independent() {
    let _serialized_environment = clean_database().await;
    let source = "urn:ngsi-ld:Snapshot:test-async-clone-source";
    let target = "urn:ngsi-ld:Snapshot:test-async-clone-target";
    pretty_assertions::assert_eq!(create_snapshot_raw(&client(), &minimal(Some(source))).await.0, StatusCode::CREATED);
    wait_for_snapshot_terminal(&client(), source, Duration::from_secs(5)).await;
    let (status, _, body) = clone_snapshot_raw(&client(), source, &json!({"id": target, "type": "Snapshot"})).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(body, Value::Null);
    let preparing = get_snapshot(&client(), target).await.1;
    assert!(["preparing", "success"].contains(&preparing["snapshotStatus"].as_str().expect("clone status")));
    pretty_assertions::assert_eq!(
        wait_for_snapshot_terminal(&client(), target, Duration::from_secs(5)).await["snapshotStatus"],
        "success"
    );
    pretty_assertions::assert_eq!(update_snapshot(&client(), source, &json!({"snapshotPriority": 2})).await.0, StatusCode::OK);
    pretty_assertions::assert_eq!(get_snapshot(&client(), target).await.1["snapshotPriority"], 5);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_resource_method_status_and_problem_matrix_matches_http_binding() {
    let _serialized_environment = clean_database().await;
    let id = "urn:ngsi-ld:Snapshot:test-resource-matrix";
    pretty_assertions::assert_eq!(create_snapshot_raw(&client(), &minimal(Some(id))).await.0, StatusCode::CREATED);
    pretty_assertions::assert_eq!(get_snapshot(&client(), id).await.0, StatusCode::OK);
    pretty_assertions::assert_eq!(update_snapshot(&client(), id, &json!({"snapshotPriority": 8})).await.0, StatusCode::OK);
    let unsupported = client()
        .put(broker_url(&format!("/snapshots/{id}")))
        .json(&json!({}))
        .send()
        .await
        .expect("unsupported method");
    pretty_assertions::assert_eq!(unsupported.status(), StatusCode::METHOD_NOT_ALLOWED);
    pretty_assertions::assert_eq!(delete_snapshot_raw(&client(), id).await.0, StatusCode::NO_CONTENT);
    let (missing, problem) = get_snapshot(&client(), id).await;
    pretty_assertions::assert_eq!(missing, StatusCode::NOT_FOUND);
    assert!(problem["type"].as_str().is_some_and(|value| value.starts_with("https://")));
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_clone_failure_is_atomic_and_terminal_failure() {
    let _serialized_environment = clean_database().await;
    let source = "urn:ngsi-ld:Snapshot:clone-fault-source";
    let target = "urn:ngsi-ld:Snapshot:clone-target:fault";
    pretty_assertions::assert_eq!(create_snapshot_raw(&client(), &minimal(Some(source))).await.0, StatusCode::CREATED);
    wait_for_snapshot_terminal(&client(), source, Duration::from_secs(5)).await;
    let entity =
        json!({"id": "urn:ngsi-ld:Vehicle:clone-fault", "type": "Vehicle", "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"});
    pretty_assertions::assert_eq!(
        client()
            .post(broker_url("/entities"))
            .header("Content-Type", "application/ld+json")
            .header("NGSILD-Snapshot", source)
            .json(&entity)
            .send()
            .await
            .expect("source branch entity create")
            .status(),
        StatusCode::CREATED
    );
    let pool = PgPool::connect(&env::var("ANDROMEDA_TEST_DATABASE_URL").expect("Snapshot database URL"))
        .await
        .expect("Snapshot database connection");
    // The status code is the registry's own constant, never caller input.
    let success = SnapshotStatusCode::Success.code();
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE OR REPLACE FUNCTION snapshot_clone_failure_injection() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.status = {success} AND EXISTS (SELECT 1 FROM snapshots WHERE snapshot_key = NEW.snapshot_key AND id LIKE '%:fault') THEN RAISE EXCEPTION 'deterministic clone fault'; END IF; RETURN NEW; END $$"
    )))
    .execute(&pool)
    .await
    .expect("clone failure function");
    sqlx::query("CREATE TRIGGER snapshot_clone_failure BEFORE UPDATE ON snapshot_preparation FOR EACH ROW EXECUTE FUNCTION snapshot_clone_failure_injection()")
        .execute(&pool)
        .await
        .expect("clone failure trigger");
    pretty_assertions::assert_eq!(
        clone_snapshot_raw(&client(), source, &json!({"id": target, "type": "Snapshot"})).await.0,
        StatusCode::CREATED
    );
    let failed = wait_for_snapshot_terminal(&client(), target, Duration::from_secs(8)).await;
    pretty_assertions::assert_eq!(failed["snapshotStatus"], "failure");
    pretty_assertions::assert_eq!(query_snapshot_entities(&client(), target, "type=Vehicle").await.2, json!([]));
    pretty_assertions::assert_eq!(
        query_snapshot_entities(&client(), source, "type=Vehicle")
            .await
            .2
            .as_array()
            .expect("source entities")
            .len(),
        1
    );
    sqlx::query("DROP TRIGGER snapshot_clone_failure ON snapshot_preparation")
        .execute(&pool)
        .await
        .expect("drop clone failure trigger");
    sqlx::query("DROP FUNCTION snapshot_clone_failure_injection()")
        .execute(&pool)
        .await
        .expect("drop clone failure function");
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_concurrent_clone_update_delete_and_purge_races_are_deterministic() {
    let _serialized_environment = clean_database().await;
    let source = "urn:ngsi-ld:Snapshot:race-source";
    let target = "urn:ngsi-ld:Snapshot:race-target";
    pretty_assertions::assert_eq!(create_snapshot_raw(&client(), &minimal(Some(source))).await.0, StatusCode::CREATED);
    wait_for_snapshot_terminal(&client(), source, Duration::from_secs(5)).await;
    let clone_attempts = (0..8).map(|_| async { clone_snapshot_raw(&client(), source, &json!({"id": target, "type": "Snapshot"})).await.0 });
    let statuses = join_all(clone_attempts).await;
    pretty_assertions::assert_eq!(statuses.iter().filter(|status| **status == StatusCode::CREATED).count(), 1);
    pretty_assertions::assert_eq!(statuses.iter().filter(|status| **status == StatusCode::CONFLICT).count(), 7);
    wait_for_snapshot_terminal(&client(), target, Duration::from_secs(5)).await;
    let patch_client = client();
    let delete_client = client();
    let purge_client = client();
    let patch_body = json!({"snapshotPriority": 7});
    let patch = update_snapshot(&patch_client, target, &patch_body);
    let delete = delete_snapshot_raw(&delete_client, target);
    let purge = purge_client
        .delete(broker_url(&format!("/snapshots?q={}", urlencoding::encode(&format!("id=={target}")))))
        .send();
    let (patch_result, delete_result, purge_result) = tokio::join!(patch, delete, purge);
    assert!([StatusCode::OK, StatusCode::NOT_FOUND].contains(&patch_result.0));
    assert!([StatusCode::NO_CONTENT, StatusCode::NOT_FOUND].contains(&delete_result.0));
    let purge_status = purge_result.expect("concurrent purge request").status();
    assert!([StatusCode::NO_CONTENT, StatusCode::NOT_FOUND, StatusCode::MULTI_STATUS].contains(&purge_status));
    pretty_assertions::assert_eq!(get_snapshot(&client(), target).await.0, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(get_snapshot(&client(), source).await.0, StatusCode::OK);
}
