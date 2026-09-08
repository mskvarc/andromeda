//! Response-code matrix for attribute delete, attribute replace and batch merge
//! (clause 6.3.16, 6.7, 6.20): local `204`, forwarded partial-failure `207`,
//! invalid `400`, and absent `404`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::vehicle,
    http::forwarding::{FORWARDED_BATCH_MERGE_PATH, context_link, forwarded_attr_path, provision_source},
    oracle::assert_status,
    topology::{self, BrokerRole},
};
use andromeda_test_client::entity_attribute::DatasetSelection;
use reqwest::Method;
use serde_json::json;

/// A local attribute delete returns `204` (clause 6.7.3.2). Confirms REQ-HTTP-77.
#[tokio::test]
async fn dist_http_delete_attr_local_returns_204() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:delattr-204");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .delete_entity_attr(&entity_id, "speed", None, DatasetSelection::Default, Some(context_link()))
        .await
        .expect("local attr delete");
    assert_status(&response, 204);
    topology.finish().await.expect("topology cleanup");
}

/// An attribute delete where a forwarded endpoint fails is a partial failure,
/// mapped to `207` (clause 6.3.16). Confirms REQ-HTTP-77.
#[tokio::test]
async fn dist_http_delete_attr_partial_failure_returns_207_update_result() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:delattr-207");
    let reg_id = topology.id("registration:delattr-207");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&provision_source(&reg_id, &mock.url(""), &entity_id, "inclusive", &["deleteAttrs"]))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub(Method::DELETE, &forwarded_attr_path(&entity_id, "speed"), 500).await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .delete_entity_attr(&entity_id, "speed", None, DatasetSelection::Default, Some(context_link()))
        .await
        .expect("forwarded attr delete");
    assert_status(&response, 207);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An invalid entity id on attribute delete is rejected with `400`
/// (clause 6.7.3.2). Confirms REQ-HTTP-77.
#[tokio::test]
async fn dist_http_delete_attr_invalid_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .delete_entity_attr("not a uri", "speed", None, DatasetSelection::Default, Some(context_link()))
        .await
        .expect("invalid attr delete");
    assert_status(&response, 400);
    topology.finish().await.expect("topology cleanup");
}

/// Deleting an absent attribute returns `404` (clause 6.7.3.2). Confirms REQ-HTTP-77.
#[tokio::test]
async fn dist_http_delete_attr_absent_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:delattr-404");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .delete_entity_attr(&entity_id, "color", None, DatasetSelection::Default, Some(context_link()))
        .await
        .expect("absent attr delete");
    assert_status(&response, 404);
    topology.finish().await.expect("topology cleanup");
}

/// A local attribute replace returns `204` (clause 6.7.3.3). Confirms REQ-HTTP-78.
#[tokio::test]
async fn dist_http_replace_attr_local_returns_204() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:repattr-204");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::PUT, &format!("/ngsi-ld/v1/entities/{}/attrs/speed", urlencoding::encode(&entity_id)))
        .header("content-type", "application/ld+json")
        .json(&json!({ "type": "Property", "value": 77, "@context": context_link() }))
        .send()
        .await
        .expect("local attr replace");
    assert_status(&response, 204);
    topology.finish().await.expect("topology cleanup");
}

/// An attribute replace where a forwarded endpoint fails is a partial failure,
/// mapped to `207` (clause 6.3.16). The registration advertises `replaceAttrs`,
/// the Table 4.20-1 name of 5.6.19 Replace Attribute. Confirms REQ-HTTP-78.
#[tokio::test]
async fn dist_http_replace_attr_partial_failure_returns_207_update_result() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:repattr-207");
    let reg_id = topology.id("registration:repattr-207");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&provision_source(&reg_id, &mock.url(""), &entity_id, "inclusive", &["replaceAttrs"]))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub(Method::PUT, &forwarded_attr_path(&entity_id, "speed"), 500).await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::PUT, &format!("/ngsi-ld/v1/entities/{}/attrs/speed", urlencoding::encode(&entity_id)))
        .header("content-type", "application/ld+json")
        .json(&json!({ "type": "Property", "value": 77, "@context": context_link() }))
        .send()
        .await
        .expect("forwarded attr replace");
    assert_status(&response, 207);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An attribute replace with an invalid body is rejected with `400`
/// (clause 6.7.3.3). Confirms REQ-HTTP-78.
#[tokio::test]
async fn dist_http_replace_attr_invalid_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:repattr-400");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::PUT, &format!("/ngsi-ld/v1/entities/{}/attrs/speed", urlencoding::encode(&entity_id)))
        .header("content-type", "application/ld+json")
        .json(&json!({ "type": "NotAnAttributeType", "value": 1, "@context": context_link() }))
        .send()
        .await
        .expect("invalid attr replace");
    assert_status(&response, 400);
    topology.finish().await.expect("topology cleanup");
}

/// Replacing an absent attribute returns `404` (clause 6.7.3.3). Confirms REQ-HTTP-78.
#[tokio::test]
async fn dist_http_replace_attr_absent_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:repattr-404");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::PUT, &format!("/ngsi-ld/v1/entities/{}/attrs/color", urlencoding::encode(&entity_id)))
        .header("content-type", "application/ld+json")
        .json(&json!({ "type": "Property", "value": "red", "@context": context_link() }))
        .send()
        .await
        .expect("absent attr replace");
    assert_status(&response, 404);
    topology.finish().await.expect("topology cleanup");
}

/// A local batch merge returns `204` (clause 6.20.3.1). Confirms REQ-HTTP-92.
#[tokio::test]
async fn dist_http_batch_merge_local_returns_204() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:batch-merge-204");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let body = json!([ { "id": entity_id, "type": "Vehicle", "speed": { "type": "Property", "value": 99 }, "@context": context_link() } ]);
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .batch_merge(&body)
        .await
        .expect("local batch merge");
    assert_status(&response, 204);
    topology.finish().await.expect("topology cleanup");
}

/// A batch merge where a forwarded endpoint fails is a partial failure, mapped to
/// `207` with a `BatchOperationResult` (clause 6.3.16). The registration
/// advertises `mergeBatch` (Table 4.20-1, 5.6.20), so the batch itself is
/// forwarded to `POST /entityOperations/merge` (clause 5.6.20.4). Confirms
/// REQ-HTTP-92.
#[tokio::test]
async fn dist_http_batch_merge_partial_failure_returns_207_batch_result() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:batch-merge-207");
    let reg_id = topology.id("registration:batch-merge-207");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&provision_source(&reg_id, &mock.url(""), &entity_id, "inclusive", &["mergeBatch"]))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub(Method::POST, FORWARDED_BATCH_MERGE_PATH, 500).await;

    let body = json!([ { "id": entity_id, "type": "Vehicle", "speed": { "type": "Property", "value": 99 }, "@context": context_link() } ]);
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .batch_merge(&body)
        .await
        .expect("forwarded batch merge");
    assert_status(&response, 207);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A batch merge with an invalid body is rejected with `400` (clause 6.20.3.1).
/// Confirms REQ-HTTP-92.
#[tokio::test]
async fn dist_http_batch_merge_invalid_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let body = json!([ { "speed": { "type": "Property", "value": 1 }, "@context": context_link() } ]);
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .batch_merge(&body)
        .await
        .expect("invalid batch merge");
    assert_status(&response, 400);
    topology.finish().await.expect("topology cleanup");
}
