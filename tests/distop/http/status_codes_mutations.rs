//! Response-code matrix for entity retrieve and delete (clause 6.3.16, 6.4.3.2,
//! 6.4.3.3): the invalid-id `400`, unknown `404`, single-source `501`, local
//! `204`, and forwarded partial-failure `207` outcomes.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::vehicle,
    http::forwarding::{context_link, forwarded_entity_path, provision_source, retrieval_source},
    oracle::assert_status,
    topology::{self, BrokerRole},
};
use reqwest::Method;

/// An invalid entity id on retrieve is rejected with `400` (clause 6.4.3.2).
/// Confirms REQ-HTTP-68.
#[tokio::test]
async fn dist_http_retrieve_entity_invalid_id_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .get_entity("not a uri", "", "application/ld+json", Some(context_link()))
        .await
        .expect("invalid retrieve");
    assert_status(&response, 400);
    topology.finish().await.expect("topology cleanup");
}

/// An unknown entity on retrieve returns `404` (clause 6.4.3.2). Confirms REQ-HTTP-68.
#[tokio::test]
async fn dist_http_retrieve_entity_unknown_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:retrieve-unknown");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .get_entity(&entity_id, "", "application/ld+json", Some(context_link()))
        .await
        .expect("unknown retrieve");
    assert_status(&response, 404);
    topology.finish().await.expect("topology cleanup");
}

/// A retrieve resolving to a single source that cannot serve the representation
/// maps to `501` (clause 6.3.16). Confirms REQ-HTTP-68.
#[tokio::test]
async fn dist_http_retrieve_entity_source_unsupported_format_returns_501() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:retrieve-501");
    let reg_id = topology.id("registration:retrieve-501");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, "exclusive"))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub(Method::GET, &forwarded_entity_path(&entity_id), 501).await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .get_entity(&entity_id, "", "application/ld+json", Some(context_link()))
        .await
        .expect("forwarded retrieve");
    assert_status(&response, 501);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A local delete returns `204` (clause 6.4.3.3). Confirms REQ-HTTP-69.
#[tokio::test]
async fn dist_http_delete_entity_local_returns_204() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:delete-204");
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
        .delete_entity(&entity_id)
        .await
        .expect("local delete");
    assert_status(&response, 204);
    topology.finish().await.expect("topology cleanup");
}

/// A delete where a forwarded endpoint fails is a partial failure, mapped to
/// `207` (clause 6.3.16). Confirms REQ-HTTP-69.
#[tokio::test]
async fn dist_http_delete_entity_partial_failure_returns_207_batch_result() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:delete-207");
    let reg_id = topology.id("registration:delete-207");
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
            .create_registration(&provision_source(&reg_id, &mock.url(""), &entity_id, "inclusive", &["deleteEntity"]))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub(Method::DELETE, &forwarded_entity_path(&entity_id), 500).await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .delete_entity(&entity_id)
        .await
        .expect("forwarded delete");
    assert_status(&response, 207);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An invalid entity id on delete is rejected with `400` (clause 6.4.3.3).
/// Confirms REQ-HTTP-69.
#[tokio::test]
async fn dist_http_delete_entity_invalid_id_returns_400() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .delete_entity("not a uri")
        .await
        .expect("invalid delete");
    assert_status(&response, 400);
    topology.finish().await.expect("topology cleanup");
}

/// An unknown entity on delete returns `404` (clause 6.4.3.3). Confirms REQ-HTTP-69.
#[tokio::test]
async fn dist_http_delete_entity_unknown_returns_404() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:delete-unknown");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .delete_entity(&entity_id)
        .await
        .expect("unknown delete");
    assert_status(&response, 404);
    topology.finish().await.expect("topology cleanup");
}
