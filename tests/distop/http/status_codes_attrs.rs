//! Response-code matrix for attribute append, update and partial-update (clause
//! 6.3.16, 6.6, 6.7): local `204`, forwarded partial-failure `207` with an
//! `UpdateResult`, invalid `400`, and absent-entity/attribute `404`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{attribute_fragment, property, vehicle},
    http::forwarding::{context_link, forwarded_attr_path, forwarded_attrs_path, provision_source},
    oracle::assert_status,
    topology::{self, BrokerRole},
};
use reqwest::Method;
use serde_json::json;

/// A local attribute append returns `204` (clause 6.6.3.1). Confirms REQ-HTTP-74.
#[tokio::test]
async fn dist_http_append_attrs_local_returns_204() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:append-204");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let fragment = attribute_fragment(&[("fuel", property(json!("diesel")))]);
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .append_entity_attrs(&entity_id, &fragment)
        .await
        .expect("local append");
    assert_status(&response, 204);
    topology.finish().await.expect("topology cleanup");
}

/// An append where a forwarded endpoint fails is a partial failure, mapped to
/// `207` with an `UpdateResult` (clause 6.3.16). The appended attribute is the
/// registered `speed`: only attributes the registration covers are forwarded
/// (clause 5.6.3.4), so appending an uncovered attribute would never reach the
/// failing endpoint. Confirms REQ-HTTP-74.
#[tokio::test]
async fn dist_http_append_attrs_partial_failure_returns_207_update_result() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:append-207");
    let reg_id = topology.id("registration:append-207");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
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
            .client()
            .create_registration(&provision_source(&reg_id, &mock.url(""), &entity_id, "inclusive", &["appendAttrs"]))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub(Method::POST, &forwarded_attrs_path(&entity_id), 500).await;

    let fragment = attribute_fragment(&[("speed", property(json!(50)))]);
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .append_entity_attrs(&entity_id, &fragment)
        .await
        .expect("forwarded append");
    assert_status(&response, 207);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An append with an invalid fragment is rejected with `400` (clause 6.6.3.1).
/// Confirms REQ-HTTP-74.
#[tokio::test]
async fn dist_http_append_attrs_invalid_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:append-400");
    assert_status(
        &topology
            .broker(BrokerRole::A)
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
        .client()
        .append_entity_attrs(
            &entity_id,
            &json!({ "fuel": { "type": "NotAnAttributeType", "value": 1 }, "@context": context_link() }),
        )
        .await
        .expect("invalid append");
    assert_status(&response, 400);
    topology.finish().await.expect("topology cleanup");
}

/// Appending to an absent entity returns `404` (clause 6.6.3.1). Confirms REQ-HTTP-74.
#[tokio::test]
async fn dist_http_append_attrs_entity_absent_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:append-404");
    let fragment = attribute_fragment(&[("fuel", property(json!("diesel")))]);
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .append_entity_attrs(&entity_id, &fragment)
        .await
        .expect("absent append");
    assert_status(&response, 404);
    topology.finish().await.expect("topology cleanup");
}

/// A local attribute update returns `204` (clause 6.6.3.2). Confirms REQ-HTTP-75.
#[tokio::test]
async fn dist_http_update_attrs_local_returns_204() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:update-204");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let fragment = attribute_fragment(&[("speed", property(json!(50)))]);
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .update_entity_attrs(&entity_id, &fragment)
        .await
        .expect("local update");
    assert_status(&response, 204);
    topology.finish().await.expect("topology cleanup");
}

/// An update where a forwarded endpoint fails is a partial failure, mapped to
/// `207` with an `UpdateResult` (clause 6.3.16). The registration advertises
/// `updateEntity`, the Table 4.20-1 name of 5.6.2 Update Attributes.
/// Confirms REQ-HTTP-75.
#[tokio::test]
async fn dist_http_update_attrs_partial_failure_returns_207_update_result() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:update-207");
    let reg_id = topology.id("registration:update-207");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
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
            .client()
            .create_registration(&provision_source(&reg_id, &mock.url(""), &entity_id, "inclusive", &["updateEntity"]))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub(Method::PATCH, &forwarded_attrs_path(&entity_id), 500).await;

    let fragment = attribute_fragment(&[("speed", property(json!(50)))]);
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .update_entity_attrs(&entity_id, &fragment)
        .await
        .expect("forwarded update");
    assert_status(&response, 207);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An update with an invalid fragment is rejected with `400` (clause 6.6.3.2).
