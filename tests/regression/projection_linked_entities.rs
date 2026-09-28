//! What the clause 4.21 projection reaches, and what it leaves behind.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.21 gives the Attribute Projection Language
//! one reach and two depths: it specifies "the Attributes to be retrieved
//! within an Entity (or associated Linked Entities when Linked Entity Retrieval
//! is used)". Clause 5.7.1.5 says the same of the retrieval's output -- "If a
//! restrictive list of Entity member names is present, the Entity is reduced
//! down to only contain the defined Entity members (applies to all Entities in
//! the payload in the case of Linked Entity retrieval)" -- and clause 5.7.2.5
//! of the query's, reducing "every Entity within the payload body". A Linked
//! Entity is therefore narrowed by exactly the rule that narrows a matching
//! one, `id` and `type` included, and the broker's earlier rule of preserving
//! identity on a Linked Entity has no clause behind it.
//!
//! Three further consequences follow from the same sentences.
//!
//! Clause 5.7.1.5 makes membership of the flattened array follow the payload:
//! "If any of the returned Attributes corresponds to an annotated Relationship,
//! unless a target URI has been previously encountered, the data corresponding
//! to each URI of the Relationship's target object URIs is appended to the
//! array." The Attributes that decide it are the *returned* ones, so an entity
//! reachable only through a Relationship the projection removed is no longer
//! part of the answer, however legitimately the traversal fetched it.
//!
//! Clause 5.7.1.4 answers a retrieval that keeps nothing with
//! `ResourceNotFound`: "If the optional Attribute list is present and the
//! NGSI-LD endpoint does know about a matching Entity for the Entity ID, but
//! this Entity does not have any of the Attributes in the Attribute list, then
//! an error of type `ResourceNotFound` shall be raised." Tables 6.4.3.2-1 and
//! 5.2.14.1-1 make `attrs` "a synonym for pick, except that id, type, scope are
//! not allowed", so a `pick` that matches nothing is the same failure, and so
//! is an `omit` naming every member the entity has. A query answers the same
//! question by dropping the entity from the payload instead, and clauses
//! 5.7.3.5 and 5.7.4.5 carry both answers to the temporal operations.
//!
//! Finally, Table 5.2.4-1 makes `location`, `observationSpace` and
//! `operationSpace` Attributes of the Entity like any other -- clause 4.7 fixes
//! what they mean, not what they are. Andromeda keeps all three in Entity
//! members of their own, and every narrowing that selected Attributes by name
//! read the rest of the attribute map only, so all three survived `attrs`,
//! `pick` and `omit` alike.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{header::jsonld_link_header, response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The problem type clause 5.7.1.4 names for an entity the projection empties.
const RESOURCE_NOT_FOUND: &str = "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound";

/// The temporal window clause 5.7.3.3 needs beside the projection under test.
///
/// It is placed on `modifiedAt`, which every instance a Core write recorded
/// carries: the fixture's instances declare no `observedAt`, and a window on
/// that default axis would select none of them, leaving no Temporal Evolution
/// to project at all (clause 5.7.3.4, step S1).
const WINDOW: &str = "timeproperty=modifiedAt&timerel=after&timeAt=2000-01-01T00:00:00Z";

