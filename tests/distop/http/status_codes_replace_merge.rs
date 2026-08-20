//! Response-code matrix for entity replace and merge (clause 6.3.16, 6.4.3.3):
//! local `204`, forwarded partial-failure `207`, invalid `400`, unknown `404`,
//! and the simplified-merge type-change `400`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{attribute_fragment, property, vehicle},
    http::forwarding::{context_link, forwarded_entity_path, provision_source},
    oracle::assert_status,
    topology::DistributedTopology,
};
use reqwest::Method;
use serde_json::json;

/// A local replace returns `204` (clause 6.4.3.3). Confirms REQ-HTTP-70.
#[tokio::test]
async fn dist_http_replace_entity_local_returns_204() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:replace-204");
    assert_status(
        &topology
            .front()
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let response = topology
        .front()
        .client()
        .replace_entity(&entity_id, &vehicle(&entity_id, 9), "application/ld+json", None)
        .await
        .expect("local replace");
    assert_status(&response, 204);
    topology.finish().await.expect("topology cleanup");
}

/// A replace where a forwarded endpoint fails is a partial failure, mapped to
/// `207` (clause 6.3.16). Confirms REQ-HTTP-70.
#[tokio::test]
async fn dist_http_replace_entity_partial_failure_returns_207_batch_result() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:replace-207");
    let reg_id = topology.id("registration:replace-207");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .front()
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
            .front()
            .client()
            .create_registration(&provision_source(&reg_id, &mock.url(""), &entity_id, "inclusive", &["replaceEntity"]))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub(Method::PUT, &forwarded_entity_path(&entity_id), 500).await;

    let response = topology
        .front()
        .client()
        .replace_entity(&entity_id, &vehicle(&entity_id, 9), "application/ld+json", None)
        .await
        .expect("forwarded replace");
    assert_status(&response, 207);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A replace with an invalid body is rejected with `400` (clause 6.4.3.3).
/// Confirms REQ-HTTP-70.
#[tokio::test]
async fn dist_http_replace_entity_invalid_returns_400() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:replace-400");
    assert_status(
        &topology
            .front()
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let response = topology
        .front()
        .client()
        .replace_entity(&entity_id, &json!({ "id": entity_id, "@context": context_link() }), "application/ld+json", None)
        .await
        .expect("invalid replace");
    assert_status(&response, 400);
    topology.finish().await.expect("topology cleanup");
}

/// Replacing an unknown entity returns `404` (clause 6.4.3.3). Confirms REQ-HTTP-70.
#[tokio::test]
async fn dist_http_replace_entity_unknown_returns_404() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:replace-unknown");
    let response = topology
        .front()
        .client()
        .replace_entity(&entity_id, &vehicle(&entity_id, 9), "application/ld+json", None)
        .await
        .expect("unknown replace");
    assert_status(&response, 404);
    topology.finish().await.expect("topology cleanup");
}

/// A simplified merge that would change the entity type is rejected with `400`
/// (clause 6.4.3.3). Confirms REQ-HTTP-72.
#[tokio::test]
async fn dist_http_merge_entity_simplified_type_change_returns_400() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:merge-type-change");
    assert_status(
        &topology
            .front()
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let response = topology
        .front()
        .client()
        .merge_entity(&entity_id, &json!({ "type": "Car", "@context": context_link() }), None)
        .await
        .expect("type-changing merge");
    assert_status(&response, 400);
    topology.finish().await.expect("topology cleanup");
}

/// A local merge returns `204` (clause 6.4.3.3). Confirms REQ-HTTP-73.
#[tokio::test]
async fn dist_http_merge_entity_local_returns_204() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:merge-204");
    assert_status(
        &topology
            .front()
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let fragment = attribute_fragment(&[("speed", property(json!(88)))]);
    let response = topology.front().client().merge_entity(&entity_id, &fragment, None).await.expect("local merge");
    assert_status(&response, 204);
    topology.finish().await.expect("topology cleanup");
}

/// A merge where a forwarded endpoint fails is a partial failure, mapped to `207`
/// (clause 6.3.16). Confirms REQ-HTTP-73.
#[tokio::test]
async fn dist_http_merge_entity_partial_failure_returns_207_batch_result() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:merge-207");
    let reg_id = topology.id("registration:merge-207");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .front()
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
            .front()
            .client()
            .create_registration(&provision_source(&reg_id, &mock.url(""), &entity_id, "inclusive", &["mergeEntity"]))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub(Method::PATCH, &forwarded_entity_path(&entity_id), 500).await;

    let fragment = attribute_fragment(&[("speed", property(json!(88)))]);
    let response = topology
        .front()
        .client()
        .merge_entity(&entity_id, &fragment, None)
        .await
        .expect("forwarded merge");
    assert_status(&response, 207);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A merge with an invalid fragment is rejected with `400` (clause 6.4.3.3).
/// Confirms REQ-HTTP-73.
#[tokio::test]
async fn dist_http_merge_entity_invalid_returns_400() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:merge-400");
    assert_status(
        &topology
            .front()
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let response = topology
        .front()
        .client()
        .merge_entity(
            &entity_id,
            &json!({ "speed": { "type": "NotAnAttributeType", "value": 1 }, "@context": context_link() }),
            None,
        )
        .await
        .expect("invalid merge");
    assert_status(&response, 400);
    topology.finish().await.expect("topology cleanup");
}

/// Merging an unknown entity returns `404` (clause 6.4.3.3). Confirms REQ-HTTP-73.
#[tokio::test]
async fn dist_http_merge_entity_unknown_returns_404() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:merge-unknown");
    let fragment = attribute_fragment(&[("speed", property(json!(88)))]);
    let response = topology
        .front()
        .client()
        .merge_entity(&entity_id, &fragment, None)
        .await
        .expect("unknown merge");
    assert_status(&response, 404);
    topology.finish().await.expect("topology cleanup");
}