/// Confirms REQ-HTTP-75.
#[tokio::test]
async fn dist_http_update_attrs_invalid_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:update-400");
    assert_status(
        &topology
            .broker(BrokerRole::A)
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
        .client()
        .update_entity_attrs(
            &entity_id,
            &json!({ "speed": { "type": "NotAnAttributeType", "value": 1 }, "@context": context_link() }),
        )
        .await
        .expect("invalid update");
    assert_status(&response, 400);
    topology.finish().await.expect("topology cleanup");
}

/// Updating an absent entity returns `404` (clause 6.6.3.2). Confirms REQ-HTTP-75.
#[tokio::test]
async fn dist_http_update_attrs_entity_absent_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:update-404");
    let fragment = attribute_fragment(&[("speed", property(json!(50)))]);
    let response = topology
        .broker(BrokerRole::A)
        .client()
        .update_entity_attrs(&entity_id, &fragment)
        .await
        .expect("absent update");
    assert_status(&response, 404);
    topology.finish().await.expect("topology cleanup");
}

/// A local partial attribute update returns `204` (clause 6.7.3.1).
/// Confirms REQ-HTTP-76.
#[tokio::test]
async fn dist_http_partial_update_attr_local_returns_204() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:partial-204");
    assert_status(
        &topology
            .broker(BrokerRole::A)
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
        .client()
        .partial_update_attr(&entity_id, "speed", &json!({ "value": 60, "@context": context_link() }))
        .await
        .expect("local partial update");
    assert_status(&response, 204);
    topology.finish().await.expect("topology cleanup");
}

/// A partial update where a forwarded endpoint fails is a partial failure, mapped
/// to `207` with an `UpdateResult` (clause 6.3.16). Confirms REQ-HTTP-76.
#[tokio::test]
async fn dist_http_partial_update_attr_partial_failure_returns_207_update_result() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:partial-207");
    let reg_id = topology.id("registration:partial-207");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
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
            .client()
            .create_registration(&provision_source(&reg_id, &mock.url(""), &entity_id, "inclusive", &["updateAttrs"]))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub(Method::PATCH, &forwarded_attr_path(&entity_id, "speed"), 500).await;

    let response = topology
        .broker(BrokerRole::A)
        .client()
        .partial_update_attr(&entity_id, "speed", &json!({ "value": 60, "@context": context_link() }))
        .await
        .expect("forwarded partial update");
    assert_status(&response, 207);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A partial update with an invalid fragment is rejected with `400`
/// (clause 6.7.3.1). Confirms REQ-HTTP-76.
#[tokio::test]
async fn dist_http_partial_update_attr_invalid_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:partial-400");
    assert_status(
        &topology
            .broker(BrokerRole::A)
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
        .client()
        .partial_update_attr(
            &entity_id,
            "speed",
            &json!({ "type": "NotAnAttributeType", "value": 1, "@context": context_link() }),
        )
        .await
        .expect("invalid partial update");
    assert_status(&response, 400);
    topology.finish().await.expect("topology cleanup");
}

/// A partial update of an absent attribute returns `404` (clause 6.7.3.1).
/// Confirms REQ-HTTP-76.
#[tokio::test]
async fn dist_http_partial_update_attr_absent_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:partial-404");
    assert_status(
        &topology
            .broker(BrokerRole::A)
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
        .client()
        .partial_update_attr(&entity_id, "color", &json!({ "value": "red", "@context": context_link() }))
        .await
        .expect("absent partial update");
    assert_status(&response, 404);
    topology.finish().await.expect("topology cleanup");
}
