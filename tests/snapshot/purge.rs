//! Snapshot purge, expiry, and resource-pressure acceptance cases.

use super::*;

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
    pretty_assertions::assert_eq!(
        purge(&client, "type==Snapshot;snapshotPriority==3").await.status(),
        StatusCode::NO_CONTENT
    );
    pretty_assertions::assert_eq!(
        purge(
            &client,
            &format!("id=={}|id==urn:ngsi-ld:Snapshot:does-not-exist", ids[3]),
        )
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
        pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
    }

    let missing = client.delete(broker_url("/snapshots")).send().await.expect("purge without q");
    pretty_assertions::assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
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
    pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound");
}
