//! Which named operation group advertises which distributed operation.
//!
//! ETSI GS CIM 009 v1.9.1 Table 4.20-2 fixes the membership of each named
//! operation group, and clause 5.9.2 makes a registration's `operations` member
//! the gate a distributed operation has to pass before it is forwarded. The
//! broker expanded `purgeEntity` to no group at all, so a Context Source that
//! advertised `redirectionOps` -- which the table lists `purgeEntity` under --
//! never received a forwarded purge.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::{context_source_adapter::ContextSourceAdapter, probe_entity_type::probe_entity_type};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use http::Method;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The path a forwarded purge lands on at a registered Context Source.
const FORWARDED_PURGE_PATH: &str = "/ngsi-ld/v1/entities";

/// A registration that advertises exactly one named operation group.
fn registration_advertising(id: &str, endpoint: &str, group: &str, entity_type: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "endpoint": endpoint,
        "mode": "redirect",
        "operations": [group],
        "information": [{"entities": [{"type": entity_type}]}],
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Issue a non-local purge over the entities collection for one entity type.
async fn purge_by_type(client: &TestClient, entity_type: &str) -> StatusCode {
    let path = format!("/ngsi-ld/v1/entities?type={entity_type}");
    let response = client
        .raw(Method::DELETE, &path)
        .header(
            "link",
            format!("<{TEST_SUITE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"").as_str(),
        )
        .send()
        .await
        .expect("purge request");
    response.status
}

/// Table 4.20-2 lists `purgeEntity` under `redirectionOps`, so a registration
/// advertising that group alone is one the purge has to be forwarded to. The
/// broker expanded `purgeEntity` to its own token only, so this source was
/// never consulted and the purge silently completed locally.
#[tokio::test]
async fn regression_operation_group_redirection_ops_advertises_purge_entity() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let source = ContextSourceAdapter::start().await;
    let entity_type = probe_entity_type(&scope, "PurgeGroupProbe");

    let registration = registration_advertising(
        &scope.id("registration:purge-redirection-ops"),
        &source.url("/"),
        "redirectionOps",
        &entity_type,
    );
    let created = client.create_registration(&registration).await.expect("create registration");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    source.set_stub(Method::DELETE, FORWARDED_PURGE_PATH, 204).await;
    let status = purge_by_type(client, &entity_type).await;
    assert!(
        matches!(status.as_u16(), 200 | 204 | 207),
        "a purge across a redirect registration completes: {status}"
    );

    assert_eq!(
        source.get_count(Method::DELETE, FORWARDED_PURGE_PATH),
        1,
        "Table 4.20-2 lists purgeEntity under redirectionOps, so the purge is forwarded"
    );
    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// The same table lists `purgeEntity` under no other group. `federationOps` and
/// `associationOps` therefore must not admit it -- widening the fix to every
/// group would leak provisioning operations to sources that only advertise
/// consumption ones.
#[tokio::test]
async fn regression_operation_group_no_other_group_advertises_purge_entity() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (group, probe_base) in [("federationOps", "PurgeFederationProbe"), ("associationOps", "PurgeAssociationProbe")] {
        let source = ContextSourceAdapter::start().await;
        let entity_type = probe_entity_type(&scope, probe_base);
        let registration = registration_advertising(&scope.id(&format!("registration:purge-{group}")), &source.url("/"), group, &entity_type);
        let created = client.create_registration(&registration).await.expect("create registration");
        assert_eq!(created.status, StatusCode::CREATED, "{group}: {:?}", created.body);

        source.set_stub(Method::DELETE, FORWARDED_PURGE_PATH, 204).await;
        purge_by_type(client, &entity_type).await;

        assert_eq!(
            source.get_total_count(),
            0,
            "{group} does not list purgeEntity in Table 4.20-2, so nothing is forwarded to it"
        );
        source.shutdown().await;
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.20: a registration naming `purgeEntity` itself has always been a
/// match, and must remain one -- the group expansion is additional to the
/// operation's own token, never a replacement for it.
#[tokio::test]
async fn regression_operation_group_purge_entity_named_directly_still_matches() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let source = ContextSourceAdapter::start().await;
    let entity_type = probe_entity_type(&scope, "PurgeNamedProbe");

    let registration = registration_advertising(&scope.id("registration:purge-named"), &source.url("/"), "purgeEntity", &entity_type);
    let created = client.create_registration(&registration).await.expect("create registration");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    source.set_stub(Method::DELETE, FORWARDED_PURGE_PATH, 204).await;
    purge_by_type(client, &entity_type).await;

    assert_eq!(
        source.get_count(Method::DELETE, FORWARDED_PURGE_PATH),
        1,
        "the operation's own Table 4.20-1 token still advertises it"
    );
    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}
