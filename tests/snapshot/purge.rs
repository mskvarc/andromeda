//! Snapshot purge, expiry, and resource-pressure acceptance cases.

use super::*;
use sqlx::PgPool;
use std::env;

fn snapshot(id: &str, priority: u8, endpoint: Option<&str>) -> Value {
    let mut value = json!({
        "id": id,
        "type": "Snapshot",
        "snapshotQueries": [],
        "snapshotPriority": priority
    });
    if let Some(endpoint) = endpoint {
        value["endpoint"] = Value::String(endpoint.to_owned());
    }
    value
}

async fn purge(client: &reqwest::Client, expression: &str) -> reqwest::Response {
    client
        .delete(broker_url(&format!("/snapshots?q={}", urlencoding::encode(expression))))
        .send()
        .await
        .expect("Snapshot purge request")
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_purge_q_accepts_only_unambiguous_table_5_2_41_1_members() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let ids = [
        "urn:ngsi-ld:Snapshot:purge-id",
        "urn:ngsi-ld:Snapshot:purge-endpoint",
        "urn:ngsi-ld:Snapshot:purge-priority",
        "urn:ngsi-ld:Snapshot:purge-or",
        "urn:ngsi-ld:Snapshot:purge-survivor",
    ];
    let inputs = [
        snapshot(ids[0], 1, None),
        snapshot(ids[1], 2, Some("https://example.org/purge-target")),
        snapshot(ids[2], 3, None),
        snapshot(ids[3], 4, None),
        snapshot(ids[4], 5, Some("https://example.org/survivor")),
    ];
    for input in inputs {
        pretty_assertions::assert_eq!(create_snapshot_raw(&client, &input).await.0, StatusCode::CREATED);
    }

    pretty_assertions::assert_eq!(purge(&client, &format!("id=={}", ids[0])).await.status(), StatusCode::NO_CONTENT);
    pretty_assertions::assert_eq!(
        purge(&client, "endpoint==\"https://example.org/purge-target\"").await.status(),
        StatusCode::NO_CONTENT
    );
    pretty_assertions::assert_eq!(purge(&client, "type==Snapshot;snapshotPriority==3").await.status(), StatusCode::NO_CONTENT);
    pretty_assertions::assert_eq!(
        purge(&client, &format!("id=={}|id==urn:ngsi-ld:Snapshot:does-not-exist", ids[3]),)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    pretty_assertions::assert_eq!(purge(&client, "type!=Snapshot").await.status(), StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(purge(&client, "snapshotPriority!=5").await.status(), StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(get_snapshot(&client, ids[4]).await.0, StatusCode::OK);

    for invalid in [
        "snapshotStatus==success",
        "createdAt==2026-01-01T00:00:00Z",
        "modifiedAt==2026-01-01T00:00:00Z",
        "expiresAt==2026-01-01T00:00:00Z",
        "lastUsedAt==2026-01-01T00:00:00Z",
        "snapshotQueries==[]",
        "snapshotTemporalQueries==[]",
        "receiverInfo.key==X-Test",
        "snapshotLifetime>PT1H",
        "snapshotPriority>5",
        "id<urn:ngsi-ld:Snapshot:z",
        "type>=Snapshot",
        "endpoint!=not-a-url",
        "snapshotPriority==1.5",
        "snapshotPriority==0",
        "snapshotPriority==11",
        "unknown==value",
        "type==Snapshot;snapshotPriority==5|id==urn:ngsi-ld:Snapshot:any",
    ] {
        let response = purge(&client, invalid).await;
        pretty_assertions::assert_eq!(response.status(), StatusCode::BAD_REQUEST, "expression: {invalid}");
        pretty_assertions::assert_eq!(response.headers()["content-type"], "application/json");
        let problem: Value = response.json().await.expect("purge ProblemDetails");
        assert_problem_details(&problem, "https://uri.etsi.org/ngsi-ld/errors/BadRequestData", StatusCode::BAD_REQUEST);
    }

    let missing = client.delete(broker_url("/snapshots")).send().await.expect("purge without q");
    pretty_assertions::assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
    assert_problem_details(
        &missing.json::<Value>().await.expect("missing purge q ProblemDetails"),
        "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
        StatusCode::BAD_REQUEST,
    );
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_purge_full_success_and_no_match_have_exact_http_results() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let matching = ["urn:ngsi-ld:Snapshot:purge-full-a", "urn:ngsi-ld:Snapshot:purge-full-b"];
    let survivor = "urn:ngsi-ld:Snapshot:purge-full-survivor";
    for id in matching {
        pretty_assertions::assert_eq!(create_snapshot_raw(&client, &snapshot(id, 4, None)).await.0, StatusCode::CREATED);
    }
    pretty_assertions::assert_eq!(create_snapshot_raw(&client, &snapshot(survivor, 8, None)).await.0, StatusCode::CREATED);

    let deleted = purge(&client, "snapshotPriority==4").await;
    pretty_assertions::assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    assert!(!deleted.headers().contains_key("content-length"));
    pretty_assertions::assert_eq!(deleted.bytes().await.expect("empty purge response").len(), 0);
    for id in matching {
        pretty_assertions::assert_eq!(get_snapshot(&client, id).await.0, StatusCode::NOT_FOUND);
    }
    pretty_assertions::assert_eq!(get_snapshot(&client, survivor).await.0, StatusCode::OK);

    let no_match = purge(&client, "snapshotPriority==4").await;
    pretty_assertions::assert_eq!(no_match.status(), StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(no_match.headers()["content-type"], "application/json");
    let problem: Value = no_match.json().await.expect("purge no-match ProblemDetails");
    assert_problem_details(&problem, "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound", StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_purge_partial_and_all_failed_return_exact_207_results() {
    let _serialized_environment = clean_database().await;
    let client = client();
    let pool = PgPool::connect(&env::var("ANDROMEDA_TEST_DATABASE_URL").expect("Snapshot database URL"))
        .await
        .expect("Snapshot database connection");
    sqlx::query(
        "CREATE OR REPLACE FUNCTION snapshot_purge_failure_injection() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF OLD.id LIKE '%:fault' THEN RAISE EXCEPTION 'deterministic purge fault'; END IF; RETURN OLD; END $$",
    )
    .execute(&pool)
    .await
    .expect("purge failure function");
    sqlx::query("CREATE TRIGGER snapshot_purge_failure BEFORE DELETE ON snapshots FOR EACH ROW EXECUTE FUNCTION snapshot_purge_failure_injection()")
        .execute(&pool)
        .await
        .expect("purge failure trigger");

    for id in ["urn:ngsi-ld:Snapshot:partial-ok", "urn:ngsi-ld:Snapshot:partial:fault"] {
        pretty_assertions::assert_eq!(create_snapshot_raw(&client, &snapshot(id, 8, None)).await.0, StatusCode::CREATED);
    }
    let partial = purge(&client, "snapshotPriority==8").await;
    pretty_assertions::assert_eq!(partial.status(), StatusCode::MULTI_STATUS);
    pretty_assertions::assert_eq!(partial.headers()["content-type"], "application/json");
    let partial_body: Value = partial.json().await.expect("partial purge result");
    pretty_assertions::assert_eq!(partial_body["success"], json!(["urn:ngsi-ld:Snapshot:partial-ok"]));
    pretty_assertions::assert_eq!(partial_body["errors"].as_array().expect("partial errors").len(), 1);
    pretty_assertions::assert_eq!(partial_body["errors"][0]["entityId"], "urn:ngsi-ld:Snapshot:partial:fault");

    for id in ["urn:ngsi-ld:Snapshot:all-a:fault", "urn:ngsi-ld:Snapshot:all-b:fault"] {
        pretty_assertions::assert_eq!(create_snapshot_raw(&client, &snapshot(id, 9, None)).await.0, StatusCode::CREATED);
    }
    let all_failed = purge(&client, "snapshotPriority==9").await;
    pretty_assertions::assert_eq!(all_failed.status(), StatusCode::MULTI_STATUS);
    let all_failed_body: Value = all_failed.json().await.expect("all-failed purge result");
    pretty_assertions::assert_eq!(all_failed_body["success"], json!([]));
    pretty_assertions::assert_eq!(all_failed_body["errors"].as_array().expect("all-failed errors").len(), 2);

    sqlx::query("DROP TRIGGER snapshot_purge_failure ON snapshots")
        .execute(&pool)
        .await
        .expect("drop purge failure trigger");
    sqlx::query("DROP FUNCTION snapshot_purge_failure_injection()")
        .execute(&pool)
        .await
        .expect("drop purge failure function");
}