/// A Building carrying one Property, one Relationship and the clause 4.7
/// default geo-property, so one request can watch all three narrowings.
fn building(id: &str, target: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {"type": "Property", "value": "Depot"},
        "linkedTo": {"type": "Relationship", "object": target},
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// A City linking on to a Country, so a two-level traversal has somewhere to go.
fn city(id: &str, target: &str) -> Value {
    json!({
        "id": id,
        "type": "City",
        "name": {"type": "Property", "value": "Paris"},
        "linkedTo": {"type": "Relationship", "object": target},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// The entity at the end of the chain.
fn country(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Country",
        "name": {"type": "Property", "value": "France"},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Create one entity, asserting the broker accepted it.
async fn create(client: &TestClient, entity: &Value) {
    let created = client.post_entity(entity).send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
}

/// Send one read against a root-relative path.
async fn read(client: &TestClient, path: &str) -> TestResponse {
    client
        .raw(Method::GET, path)
        .header("Accept", "application/json")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("test-suite context Link must be valid"))
        .send()
        .await
        .expect("read request")
}

/// Read one entity, asserting a 200, and answer with the body.
async fn read_ok(client: &TestClient, path: &str) -> Value {
    let response = read(client, path).await;
    assert_eq!(response.status, StatusCode::OK, "{path}: {:?}", response.body);
    response.json_value().expect("a 200 carries a JSON body")
}

/// The members of a collection body, in response order.
fn members(body: &Value) -> &Vec<Value> {
    body.as_array().expect("a collection body is an array")
}

/// Assert a read was refused as clause 5.7.1.4's `ResourceNotFound`.
fn assert_not_found(response: &TestResponse, scenario: &str) {
    assert_eq!(response.status, StatusCode::NOT_FOUND, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(RESOURCE_NOT_FOUND), "{scenario}: {problem}");
    assert!(problem["title"].is_string(), "{scenario}: a problem carries a title: {problem}");
}

/// Table 5.2.4-1 makes the three clause 4.7 geo-properties Attributes of the
/// Entity, so every selection that names Attributes reaches them: `attrs` and
/// `pick` keep only what they list, and `omit` removes what it lists.
#[tokio::test]
async fn regression_projection_reach_the_default_geo_properties_are_ordinary_attributes() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:geo-projection");
    let target = scope.id("City:geo-projection");
    create(client, &building(&id, &target)).await;

    let selected = read_ok(client, &format!("entities/{id}?attrs=name")).await;
    assert!(selected.get("name").is_some(), "the selected attribute survives: {selected}");
    assert!(selected.get("location").is_none(), "`attrs` reaches the default geo-property: {selected}");

    let picked = read_ok(client, &format!("entities/{id}?pick=id,name")).await;
    assert!(picked.get("name").is_some(), "the picked attribute survives: {picked}");
    assert!(picked.get("location").is_none(), "`pick` reaches the default geo-property: {picked}");

    let omitted = read_ok(client, &format!("entities/{id}?omit=location")).await;
    assert!(omitted.get("name").is_some(), "`omit` leaves what it does not name: {omitted}");
    assert!(omitted.get("location").is_none(), "`omit` reaches the default geo-property: {omitted}");

    let kept = read_ok(client, &format!("entities/{id}?pick=id,location")).await;
    assert!(kept.get("location").is_some(), "a `pick` that names it keeps it: {kept}");
    assert!(kept.get("name").is_none(), "and drops what it does not name: {kept}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.1.4: a retrieval whose projection leaves no Entity member is a
/// `ResourceNotFound`, whether the projection is a `pick` that matches nothing
/// or an `omit` that names every member the entity has.
#[tokio::test]
async fn regression_empty_projection_a_retrieval_left_with_no_member_is_not_found() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:empty-projection");
    let target = scope.id("City:empty-projection");
    create(client, &building(&id, &target)).await;

    for parameters in [
        "pick=unknown",
        "pick=unknown,alsoUnknown",
        "attrs=unknown",
        "omit=id,type,name,linkedTo,location",
    ] {
        assert_not_found(&read(client, &format!("entities/{id}?{parameters}")).await, parameters);
    }

    // One surviving member is enough to answer with, whichever member it is.
    for parameters in ["pick=id", "pick=type", "pick=unknown,id", "pick=location", "omit=id,type,name,linkedTo"] {
        let body = read_ok(client, &format!("entities/{id}?{parameters}")).await;
        assert!(body.as_object().is_some_and(|map| !map.is_empty()), "{parameters}: {body}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.2.5 reduces "every Entity within the payload body", so a query
/// answers the same question by leaving the emptied entity out of the payload
/// rather than by refusing the request.
#[tokio::test]
async fn regression_empty_projection_a_query_drops_the_entity_it_empties() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let kept = scope.id("Building:query-kept");
    let emptied = scope.id("Building:query-emptied");
    let target = scope.id("City:query-emptied");
    create(client, &building(&kept, &target)).await;
    create(
        client,
        &json!({"id": emptied, "type": "Building", "category": {"type": "Property", "value": "commercial"}, "@context": [TEST_SUITE_CONTEXT]}),
    )
    .await;

    let body = read_ok(
        client,
        &format!("entities?type=Building&idPattern={}.%2A&pick=name", scope.id("Building:query-")),
    )
    .await;
    assert_eq!(members(&body).len(), 1, "only the entity with a surviving member is in the payload: {body}");
    assert!(members(&body)[0].get("name").is_some(), "{body}");

    // The same query with a `pick` nothing matches answers an empty collection
    // rather than a 404: the operation succeeded, it simply matched nothing.
    let none = read_ok(
        client,
        &format!("entities?type=Building&idPattern={}.%2A&pick=unknown", scope.id("Building:query-")),
    )
    .await;
    assert!(members(&none).is_empty(), "an emptied result set is an empty collection: {none}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.3.5 reduces the Temporal Evolution to the defined members too, so
/// the temporal retrieval carries the clause 5.7.1.4 answer and the temporal
/// query carries the clause 5.7.2.5 one.
#[tokio::test]
async fn regression_empty_projection_the_temporal_operations_carry_the_same_rule() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:temporal-projection");
    let target = scope.id("City:temporal-projection");
    create(client, &building(&id, &target)).await;

    for parameters in ["pick=unknown", "omit=id,type,name,linkedTo,location"] {
        assert_not_found(
            &read(client, &format!("temporal/entities/{id}?{WINDOW}&{parameters}")).await,
            &format!("temporal retrieve with {parameters}"),
        );
    }

    let kept = read_ok(client, &format!("temporal/entities/{id}?{WINDOW}&pick=id,name")).await;
    assert_eq!(kept["id"].as_str(), Some(id.as_str()), "a projection that keeps a member answers: {kept}");

    let emptied = read_ok(client, &format!("temporal/entities?{WINDOW}&type=Building&id={id}&pick=unknown")).await;
    assert!(members(&emptied).is_empty(), "the temporal query drops what it empties: {emptied}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.1.5 applies the restriction "to all Entities in the payload in
/// the case of Linked Entity retrieval", so a Linked Entity keeps `id` and
/// `type` only when the nested terms name them -- identically in both join
/// modes, because the same clause governs both.
#[tokio::test]
async fn regression_linked_projection_both_join_modes_narrow_identity_by_the_nested_terms() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:linked-identity");
    let target = scope.id("City:linked-identity");
    let onward = scope.id("Country:linked-identity");
    create(client, &building(&id, &target)).await;
    create(client, &city(&target, &onward)).await;
    create(client, &country(&onward)).await;

    let inline = read_ok(client, &format!("entities/{id}?join=inline&joinLevel=1&pick=id,linkedTo%7Bid,name%7D")).await;
    let nested = &inline["linkedTo"]["entity"];
    assert_eq!(nested["id"].as_str(), Some(target.as_str()), "the nested pick names `id`: {nested}");
    assert!(nested.get("name").is_some(), "the nested pick names `name`: {nested}");
    assert!(nested.get("type").is_none(), "the nested pick names no `type`: {nested}");

    let flat = read_ok(client, &format!("entities/{id}?join=flat&joinLevel=1&pick=id,linkedTo%7Bid,name%7D")).await;
    assert_eq!(members(&flat).len(), 2, "the root and what it links to: {flat}");
    assert_eq!(members(&flat)[1], *nested, "both join modes leave the same linked entity: {flat}");

    // The bare form of the same term: no keyword nested, no identity left.
    let bare = read_ok(client, &format!("entities/{id}?join=flat&joinLevel=1&pick=id,linkedTo%7Bname%7D")).await;
    let linked = &members(&bare)[1];
    assert!(linked.get("id").is_none(), "the nested pick names no `id`: {linked}");
    assert!(linked.get("type").is_none(), "the nested pick names no `type`: {linked}");
    assert!(linked.get("name").is_some(), "and keeps what it does name: {linked}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.21: a bare `omit` term drops the Attribute from the entity that
/// carries it, while a nested one leaves the Relationship standing and travels
/// one level down -- so an `omit` never removes a Linked Entity's identity
/// unless it names it.
#[tokio::test]
async fn regression_linked_projection_a_nested_omit_leaves_the_identity_it_does_not_name() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:linked-omit");
    let target = scope.id("City:linked-omit");
    let onward = scope.id("Country:linked-omit");
    create(client, &building(&id, &target)).await;
    create(client, &city(&target, &onward)).await;
    create(client, &country(&onward)).await;

    let flat = read_ok(client, &format!("entities/{id}?join=flat&joinLevel=1&omit=linkedTo%7Bname%7D")).await;
    let root = &members(&flat)[0];
    assert!(root.get("linkedTo").is_some(), "a nested omit leaves the Relationship: {root}");
    let linked = &members(&flat)[1];
    assert_eq!(linked["id"].as_str(), Some(target.as_str()), "the nested omit names no `id`: {linked}");
    assert_eq!(linked["type"].as_str(), Some("City"), "the nested omit names no `type`: {linked}");
    assert!(linked.get("name").is_none(), "and removes what it does name: {linked}");

    let named = read_ok(client, &format!("entities/{id}?join=flat&joinLevel=1&omit=linkedTo%7Bid,type%7D")).await;
    let linked = &members(&named)[1];
    assert!(linked.get("id").is_none(), "a nested omit that names `id` removes it: {linked}");
    assert!(linked.get("type").is_none(), "a nested omit that names `type` removes it: {linked}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.1.5 appends to the flattened array what the *returned* Attributes
/// name, so a Relationship the projection removed brings nothing along and
/// whatever was reachable only through it leaves the array with it.
#[tokio::test]
async fn regression_flat_membership_only_the_returned_relationships_name_the_array() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:flat-membership");
    let target = scope.id("City:flat-membership");
    let onward = scope.id("Country:flat-membership");
    create(client, &building(&id, &target)).await;
    create(client, &city(&target, &onward)).await;
    create(client, &country(&onward)).await;

    // Both hops survive: the City keeps the Relationship that names the Country.
    let whole = read_ok(
        client,
        &format!("entities/{id}?join=flat&joinLevel=2&pick=id,linkedTo%7Bid,linkedTo%7Bid%7D%7D"),
    )
    .await;
    assert_eq!(
        whole
            .as_array()
            .map(|members| members.iter().filter_map(|member| member["id"].as_str()).collect::<Vec<_>>()),
        Some(vec![id.as_str(), target.as_str(), onward.as_str()]),
        "the roots precede the entities the traversal reached, in traversal order: {whole}"
    );

    // The City's own Relationship is projected away, so the Country it named is
    // no longer named by anything the response returns.
    let cut = read_ok(client, &format!("entities/{id}?join=flat&joinLevel=2&pick=id,linkedTo%7Bid,name%7D")).await;
    assert_eq!(
        cut.as_array()
            .map(|members| members.iter().filter_map(|member| member["id"].as_str()).collect::<Vec<_>>()),
        Some(vec![id.as_str(), target.as_str()]),
        "the entity reachable only through the removed Relationship is gone: {cut}"
    );

    // A `pick` that keeps no Relationship at all names no Linked Entity.
    let none = read_ok(client, &format!("entities/{id}?join=flat&joinLevel=2&pick=id,name")).await;
    assert_eq!(members(&none).len(), 1, "nothing is linked to any more: {none}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.21's union rule for `join=flat`: the array renders each Linked
/// Entity once, so a target two Relationships reach renders unrestricted when
/// either of them carried no sub-projection.
#[tokio::test]
async fn regression_linked_projection_two_relationships_sharing_a_target_take_the_union() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:union-root");
    let target = scope.id("City:union-target");
    let onward = scope.id("Country:union-target");
    create(
        client,
        &json!({
            "id": id,
            "type": "Building",
            "linkedTo": {"type": "Relationship", "object": target},
            "alsoLinkedTo": {"type": "Relationship", "object": target},
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    create(client, &city(&target, &onward)).await;
    create(client, &country(&onward)).await;

    let body = read_ok(client, &format!("entities/{id}?join=flat&joinLevel=1&pick=id,linkedTo%7Bname%7D,alsoLinkedTo")).await;
    let linked = &members(&body)[1];
    assert_eq!(
        linked["id"].as_str(),
        Some(target.as_str()),
        "the unrestricted reach widens the target: {linked}"
    );
    assert_eq!(linked["type"].as_str(), Some("City"), "{linked}");
    assert!(linked.get("name").is_some(), "{linked}");

    // Two nested sub-projections instead: the union is what both named.
    let narrowed = read_ok(
        client,
        &format!("entities/{id}?join=flat&joinLevel=1&pick=id,linkedTo%7Bname%7D,alsoLinkedTo%7Bid%7D"),
    )
    .await;
    let linked = &members(&narrowed)[1];
    assert_eq!(linked["id"].as_str(), Some(target.as_str()), "one term named `id`: {linked}");
    assert!(linked.get("name").is_some(), "the other named `name`: {linked}");
    assert!(linked.get("type").is_none(), "neither named `type`: {linked}");

    scope.finish().await.expect("cleanup failed");
}

/// The query binding carries the linked-entity rule exactly as the retrieval
/// does (clause 5.7.2.5), at both join modes and at two traversal levels.
#[tokio::test]
async fn regression_linked_projection_the_query_binding_carries_the_same_rule() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:query-linked");
    let target = scope.id("City:query-linked");
    let onward = scope.id("Country:query-linked");
    create(client, &building(&id, &target)).await;
    create(client, &city(&target, &onward)).await;
    create(client, &country(&onward)).await;

    let inline = read_ok(
        client,
        &format!("entities?type=Building&id={id}&join=inline&joinLevel=2&pick=id,linkedTo%7Bid,linkedTo%7Bname%7D%7D"),
    )
    .await;
    let root = &members(&inline)[0];
    let first = &root["linkedTo"]["entity"];
    assert_eq!(first["id"].as_str(), Some(target.as_str()), "{first}");
    assert!(first.get("name").is_none(), "the first level named no `name`: {first}");
    let second = &first["linkedTo"]["entity"];
    assert!(second.get("id").is_none(), "the second level named no `id`: {second}");
    assert!(second.get("name").is_some(), "the second level named `name`: {second}");

    let flat = read_ok(
        client,
        &format!("entities?type=Building&id={id}&join=flat&joinLevel=2&pick=id,linkedTo%7Bid,linkedTo%7Bname%7D%7D"),
    )
    .await;
    assert_eq!(members(&flat).len(), 3, "the root and both entities the traversal reached: {flat}");
    let flat_first = &members(&flat)[1];
    assert_eq!(flat_first["id"].as_str(), Some(target.as_str()), "{flat_first}");
    assert!(flat_first.get("name").is_none(), "the first level named no `name`: {flat_first}");
    let flat_second = &members(&flat)[2];
    assert!(flat_second.get("id").is_none(), "the second level named no `id`: {flat_second}");
    assert!(flat_second.get("name").is_some(), "the second level named `name`: {flat_second}");

    scope.finish().await.expect("cleanup failed");
}
