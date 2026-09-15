//! Conformance-mode gate-leak coverage (extension-strategy.md 6.4).
//!
//! A broker started with `ANDROMEDA__EXTENSIONS__CONFORMANCE_MODE=enabled` must
//! serve no extension even when the individual feature flags are also on. A
//! single broker cannot hold both states at once, so these tests run against a
//! *second* broker whose URL is passed as `ANDROMEDA_TEST_CONFORMANCE_BROKER_URL`
//! (started with conformance mode and the flags on, to prove the override
//! wins). They skip themselves when that variable is absent, so the suite runs
//! without it.

use andromeda_test_client::{broker_endpoint::BrokerEndpoint, header::jsonld_link_header, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use reqwest::{Method, StatusCode};
use std::env;

/// The conformance-mode broker client, or `None` when no second broker is
/// configured for the run.
fn conformance_client() -> Option<TestClient> {
    let url = env::var("ANDROMEDA_TEST_CONFORMANCE_BROKER_URL").ok()?;
    let endpoint = BrokerEndpoint::parse(&url).expect("conformance broker URL is valid");
    Some(TestClient::new(endpoint, None))
}

/// Conformance mode leaves `DELETE /temporal/entities` unrouted: the method is
/// refused exactly as a broker that never implemented the extension refuses it.
#[tokio::test]
async fn conformance_mode_does_not_route_the_temporal_delete_by_query() {
    let Some(client) = conformance_client() else {
        eprintln!("skipping: ANDROMEDA_TEST_CONFORMANCE_BROKER_URL is not set");
        return;
    };
    let response = client
        .raw(Method::DELETE, "/ngsi-ld/v1/temporal/entities?type=Vehicle&drop=speed")
        .header("Link", jsonld_link_header(CORE_CONTEXT).expect("core context Link"))
        .send()
        .await
        .expect("temporal delete request");
    assert_eq!(
        response.status,
        StatusCode::METHOD_NOT_ALLOWED,
        "conformance mode must not route the temporal delete-by-query"
    );
}

/// Conformance mode refuses a `datasetId` on Purge Entities as an invalid
/// request, because the dataset-scope extension is off.
#[tokio::test]
async fn conformance_mode_rejects_the_purge_dataset_scope() {
    let Some(client) = conformance_client() else {
        eprintln!("skipping: ANDROMEDA_TEST_CONFORMANCE_BROKER_URL is not set");
        return;
    };
    let response = client
        .delete_entities_by_query("type=Vehicle&drop=speed&datasetId=urn:ngsi-ld:Dataset:one", Some(CORE_CONTEXT))
        .await
        .expect("purge request");
    assert_eq!(
        response.status,
        StatusCode::BAD_REQUEST,
        "conformance mode must reject datasetId on Purge with InvalidRequest"
    );
}

/// Conformance mode advertises no extension in the Context Source Identity.
#[tokio::test]
async fn conformance_mode_advertises_no_extension() {
    let Some(client) = conformance_client() else {
        eprintln!("skipping: ANDROMEDA_TEST_CONFORMANCE_BROKER_URL is not set");
        return;
    };
    let response = client.source_identity(None).await.expect("source identity");
    assert_eq!(response.status, StatusCode::OK);
    let advertised = response.json_value().expect("identity body is JSON").to_string();
    assert!(
        !advertised.contains("temporal-delete-by-query"),
        "no extension token is advertised: {advertised}"
    );
    assert!(!advertised.contains("purge-dataset-scope"), "no extension token is advertised: {advertised}");
}
