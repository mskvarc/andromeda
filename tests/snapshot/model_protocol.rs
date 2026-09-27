//! Snapshot model and protocol acceptance cases.

use crate::{
    notification_adapter::NotificationAdapter,
    support::{
        assert_snapshot_create_rejected,
        broker_url,
        clean_database,
        client,
        create_entity,
        create_snapshot_raw,
        delete_snapshot_raw,
        get_snapshot,
        update_snapshot,
        wait_for_snapshot_terminal,
    },
};
use reqwest::StatusCode;
use serde_json::{Value, json};
use serial_test::serial;
use std::time::Duration;
use tokio::{task::yield_now, time::timeout};

const SNAPSHOT_ID: &str = "urn:ngsi-ld:Snapshot:test-model";

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_model_accepts_exact_create_shape_and_query_discriminators() {
    let _serialized_environment = clean_database().await;
    let body = json!({
        "id": SNAPSHOT_ID,
        "type": "Snapshot",
        "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}],
        "snapshotTemporalQueries": [{
            "type": "Query",
            "entities": [{"type": "Vehicle"}],
            "temporalQ": {"timerel": "between", "timeAt": "2025-01-01T00:00:00Z", "endTimeAt": "2025-01-02T00:00:00Z"}
        }],
        "snapshotLifetime": "PT1H",
        "snapshotPriority": 6,
        "endpoint": "http://example.test/snapshot-status",
        "receiverInfo": [{"key": "X-Receiver", "value": "model-case"}]
    });
    let (status, headers, response_body) = create_snapshot_raw(&client(), &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(response_body, Value::Null);
    pretty_assertions::assert_eq!(
        headers.get("location").and_then(|value| value.to_str().ok()),
        Some("/ngsi-ld/v1/snapshots/urn:ngsi-ld:Snapshot:test-model")
    );
    let (get_status, snapshot) = get_snapshot(&client(), SNAPSHOT_ID).await;
    pretty_assertions::assert_eq!(get_status, StatusCode::OK);
    pretty_assertions::assert_eq!(snapshot["type"], "Snapshot");
    pretty_assertions::assert_eq!(snapshot["snapshotQueries"][0]["type"], "Query");
    pretty_assertions::assert_eq!(snapshot["snapshotTemporalQueries"][0]["type"], "Query");
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_model_rejects_invalid_discriminators_members_and_cardinalities() {
    let _serialized_environment = clean_database().await;
    let invalid = [
        json!({"snapshotQueries": []}),
        json!({"type": "Entity", "snapshotQueries": []}),
        json!({"type": "Snapshot"}),
        json!({"type": "Snapshot", "snapshotQueries": [{"entities": [{"type": "Vehicle"}]}]}),
        json!({"type": "Snapshot", "snapshotQueries": [{"type": "Entity"}]}),
        json!({"type": "Snapshot", "snapshotQueries": [], "unknown": true}),
        json!({"id": "not a URI", "type": "Snapshot", "snapshotQueries": []}),
        json!({"type": "Snapshot", "snapshotQueries": {"type": "Query"}}),
    ];
    for body in invalid {
        assert_snapshot_create_rejected(&client(), body).await;
    }
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_model_enforces_entity_and_temporal_query_collection_rules() {
    let _serialized_environment = clean_database().await;
    for invalid in [
        json!({"type": "Snapshot", "snapshotQueries": [{"type": "Query", "temporalQ": {"timerel": "after", "timeAt": "2025-01-01T00:00:00Z"}}]}),
        json!({"type": "Snapshot", "snapshotTemporalQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]}),
    ] {
        assert_snapshot_create_rejected(&client(), invalid).await;
    }
    let present_empty = json!({"id": "urn:ngsi-ld:Snapshot:test-present-empty", "type": "Snapshot", "snapshotQueries": []});
    let (status, _, body) = create_snapshot_raw(&client(), &present_empty).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    pretty_assertions::assert_eq!(body, Value::Null);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_model_validates_lifetime_priority_endpoint_and_receiver_info() {
    let _serialized_environment = clean_database().await;
    for (suffix, priority) in [("low", 1), ("high", 10)] {
        let body = json!({
            "id": format!("urn:ngsi-ld:Snapshot:test-{suffix}"),
            "type": "Snapshot",
            "snapshotQueries": [],
            "snapshotLifetime": "PT30M",
            "snapshotPriority": priority,
            "endpoint": "http://example.test/status",
            "receiverInfo": [{"key": "X-Correlation", "value": suffix}]
        });
        pretty_assertions::assert_eq!(create_snapshot_raw(&client(), &body).await.0, StatusCode::CREATED);
    }
    for (suffix, member) in [
        ("priority-zero", json!({"snapshotPriority": 0})),
        ("priority-eleven", json!({"snapshotPriority": 11})),
        ("priority-fraction", json!({"snapshotPriority": 1.5})),
        ("lifetime", json!({"snapshotLifetime": "one hour"})),
        ("endpoint", json!({"endpoint": "not a URL"})),
        ("receiver", json!({"receiverInfo": [{"key": 4, "value": []}]})),
    ] {
        let mut body = json!({"id": format!("urn:ngsi-ld:Snapshot:test-{suffix}"), "type": "Snapshot", "snapshotQueries": []});
        body.as_object_mut()
            .expect("fixture object")
            .extend(member.as_object().expect("member object").clone());
        assert_snapshot_create_rejected(&client(), body).await;
    }
    let default_id = "urn:ngsi-ld:Snapshot:test-default-priority";
    pretty_assertions::assert_eq!(
        create_snapshot_raw(&client(), &json!({"id": default_id, "type": "Snapshot", "snapshotQueries": []}))
            .await
            .0,
        StatusCode::CREATED
    );
    pretty_assertions::assert_eq!(get_snapshot(&client(), default_id).await.1["snapshotPriority"], 5);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_model_ignores_forged_output_only_members_on_create_and_update() {
    let _serialized_environment = clean_database().await;
    let id = "urn:ngsi-ld:Snapshot:test-forged-output";
    let forged = json!({
        "id": id,
        "type": "Snapshot",
        "snapshotQueries": [],
        "snapshotStatus": "success",
        "snapshotQueriesDetails": [{"resultStatus": "failure"}],
        "snapshotTemporalQueriesDetails": [{"resultStatus": "failure"}],
        "createdAt": "1900-01-01T00:00:00Z",
        "modifiedAt": "1900-01-01T00:00:00Z",
        "expiresAt": "1900-01-01T00:00:00Z",
        "lastUsedAt": "1900-01-01T00:00:00Z"
    });
    pretty_assertions::assert_eq!(create_snapshot_raw(&client(), &forged).await.0, StatusCode::CREATED);
    let before = get_snapshot(&client(), id).await.1;
    assert_ne!(before["createdAt"], "1900-01-01T00:00:00Z");
    let (status, patched) = update_snapshot(
        &client(),
        id,
        &json!({
            "snapshotPriority": 7,
            "snapshotStatus": "failure",
            "createdAt": "1900-01-01T00:00:00Z",
            "expiresAt": "1900-01-01T00:00:00Z"
        }),
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(patched["snapshotPriority"], 7);
    assert_ne!(patched["createdAt"], "1900-01-01T00:00:00Z");
    assert_ne!(patched["expiresAt"], "1900-01-01T00:00:00Z");
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_model_renders_exact_status_detail_and_timestamp_members() {
    let _serialized_environment = clean_database().await;
    let id = "urn:ngsi-ld:Snapshot:test-wire-members";
    pretty_assertions::assert_eq!(
        create_snapshot_raw(&client(), &json!({"id": id, "type": "Snapshot", "snapshotQueries": []}))
            .await
            .0,
        StatusCode::CREATED
    );
    let preparing = get_snapshot(&client(), id).await.1;
    pretty_assertions::assert_eq!(preparing["snapshotStatus"], "preparing");
    for member in ["createdAt", "modifiedAt", "expiresAt", "lastUsedAt"] {
        assert!(preparing[member].as_str().is_some(), "{member} must be a rendered DateTime");
    }
    let terminal = wait_for_snapshot_terminal(&client(), id, Duration::from_secs(5)).await;
    assert!(["success", "partial", "empty", "failure"].contains(&terminal["snapshotStatus"].as_str().expect("status token")));
    assert!(terminal.get("preparation").is_none());
    assert!(terminal.get("queriesDetails").is_none());
    if terminal.get("snapshotQueriesDetails").is_some() {
        assert!(terminal["snapshotQueriesDetails"].is_array());
    }
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_execution_details_encode_success_empty_and_problem_failure() {
    let _serialized_environment = clean_database().await;
    let entity = json!({
        "id": "urn:ngsi-ld:Vehicle:result-details",
        "type": "https://example.org/ResultVehicle",
        "speed": {"type": "Property", "value": 10},
        "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"
    });
    pretty_assertions::assert_eq!(create_entity(&client(), &entity).await, StatusCode::CREATED);
    let registration = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:snapshot-failed-result",
        "type": "ContextSourceRegistration",
        "endpoint": "http://host.docker.internal:1",
        "information": [{"entities": [{"type": "https://example.org/FailedResultType"}]}],
        "operations": ["queryEntity"],
        "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"
    });
    let registration_status = client()
        .post(broker_url("/csourceRegistrations"))
        .header("Content-Type", "application/ld+json")
        .json(&registration)
        .send()
        .await
        .expect("failed-source registration request")
        .status();
    pretty_assertions::assert_eq!(registration_status, StatusCode::CREATED);
    let id = "urn:ngsi-ld:Snapshot:test-result-details";
    let body = json!({
        "id": id,
        "type": "Snapshot",
        "snapshotQueries": [
            {"type": "Query", "entities": [{"type": "https://example.org/ResultVehicle"}]},
            {"type": "Query", "entities": [{"type": "https://example.org/DefinitelyAbsentType"}]},
            {"type": "Query", "entities": [{"type": "https://example.org/FailedResultType"}]}
        ]
    });
    pretty_assertions::assert_eq!(create_snapshot_raw(&client(), &body).await.0, StatusCode::CREATED);
    let terminal = wait_for_snapshot_terminal(&client(), id, Duration::from_secs(5)).await;
    let details = terminal["snapshotQueriesDetails"].as_array().expect("ordered execution details");
    pretty_assertions::assert_eq!(details.len(), 3);
    pretty_assertions::assert_eq!(details[0]["resultStatus"], "success");
    pretty_assertions::assert_eq!(details[1]["resultStatus"], "empty");
    pretty_assertions::assert_eq!(details[2]["resultStatus"], "failure");
    assert!(details[0].get("problemDetails").is_none());
    assert!(details[1].get("problemDetails").is_none());
    assert!(details[2]["problemDetails"]["type"].as_str().is_some());
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_notification_model_uses_exact_terminal_wire_shape() {
    let _serialized_environment = clean_database().await;
    let first_receiver = NotificationAdapter::start().await;
    let second_receiver = NotificationAdapter::start().await;
    let entity = json!({
        "id": "urn:ngsi-ld:Vehicle:notification-result",
        "type": "https://example.org/NotificationResultVehicle",
        "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"
    });
    pretty_assertions::assert_eq!(create_entity(&client(), &entity).await, StatusCode::CREATED);
    let registration = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:snapshot-notification-failure",
        "type": "ContextSourceRegistration",
        "endpoint": "http://host.docker.internal:1",
        "information": [{"entities": [{"type": "https://example.org/NotificationFailureType"}]}],
        "operations": ["queryEntity"],
        "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"
    });
    pretty_assertions::assert_eq!(
        client()
            .post(broker_url("/csourceRegistrations"))
            .header("Content-Type", "application/ld+json")
            .json(&registration)
            .send()
            .await
            .expect("notification failure registration")
            .status(),
        StatusCode::CREATED
    );
    let cases = [
        (
            "urn:ngsi-ld:Snapshot:terminal-success",
            json!([{"type": "Query", "entities": [{"type": "https://example.org/NotificationResultVehicle"}]}]),
            &first_receiver,
            "first",
            "success",
        ),
        (
            "urn:ngsi-ld:Snapshot:terminal-empty",
            json!([{"type": "Query", "entities": [{"type": "https://example.org/NotificationAbsentType"}]}]),
            &second_receiver,
            "second",
            "empty",
        ),
        (
            "urn:ngsi-ld:Snapshot:terminal-partial",
            json!([
                {"type": "Query", "entities": [{"type": "https://example.org/NotificationResultVehicle"}]},
                {"type": "Query", "entities": [{"type": "https://example.org/NotificationFailureType"}]}
            ]),
            &first_receiver,
            "first",
            "partial",
        ),
        (
            "urn:ngsi-ld:Snapshot:terminal-failure",
            json!([{"type": "Query", "entities": [{"type": "https://example.org/NotificationFailureType"}]}]),
            &second_receiver,
            "second",
            "failure",
        ),
    ];
    for (id, queries, receiver, receiver_name, expected_status) in cases {
        let body = json!({
        "id": id,
        "type": "Snapshot",
        "snapshotQueries": queries,
        "endpoint": receiver.endpoint_url(),
        "receiverInfo": [{"key": "X-Snapshot-Test", "value": receiver_name}]
        });
        pretty_assertions::assert_eq!(create_snapshot_raw(&client(), &body).await.0, StatusCode::CREATED);
        let terminal = wait_for_snapshot_terminal(&client(), id, Duration::from_secs(8)).await;
        pretty_assertions::assert_eq!(terminal["snapshotStatus"], expected_status);
        let notification = receiver.wait_for_notification_matching("snapshotId", id, Duration::from_secs(10)).await;
        assert!(
            notification.body["id"]
                .as_str()
                .is_some_and(|value| value.starts_with("urn:ngsi-ld:SnapshotNotification:"))
        );
        pretty_assertions::assert_eq!(notification.body["type"], "SnapshotNotification");
        pretty_assertions::assert_eq!(notification.body["snapshotId"], id);
        pretty_assertions::assert_eq!(notification.body["snapshotPriority"], terminal["snapshotPriority"]);
        pretty_assertions::assert_eq!(notification.body["snapshotStatus"], expected_status);
        pretty_assertions::assert_eq!(notification.body["expiresAt"], terminal["expiresAt"]);
        assert!(notification.body["notifiedAt"].as_str().is_some());
        assert!(notification.body.get("snapshotReady").is_none());
        assert!(notification.body.get("@context").is_some());
        pretty_assertions::assert_eq!(notification.headers["content-type"], "application/ld+json");
        pretty_assertions::assert_eq!(notification.headers["X-Snapshot-Test"], receiver_name);
        assert!(notification.headers.get("NGSILD-Tenant").is_none());
    }

    let updated_id = "urn:ngsi-ld:Snapshot:terminal-success";
    let (patch_status, updated) = update_snapshot(&client(), updated_id, &json!({"snapshotPriority": 9, "snapshotLifetime": "PT2H"})).await;
    pretty_assertions::assert_eq!(patch_status, StatusCode::OK);
    let update_notification = first_receiver
        .wait_for_notification_matching("snapshotId", updated_id, Duration::from_secs(10))
        .await;
    pretty_assertions::assert_eq!(update_notification.body["snapshotPriority"], 9);
    pretty_assertions::assert_eq!(update_notification.body["expiresAt"], updated["expiresAt"]);
    pretty_assertions::assert_eq!(delete_snapshot_raw(&client(), updated_id).await.0, StatusCode::NO_CONTENT);
    first_receiver
        .wait_for_no_notification_matching("snapshotId", updated_id, Duration::from_millis(500))
        .await;

    let purged_id = "urn:ngsi-ld:Snapshot:terminal-empty";
    pretty_assertions::assert_eq!(
        client()
            .delete(broker_url(&format!("/snapshots?q={}", urlencoding::encode(&format!("id=={purged_id}")))))
            .send()
            .await
            .expect("purge endpoint-bearing Snapshot")
            .status(),
        StatusCode::NO_CONTENT
    );
    second_receiver
        .wait_for_no_notification_matching("snapshotId", purged_id, Duration::from_millis(500))
        .await;

    let expiring_id = "urn:ngsi-ld:Snapshot:terminal-expiry";
    pretty_assertions::assert_eq!(
        create_snapshot_raw(
            &client(),
            &json!({
                "id": expiring_id,
                "type": "Snapshot",
                "snapshotQueries": [],
                "snapshotLifetime": "PT1S",
                "endpoint": second_receiver.endpoint_url()
            })
        )
        .await
        .0,
        StatusCode::CREATED
    );
    wait_for_snapshot_terminal(&client(), expiring_id, Duration::from_secs(5)).await;
    second_receiver
        .wait_for_notification_matching("snapshotId", expiring_id, Duration::from_secs(10))
        .await;
    timeout(Duration::from_secs(8), async {
        loop {
            if get_snapshot(&client(), expiring_id).await.0 == StatusCode::NOT_FOUND {
                break;
            }
            yield_now().await;
        }
    })
    .await
    .expect("short-lived endpoint-bearing Snapshot expires");
    second_receiver
        .wait_for_no_notification_matching("snapshotId", expiring_id, Duration::from_millis(500))
        .await;
    first_receiver.shutdown().await;
    second_receiver.shutdown().await;
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_header_validates_uri_and_echoes_exact_value_on_all_response_classes() {
    let _serialized_environment = clean_database().await;
    let id = "urn:ngsi-ld:Snapshot:header-matrix";
    pretty_assertions::assert_eq!(
        create_snapshot_raw(&client(), &json!({"id": id, "type": "Snapshot", "snapshotQueries": []}))
            .await
            .0,
        StatusCode::CREATED
    );
    wait_for_snapshot_terminal(&client(), id, Duration::from_secs(5)).await;
    let success = client()
        .get(broker_url("/entities?type=https%3A%2F%2Fexample.org%2FAbsent"))
        .header("NGSILD-Snapshot", id)
        .send()
        .await
        .expect("scoped success");
    pretty_assertions::assert_eq!(success.status(), StatusCode::OK);
    pretty_assertions::assert_eq!(success.headers()["NGSILD-Snapshot"], id);
    let no_content = client()
        .delete(broker_url("/entities/urn:ngsi-ld:Entity:absent"))
        .header("NGSILD-Snapshot", id)
        .send()
        .await
        .expect("scoped delete error");
    pretty_assertions::assert_eq!(no_content.headers()["NGSILD-Snapshot"], id);
    // HTTP/1 recipients remove field-value OWS before application header
    // decoding (RFC 7230 section 3.2.4), so exercise significant whitespace
    // inside the URI here. The typed-header unit test separately rejects raw
    // leading/trailing whitespace when decoding is invoked before transport
    // normalisation.
    for malformed in ["", "not a URI", "urn:ngsi-ld:Snapshot:header matrix"] {
        pretty_assertions::assert_eq!(
            client()
                .get(broker_url("/entities?type=Entity"))
                .header("NGSILD-Snapshot", malformed)
                .send()
                .await
                .expect("malformed Snapshot header")
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    pretty_assertions::assert_eq!(
        client()
            .get(broker_url("/entities?type=Entity"))
            .header("NGSILD-Snapshot", "urn:ngsi-ld:Snapshot:unknown")
            .send()
            .await
            .expect("unknown Snapshot header")
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_protocol_distinguishes_invalid_json_from_bad_snapshot() {
    let _serialized_environment = clean_database().await;
    let invalid_json = client()
        .post(broker_url("/snapshots"))
        .header("Content-Type", "application/json")
        .body("{")
        .send()
        .await
        .expect("invalid Snapshot JSON");
    pretty_assertions::assert_eq!(invalid_json.status(), StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(
        invalid_json.json::<Value>().await.expect("invalid request problem")["type"],
        "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest"
    );
    let bad_model = client()
        .post(broker_url("/snapshots"))
        .header("Content-Type", "application/json")
        .json(&json!({"type": "Entity", "snapshotQueries": []}))
        .send()
        .await
        .expect("bad Snapshot model");
    pretty_assertions::assert_eq!(bad_model.status(), StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(
        bad_model.json::<Value>().await.expect("bad model problem")["type"],
        "https://uri.etsi.org/ngsi-ld/errors/BadRequestData"
    );
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_protocol_enforces_content_preconditions() {
    let _serialized_environment = clean_database().await;
    let missing_length = client().post(broker_url("/snapshots")).send().await.expect("missing length request");
    pretty_assertions::assert_eq!(missing_length.status(), StatusCode::LENGTH_REQUIRED);
    pretty_assertions::assert_eq!(missing_length.bytes().await.expect("empty 411 body").len(), 0);
    let unsupported = client()
        .post(broker_url("/snapshots"))
        .header("Content-Type", "text/plain")
        .body("payload")
        .send()
        .await
        .expect("unsupported media request");
    pretty_assertions::assert_eq!(unsupported.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    pretty_assertions::assert_eq!(unsupported.bytes().await.expect("empty 415 body").len(), 0);
    let zero = client()
        .post(broker_url("/snapshots"))
        .header("Content-Length", "0")
        .send()
        .await
        .expect("zero body request");
    pretty_assertions::assert_eq!(zero.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_protocol_resolves_context_by_media_type() {
    let _serialized_environment = clean_database().await;
    let core = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";
    let link = format!("<{core}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"");
    let json_body = json!({"id": "urn:ngsi-ld:Snapshot:json-link", "type": "Snapshot", "snapshotQueries": []});
    pretty_assertions::assert_eq!(
        client()
            .post(broker_url("/snapshots"))
            .header("Content-Type", "application/json")
            .header("Link", &link)
            .json(&json_body)
            .send()
            .await
            .expect("JSON+Link create")
            .status(),
        StatusCode::CREATED
    );
    let ld_body = json!({"id": "urn:ngsi-ld:Snapshot:jsonld-inline", "type": "Snapshot", "snapshotQueries": [], "@context": core});
    pretty_assertions::assert_eq!(
        client()
            .post(broker_url("/snapshots"))
            .header("Content-Type", "application/ld+json")
            .json(&ld_body)
            .send()
            .await
            .expect("JSON-LD create")
            .status(),
        StatusCode::CREATED
    );
    let json_inline = client()
        .post(broker_url("/snapshots"))
        .header("Content-Type", "application/json")
        .json(&ld_body)
        .send()
        .await
        .expect("illegal JSON inline context");
    pretty_assertions::assert_eq!(json_inline.status(), StatusCode::BAD_REQUEST);
    let ld_missing = client()
        .post(broker_url("/snapshots"))
        .header("Content-Type", "application/ld+json")
        .json(&json_body)
        .send()
        .await
        .expect("missing JSON-LD context");
    pretty_assertions::assert_eq!(ld_missing.status(), StatusCode::BAD_REQUEST);
    let ld_mixed = client()
        .post(broker_url("/snapshots"))
        .header("Content-Type", "application/ld+json")
        .header("Link", &link)
        .json(&ld_body)
        .send()
        .await
        .expect("mixed JSON-LD context");
    pretty_assertions::assert_eq!(ld_mixed.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
#[serial(snapshot)]
async fn snapshot_protocol_negotiates_response_context_and_errors() {
    let _serialized_environment = clean_database().await;
    let id = "urn:ngsi-ld:Snapshot:negotiation-matrix";
    pretty_assertions::assert_eq!(
        create_snapshot_raw(&client(), &json!({"id": id, "type": "Snapshot", "snapshotQueries": []}))
            .await
            .0,
        StatusCode::CREATED
    );
    for (accept, content_type, inline_context) in [
        ("application/json", "application/json", false),
        ("application/ld+json", "application/ld+json", true),
    ] {
        let response = client()
            .get(broker_url(&format!("/snapshots/{id}")))
            .header("Accept", accept)
            .send()
            .await
            .expect("negotiated Snapshot GET");
        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers()["Content-Type"].to_str().expect("content type").starts_with(content_type));
        let has_context = response.json::<Value>().await.expect("Snapshot representation").get("@context").is_some();
        pretty_assertions::assert_eq!(has_context, inline_context);
    }
    pretty_assertions::assert_eq!(
        client()
            .get(broker_url(&format!("/snapshots/{id}")))
            .header("Accept", "text/plain")
            .send()
            .await
            .expect("unacceptable Snapshot GET")
            .status(),
        StatusCode::NOT_ACCEPTABLE
    );
    let error = client()
        .get(broker_url("/snapshots/urn:ngsi-ld:Snapshot:absent"))
        .header("Accept", "application/ld+json")
        .send()
        .await
        .expect("Snapshot error representation");
    pretty_assertions::assert_eq!(error.status(), StatusCode::NOT_FOUND);
    assert!(
        error.headers()["Content-Type"]
            .to_str()
            .expect("error content type")
            .starts_with("application/json")
    );
}
