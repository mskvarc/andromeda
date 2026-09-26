#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! Distributed entity retrieve tests.
//!
//! Tests retrieving single entities across context sources with different registration modes.

use crate::{
    context_source_adapter::ContextSourceAdapter as MockServer,
    helpers::{
        client,
        create_entity_local,
        create_registration,
        delete_entity,
        delete_registration,
        get_entity,
        jsonld_link_value,
        vehicle_entity,
        vehicle_entity_second,
        vehicle_entity_speed_only,
        vehicle_registration,
        vehicle_registration_speed_property,
        vehicle_registration_with_ops,
    },
};
use andromeda_test_harness::environment::TestEnvironment;
use reqwest::{Method, StatusCode};
use serde_json::Value;
use urlencoding::encode;

// ---------------------------------------------------------------------------
// Auxiliary mode tests
// ---------------------------------------------------------------------------

/// Retrieve entity merges local and remote attributes in auxiliary mode.
#[tokio::test]
async fn distop_entity_retrieve_auxiliary_merges_local_and_remote_attributes() {
    let scope = TestEnvironment::from_suite("distop_entity").await.expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:retrieve-aux");
    let reg_id = scope.id("Registration:retrieve-aux");

    // Create local entity (brandName=Mercedes, isParked)
    let local_entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &local_entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create auxiliary registration
    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), Some("auxiliary"));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Mock returns entity with brandName=BMW and isParked2
    let mock_entity = vehicle_entity_second(&entity_id);
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
        .await;

    // Retrieve entity
    let (status, _, body) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let obj = body.as_object().unwrap();
    let keys: Vec<_> = obj.keys().collect();
    assert!(obj.contains_key("brandName"), "expected brandName in {keys:?}");
    assert!(obj.contains_key("isParked"), "expected isParked in {keys:?}");
    assert!(obj.contains_key("isParked2"), "expected isParked2 in {keys:?}");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// Exclusive mode tests
// ---------------------------------------------------------------------------

/// Retrieve entity from exclusive registration returns remote attributes.
#[tokio::test]
async fn distop_entity_retrieve_exclusive_returns_remote_attributes() {
    let scope = TestEnvironment::from_suite("distop_entity").await.expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:retrieve-exc");
    let reg_id = scope.id("Registration:retrieve-exc");

    // Create local entity (no speed attribute)
    let local_entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &local_entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create exclusive registration with retrieveEntity operation
    let reg = vehicle_registration_speed_property(&reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"), Some(&["retrieveEntity"]));
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Mock returns entity with speed attribute
    let mock_entity = vehicle_entity_speed_only(&entity_id);
    mock.set_stub_with_body(Method::GET, &format!("/broker1/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
        .await;

    // Retrieve entity
    let (status, _, body) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let obj = body.as_object().unwrap();
    assert!(
        obj.contains_key("speed"),
        "expected speed from exclusive source, got keys: {:?}",
        obj.keys().collect::<Vec<_>>()
    );

    let count = mock.get_count(Method::GET, &format!("/broker1/ngsi-ld/v1/entities/{entity_id}"));
    assert!(count > 0, "mock should have been called");

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Retrieve entity with exclusive registration and local=true excludes remote attributes.
#[tokio::test]
async fn distop_entity_retrieve_exclusive_local_true_excludes_remote() {
    let scope = TestEnvironment::from_suite("distop_entity").await.expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:retrieve-exc-local");
    let reg_id = scope.id("Registration:retrieve-exc-local");

    // Create local entity
    let local_entity = vehicle_entity(&entity_id);
    let (status, _) = create_entity_local(&c, &local_entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Create exclusive registration
    let reg = vehicle_registration_speed_property(&reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Mock returns entity with speed
    let mock_entity = vehicle_entity_speed_only(&entity_id);
    mock.set_stub_with_body(Method::GET, &format!("/broker1/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
        .await;

    // Retrieve with local=true
    let (status, _, body) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    assert!(
        !body.as_object().unwrap().contains_key("speed"),
        "local=true should not include exclusive speed attribute"
    );

    delete_entity(&c, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// Inclusive mode tests
// ---------------------------------------------------------------------------

/// Retrieve entity from inclusive registration returns entity from remote.
#[tokio::test]
async fn distop_entity_retrieve_inclusive_returns_remote_entity() {
    let scope = TestEnvironment::from_suite("distop_entity").await.expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:retrieve-inc");
    let reg_id = scope.id("Registration:retrieve-inc");

    // Create inclusive registration (no mode = inclusive default)
    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Mock returns entity
    let mock_entity = vehicle_entity(&entity_id);
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
        .await;

    // Retrieve entity
    let (status, _, _) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Retrieve with local=true and inclusive registration returns 404 when entity is only remote.
#[tokio::test]
async fn distop_entity_retrieve_inclusive_local_true_returns_404() {
    let scope = TestEnvironment::from_suite("distop_entity").await.expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:retrieve-inc-local");
    let reg_id = scope.id("Registration:retrieve-inc-local");

    // Create inclusive registration (entity only on remote)
    let reg = vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), None);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Mock stub (should not be called)
    let mock_entity = vehicle_entity(&entity_id);
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
        .await;

    // Retrieve with local=true returns 404
    let (status, _, _) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);

    // Verify no request was forwarded
    pretty_assertions::assert_eq!(mock.get_total_count(), 0);

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Inclusive registration with retrieveEntity operation forwards correctly.
#[tokio::test]
async fn distop_entity_retrieve_inclusive_with_retrieve_entity_op() {
    let scope = TestEnvironment::from_suite("distop_entity").await.expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:retrieve-inc-op1");
    let reg_id = scope.id("Registration:retrieve-inc-op1");

    // Create inclusive registration with retrieveEntity operation
    let reg = vehicle_registration_with_ops(&reg_id, Some(&entity_id), &mock.url(""), None, &["retrieveEntity"]);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Mock returns entity
    let mock_entity = vehicle_entity(&entity_id);
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
        .await;

    // Retrieve entity
    let (status, _, _) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.1.4: a retrieval is forwarded only to the registrations that "match
/// and support the retrieveEntity operation", and Table 4.20-1 names
/// `retrieveEntity`, `queryEntity` and `queryBatch` as separate operations. A
/// registration advertising only `queryEntity` or only `queryBatch` therefore
/// does not apply: its source is never contacted -- neither through a
/// retrieve-by-id nor through a query standing in for one -- and with nothing
/// held locally the retrieval is `ResourceNotFound`. (ETSI conformance
/// `D010_03_inc_02` and `D010_03_inc_03` expect the opposite and are corrected.)
async fn assert_retrieve_skips_registration_advertising_only(operation: &str, label: &str) {
    let scope = TestEnvironment::from_suite("distop_entity").await.expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id(&format!("Vehicle:{label}"));
    let reg_id = scope.id(&format!("Registration:{label}"));

    let reg = vehicle_registration_with_ops(&reg_id, Some(&entity_id), &mock.url(""), None, &[operation]);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Stubs a downgraded retrieval would hit, so a forward would answer 200
    // rather than fail unnoticed.
    let mock_array = Value::Array(vec![vehicle_entity(&entity_id)]);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;
    mock.set_stub_with_body(Method::POST, "/ngsi-ld/v1/entityOperations/query", 200, &mock_array)
        .await;

    let (status, _, _) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    // The retrieval answered only after every forwarded request settled, so a
    // source that was contacted has already recorded it. Only requests naming
    // this entity count: the suite shares one tenant, and a concurrent test's
    // type-only query is rightly forwarded to this queryEntity source.
    pretty_assertions::assert_eq!(
        requests_naming(&mock, &entity_id).await,
        Vec::<String>::new(),
        "a {operation}-only source is never contacted for a retrieval"
    );

    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// The URIs of every request `mock` received that names `entity_id`, in its
/// path or its query string.
async fn requests_naming(mock: &MockServer, entity_id: &str) -> Vec<String> {
    let encoded = encode(entity_id).into_owned();
    mock.drain_requests()
        .await
        .into_iter()
        .map(|request| request.uri.to_string())
        .filter(|uri| uri.contains(entity_id) || uri.contains(&encoded))
        .collect()
}

/// Clause 5.7.1.4 forwards a retrieval to the registrations that "support the
/// retrieveEntity operation (see operations and operation groups in clause
/// 4.20)". Table 4.20-2 puts `retrieveEntity` in `retrieveOps`,
/// `redirectionOps`, `federationOps` and `associationOps`, and a registration
/// naming no `operations` advertises `federationOps`. Each such registration is
/// contacted with the retrieval itself, `GET /entities/{id}`.
async fn assert_retrieve_forwarded_to_registration_advertising(operations: Option<&[&str]>, label: &str) {
    let scope = TestEnvironment::from_suite("distop_entity").await.expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id(&format!("Vehicle:{label}"));
    let reg_id = scope.id(&format!("Registration:{label}"));
    let reg = match operations {
        Some(operations) => vehicle_registration_with_ops(&reg_id, Some(&entity_id), &mock.url(""), None, operations),
        None => vehicle_registration(&reg_id, Some(&entity_id), &mock.url(""), None),
    };
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let retrieve_path = format!("/ngsi-ld/v1/entities/{entity_id}");
    mock.set_stub_with_body(Method::GET, &retrieve_path, 200, &vehicle_entity(&entity_id)).await;

    let (status, _, _) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    let forwarded = requests_naming(&mock, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    pretty_assertions::assert_eq!(status, StatusCode::OK, "{operations:?}: the source answers the retrieval");
    pretty_assertions::assert_eq!(forwarded.len(), 1, "{operations:?}: exactly one forwarded request: {forwarded:?}");
    assert!(
        forwarded.iter().all(|uri| !uri.contains("entityOperations") && !uri.contains("?id=")),
        "{operations:?}: the retrieval is forwarded as itself, never as a query: {forwarded:?}"
    );
}

#[tokio::test]
async fn distop_entity_retrieve_with_retrieve_ops_group_is_forwarded() {
    assert_retrieve_forwarded_to_registration_advertising(Some(&["retrieveOps"]), "retrieve-group-retrieve-ops").await;
}

#[tokio::test]
async fn distop_entity_retrieve_with_redirection_ops_group_is_forwarded() {
    assert_retrieve_forwarded_to_registration_advertising(Some(&["redirectionOps"]), "retrieve-group-redirection-ops").await;
}

#[tokio::test]
async fn distop_entity_retrieve_with_federation_ops_group_is_forwarded() {
    assert_retrieve_forwarded_to_registration_advertising(Some(&["federationOps"]), "retrieve-group-federation-ops").await;
}

#[tokio::test]
async fn distop_entity_retrieve_with_association_ops_group_is_forwarded() {
    assert_retrieve_forwarded_to_registration_advertising(Some(&["associationOps"]), "retrieve-group-association-ops").await;
}

#[tokio::test]
async fn distop_entity_retrieve_with_no_operations_member_is_forwarded() {
    assert_retrieve_forwarded_to_registration_advertising(None, "retrieve-group-default").await;
}

#[tokio::test]
async fn distop_entity_retrieve_with_only_query_entity_is_not_forwarded() {
    assert_retrieve_skips_registration_advertising_only("queryEntity", "retrieve-inc-op2").await;
}

#[tokio::test]
async fn distop_entity_retrieve_with_only_query_batch_is_not_forwarded() {
    assert_retrieve_skips_registration_advertising_only("queryBatch", "retrieve-inc-op3").await;
}

// ---------------------------------------------------------------------------
// Redirect mode tests
// ---------------------------------------------------------------------------

/// Retrieve entity with two redirect registrations merges attributes from both.
#[tokio::test]
async fn distop_entity_retrieve_redirect_merges_two_brokers() {
    let scope = TestEnvironment::from_suite("distop_entity").await.expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:retrieve-red");
    let reg_id1 = scope.id("Registration:retrieve-red-1");
    let reg_id2 = scope.id("Registration:retrieve-red-2");

    // Create two redirect registrations
    let reg1 = vehicle_registration_with_ops(&reg_id1, Some(&entity_id), &mock.url("/broker1"), Some("redirect"), &["redirectionOps"]);
    let (status, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg2 = vehicle_registration_with_ops(&reg_id2, Some(&entity_id), &mock.url("/broker2"), Some("redirect"), &["redirectionOps"]);
    let (status, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // broker1 returns entity with isParked
    let mock_entity1 = vehicle_entity(&entity_id);
    mock.set_stub_with_body(Method::GET, &format!("/broker1/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity1)
        .await;

    // broker2 returns entity with isParked2
    let mock_entity2 = vehicle_entity_second(&entity_id);
    mock.set_stub_with_body(Method::GET, &format!("/broker2/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity2)
        .await;

    // Retrieve entity
    let (status, _, body) = get_entity(&c, &entity_id, "", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let obj = body.as_object().unwrap();
    let keys: Vec<_> = obj.keys().collect();
    assert!(obj.contains_key("isParked"), "expected isParked in {keys:?}");
    assert!(obj.contains_key("isParked2"), "expected isParked2 in {keys:?}");

    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Retrieve with redirect registration and local=true returns 404.
#[tokio::test]
async fn distop_entity_retrieve_redirect_local_true_returns_404() {
    let scope = TestEnvironment::from_suite("distop_entity").await.expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;

    let entity_id = scope.id("Vehicle:retrieve-red-local");
    let reg_id1 = scope.id("Registration:retrieve-red-local-1");
    let reg_id2 = scope.id("Registration:retrieve-red-local-2");

    // Create two redirect registrations
    let reg1 = vehicle_registration_with_ops(&reg_id1, Some(&entity_id), &mock.url("/broker1"), Some("redirect"), &["redirectionOps"]);
    let (status, _) = create_registration(&c, &reg1).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let reg2 = vehicle_registration_with_ops(&reg_id2, Some(&entity_id), &mock.url("/broker2"), Some("redirect"), &["redirectionOps"]);
    let (status, _) = create_registration(&c, &reg2).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    // Set mock stubs
    mock.set_stub(Method::GET, &format!("/broker1/ngsi-ld/v1/entities/{entity_id}"), 200).await;
    mock.set_stub(Method::GET, &format!("/broker2/ngsi-ld/v1/entities/{entity_id}"), 200).await;

    // Retrieve with local=true returns 404
    let (status, _, _) = get_entity(&c, &entity_id, "local=true", &jsonld_link_value()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);

    // Verify no requests forwarded
    pretty_assertions::assert_eq!(mock.get_count(Method::GET, &format!("/broker1/ngsi-ld/v1/entities/{entity_id}")), 0);
    pretty_assertions::assert_eq!(mock.get_count(Method::GET, &format!("/broker2/ngsi-ld/v1/entities/{entity_id}")), 0);

    delete_registration(&c, &reg_id1).await;
    delete_registration(&c, &reg_id2).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// Retrieve Entity `type` parameter (clauses 5.7.1.3, 5.7.1.4)
// ---------------------------------------------------------------------------

/// Clause 5.7.1.4 retrieves the Entity "whose id (URI), and where specified
/// type, is equivalent": a local Entity of another type is not found, one of
/// the requested type is, and clause 6.3.20 refuses a selector the clause 4.17
/// grammar does not produce.
#[tokio::test]
async fn distop_entity_retrieve_type_selects_the_local_entity_only_when_its_type_matches() {
    let scope = TestEnvironment::from_suite("distop_entity").await.expect("isolated test environment");
    let c = client();
    let entity_id = scope.id("Vehicle:retrieve-type-local");

    let (status, _) = create_entity_local(&c, &vehicle_entity(&entity_id)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (matching, _, body) = get_entity(&c, &entity_id, "type=Vehicle", &jsonld_link_value()).await;
    let (other, _, _) = get_entity(&c, &entity_id, "type=Building", &jsonld_link_value()).await;
    let (invalid, _, _) = get_entity(&c, &entity_id, "type=Vehicle%7C", &jsonld_link_value()).await;
    delete_entity(&c, &entity_id).await;
    scope.finish().await.expect("cleanup failed");

    pretty_assertions::assert_eq!(matching, StatusCode::OK, "the Entity has the requested type");
    pretty_assertions::assert_eq!(body["id"], entity_id.as_str());
    pretty_assertions::assert_eq!(other, StatusCode::NOT_FOUND, "no Entity with this id has the requested type");
    pretty_assertions::assert_eq!(invalid, StatusCode::BAD_REQUEST, "a dangling type-selector operator is refused");
}

/// Clause 5.7.1.4 bullet on registrations: only registrations that match the
/// request -- including its type (clause 5.12) -- are contacted, and the type is
/// passed on so the source applies the same restriction (clause 4.3.6.1: "the
/// Entity type selector ... can be added as a filter on forwarded requests").
#[tokio::test]
async fn distop_entity_retrieve_type_is_forwarded_only_to_registrations_covering_it() {
    let scope = TestEnvironment::from_suite("distop_entity").await.expect("isolated test environment");
    let c = client();
    let mock = MockServer::start().await;
    let entity_id = scope.id("Vehicle:retrieve-type-forward");
    let reg_id = scope.id("Registration:retrieve-type-forward");

    let reg = vehicle_registration_with_ops(&reg_id, Some(&entity_id), &mock.url(""), None, &["retrieveEntity"]);
    let (status, _) = create_registration(&c, &reg).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &vehicle_entity(&entity_id))
        .await;

    let (uncovered, _, _) = get_entity(&c, &entity_id, "type=Building", &jsonld_link_value()).await;
    let forwarded_uncovered = requests_naming(&mock, &entity_id).await;
    let (covered, _, _) = get_entity(&c, &entity_id, "type=Vehicle", &jsonld_link_value()).await;
    let forwarded_covered = requests_naming(&mock, &entity_id).await;
    delete_registration(&c, &reg_id).await;
    mock.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    pretty_assertions::assert_eq!(uncovered, StatusCode::NOT_FOUND, "a Vehicle-only registration does not cover Building");
    pretty_assertions::assert_eq!(forwarded_uncovered, Vec::<String>::new(), "the uncovering registration is never contacted");
    pretty_assertions::assert_eq!(covered, StatusCode::OK);
    pretty_assertions::assert_eq!(forwarded_covered.len(), 1, "{forwarded_covered:?}");
    assert!(
        forwarded_covered.iter().all(|uri| uri.contains("type=") && uri.contains("Vehicle")),
        "the requested type travels with the forwarded retrieval: {forwarded_covered:?}"
    );
}
