//! `q` linked-entity traversal cases: the clause 4.9 `rel{…}` axis, which asks
//! a question about the entity a Relationship names rather than about the
//! entity being filtered.
//!
//! Every case here asserts on *which roots survive the filter*, never on the
//! linked entities themselves — the axis selects, it does not project. The
//! selection has to happen in the store, so the pagination case pins the
//! result count against a page smaller than the matching set.

use crate::{create, get_one, ids_from_array, query};
use andromeda_test_client::{entity::results_count, entity_attribute::DatasetSelection};
use andromeda_test_fixtures::{context::TEST_SUITE_CONTEXT, problem_detail::BAD_REQUEST_DATA};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use urlencoding::encode;

/// An entity of `entity_type` carrying a single string Property.
fn typed_with_property(id: &str, entity_type: &str, attribute: &str, value: &str) -> Value {
    json!({
        "id": id,
        "type": entity_type,
        attribute: {"type": "Property", "value": value},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// An entity of `entity_type` whose only attribute is a Relationship.
fn typed_with_relationship(id: &str, entity_type: &str, relationship: &str, target: &str) -> Value {
    json!({
        "id": id,
        "type": entity_type,
        relationship: {"type": "Relationship", "object": target},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// A Car parked in two places at once: one `isParkedIn` instance per target,
/// each carrying its own `datasetId` as clause 4.5.5 requires of a multi-attribute.
fn car_parked_in_two(id: &str, first: (&str, &str), second: (&str, &str)) -> Value {
    json!({
        "id": id,
        "type": "Car",
        "isParkedIn": [
            {"type": "Relationship", "object": first.1, "datasetId": first.0},
            {"type": "Relationship", "object": second.1, "datasetId": second.0}
        ],
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// A Car with a numeric `speed` alongside its `isParkedIn` Relationship.
fn car_with_speed(id: &str, speed: u32, building: &str) -> Value {
    json!({
        "id": id,
        "type": "Car",
        "speed": {"type": "Property", "value": speed},
        "isParkedIn": {"type": "Relationship", "object": building},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Restrict a query to the entities this test created, so a suite running its
/// cases concurrently cannot contribute rows to an exact-match assertion.
///
/// `hops` is how many Relationships the filter crosses. Clause 5.7.2.4 admits a
/// traversing filter only where Linked Entity retrieval is specified, and bounds
/// it by `joinLevel`, so every case requests the traversal it needs. `inline` is
/// the mode that leaves the result set alone: clause 4.5.23.2 has an inline
/// response "only consist of Linking Entities", so each case stays an assertion
/// about *which roots the filter selects*.
fn scoped(scope: &TestEnvironment, entity_type: &str, filter: &str, hops: u32) -> String {
    format!(
        "type={entity_type}&idPattern={pattern}&q={filter}&join=inline&joinLevel={hops}",
        pattern = encode(&format!("^{}:", scope.id_prefix())),
        filter = encode(filter),
    )
}

/// Clause 4.9: `rel{attr==value}` compares an attribute of the entity the
/// Relationship names, and keeps the root that owns the Relationship — not the
/// entity the comparison was made against.
#[tokio::test]
async fn q_linked_single_hop_filters_roots() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_commercial = scope.id("Building:qs-commercial");
    let id_industrial = scope.id("Building:qs-industrial");
    let id_car_match = scope.id("Car:qs-C1");
    let id_car_other = scope.id("Car:qs-C2");

    create(client, &typed_with_property(&id_commercial, "Building", "category", "commercial")).await;
    create(client, &typed_with_property(&id_industrial, "Building", "category", "industrial")).await;
    create(client, &typed_with_relationship(&id_car_match, "Car", "isParkedIn", &id_commercial)).await;
    create(client, &typed_with_relationship(&id_car_other, "Car", "isParkedIn", &id_industrial)).await;

    let body = query(client, &scoped(&scope, "Car", "isParkedIn{category==\"commercial\"}", 1)).await;

    assert_eq!(ids_from_array(&body), vec![id_car_match]);
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9: the `EntityType 1*("," EntityType) ":"` prefix inside the braces
/// constrains the hop's landing entity to the listed types. A target whose
/// attribute satisfies the comparison but whose type is absent from the list
/// must not make its root match.
#[tokio::test]
async fn q_linked_hop_with_type_guard() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_building = scope.id("Building:qg-listed");
    let id_warehouse = scope.id("Warehouse:qg-unlisted");
    let id_car_building = scope.id("Car:qg-toBuilding");
    let id_car_warehouse = scope.id("Car:qg-toWarehouse");

    create(client, &typed_with_property(&id_building, "Building", "category", "commercial")).await;
    create(client, &typed_with_property(&id_warehouse, "Warehouse", "category", "commercial")).await;
    create(client, &typed_with_relationship(&id_car_building, "Car", "isParkedIn", &id_building)).await;
    create(client, &typed_with_relationship(&id_car_warehouse, "Car", "isParkedIn", &id_warehouse)).await;

    let body = query(client, &scoped(&scope, "Car", "isParkedIn{Building,Garage:category==\"commercial\"}", 1)).await;

    assert_eq!(
        ids_from_array(&body),
        vec![id_car_building],
        "the guard lists Building and Garage, so the Warehouse-parked car is excluded"
    );
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9: hops chain, so `a{b{attr==value}}` crosses two Relationships and
/// still answers about the outermost root.
#[tokio::test]
async fn q_linked_two_level_nesting() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_north = scope.id("District:qn-north");
    let id_south = scope.id("District:qn-south");
    let id_building_north = scope.id("Building:qn-north");
    let id_building_south = scope.id("Building:qn-south");
    let id_car_north = scope.id("Car:qn-north");
    let id_car_south = scope.id("Car:qn-south");

    create(client, &typed_with_property(&id_north, "District", "zone", "north")).await;
    create(client, &typed_with_property(&id_south, "District", "zone", "south")).await;
    create(client, &typed_with_relationship(&id_building_north, "Building", "inDistrict", &id_north)).await;
    create(client, &typed_with_relationship(&id_building_south, "Building", "inDistrict", &id_south)).await;
    create(client, &typed_with_relationship(&id_car_north, "Car", "isParkedIn", &id_building_north)).await;
    create(client, &typed_with_relationship(&id_car_south, "Car", "isParkedIn", &id_building_south)).await;

    let body = query(client, &scoped(&scope, "Car", "isParkedIn{inDistrict{zone==\"north\"}}", 2)).await;

    assert_eq!(ids_from_array(&body), vec![id_car_north]);
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9: an attribute name with no comparison is an existence test, and
/// it keeps that meaning behind a hop — the root matches when the linked entity
/// carries the attribute at all, whatever its value.
#[tokio::test]
async fn q_linked_exists_form() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_categorised = scope.id("Building:qe-categorised");
    let id_plain = scope.id("Building:qe-plain");
    let id_car_categorised = scope.id("Car:qe-categorised");
    let id_car_plain = scope.id("Car:qe-plain");

    create(client, &typed_with_property(&id_categorised, "Building", "category", "industrial")).await;
    create(client, &typed_with_property(&id_plain, "Building", "name", "Depot")).await;
    create(client, &typed_with_relationship(&id_car_categorised, "Car", "isParkedIn", &id_categorised)).await;
    create(client, &typed_with_relationship(&id_car_plain, "Car", "isParkedIn", &id_plain)).await;

    let body = query(client, &scoped(&scope, "Car", "isParkedIn{category}", 1)).await;

    assert_eq!(
        ids_from_array(&body),
        vec![id_car_categorised],
        "the existence form matches any value, but still only where the attribute is present"
    );
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9: a hop is one term of the boolean expression like any other, so
/// `;` intersects it with a predicate evaluated locally on the root.
#[tokio::test]
async fn q_linked_combined_with_local_predicate() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_commercial = scope.id("Building:qc-commercial");
    let id_industrial = scope.id("Building:qc-industrial");
    let id_both = scope.id("Car:qc-both");
    let id_slow = scope.id("Car:qc-slow");
    let id_wrong_building = scope.id("Car:qc-wrongBuilding");

    create(client, &typed_with_property(&id_commercial, "Building", "category", "commercial")).await;
    create(client, &typed_with_property(&id_industrial, "Building", "category", "industrial")).await;
    create(client, &car_with_speed(&id_both, 80, &id_commercial)).await;
    create(client, &car_with_speed(&id_slow, 20, &id_commercial)).await;
    create(client, &car_with_speed(&id_wrong_building, 90, &id_industrial)).await;

    let body = query(client, &scoped(&scope, "Car", "speed>50;isParkedIn{category==\"commercial\"}", 1)).await;

    assert_eq!(
        ids_from_array(&body),
        vec![id_both],
        "neither the local predicate alone nor the hop alone may admit a root"
    );
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9 with clause 6.3.10: `count` reports the size of the filtered set,
/// not the size of the page. A hop that were evaluated after paging would count
/// the unfiltered roots, or count only the survivors of the first page — both
/// of which this case rejects.
#[tokio::test]
async fn q_linked_pagination_counts_filtered_set() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_commercial = scope.id("Building:qp-commercial");
    let id_industrial = scope.id("Building:qp-industrial");

    create(client, &typed_with_property(&id_commercial, "Building", "category", "commercial")).await;
    create(client, &typed_with_property(&id_industrial, "Building", "category", "industrial")).await;
    for suffix in ["m1", "m2", "m3"] {
        create(
            client,
            &typed_with_relationship(&scope.id(&format!("Car:qp-{suffix}")), "Car", "isParkedIn", &id_commercial),
        )
        .await;
    }
    for suffix in ["x1", "x2"] {
        create(
            client,
            &typed_with_relationship(&scope.id(&format!("Car:qp-{suffix}")), "Car", "isParkedIn", &id_industrial),
        )
        .await;
    }

    let request = format!("{}&limit=2&count=true", scoped(&scope, "Car", "isParkedIn{category==\"commercial\"}", 1));
    let response = client
        .query_entities(&request, "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query_entities failed");
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let body = response.json_value().expect("response body must be valid JSON");

    assert_eq!(results_count(&response.headers), Some(3), "the count covers all matching roots, not the page");
    assert_eq!(ids_from_array(&body).len(), 2, "the page is bounded by limit");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9 with clause 4.5.5: a Relationship may have several instances,
/// each distinguished by its `datasetId` and each naming a different entity.
/// The hop is an existential over those instances — one satisfying instance
/// admits the root, and a root all of whose instances miss is excluded.
#[tokio::test]
async fn q_linked_hop_matches_any_relationship_instance() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_commercial = scope.id("Building:qm-commercial");
    let id_industrial = scope.id("Building:qm-industrial");
    let id_depot = scope.id("Building:qm-depot");
    let id_dataset_first = scope.id("Dataset:qm-first");
    let id_dataset_second = scope.id("Dataset:qm-second");
    let id_car_mixed = scope.id("Car:qm-mixed");
    let id_car_neither = scope.id("Car:qm-neither");

    create(client, &typed_with_property(&id_commercial, "Building", "category", "commercial")).await;
    create(client, &typed_with_property(&id_industrial, "Building", "category", "industrial")).await;
    create(client, &typed_with_property(&id_depot, "Building", "category", "industrial")).await;
    create(
        client,
        &car_parked_in_two(&id_car_mixed, (&id_dataset_first, &id_commercial), (&id_dataset_second, &id_industrial)),
    )
    .await;
    create(
        client,
        &car_parked_in_two(&id_car_neither, (&id_dataset_first, &id_industrial), (&id_dataset_second, &id_depot)),
    )
    .await;

    // Without this the case could pass vacuously: had the broker collapsed the
    // two instances into one, the surviving instance alone would still produce
    // the expected result set.
    let stored = get_one(client, &id_car_mixed, "").await;
    assert_eq!(
        stored["isParkedIn"].as_array().map(Vec::len),
        Some(2),
        "the multi-attribute must be stored as two instances: {stored}"
    );

    let body = query(client, &scoped(&scope, "Car", "isParkedIn{category==\"commercial\"}", 1)).await;

    assert_eq!(
        ids_from_array(&body),
        vec![id_car_mixed],
        "one matching instance is enough, and a root whose every instance misses is excluded"
    );
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9 with clause 5.6.5: an attribute that has been deleted names
/// nothing, so the hop cannot cross it. Current state holds what an entity
/// carries now, so the deletion removes the projection row rather than marking
/// it, and there is no sentinel for the probe to exclude by hand. The deletion
/// itself survives in Temporal Evolution, which clause 5.6.6 leaves standing.
///
/// The pre-deletion assertion is what makes this a test of the deletion: it
/// establishes that both roots matched for the same reason, so the change after
/// the delete can only be the removed attribute.
#[tokio::test]
async fn q_linked_hop_does_not_match_through_a_deleted_relationship() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let id_commercial = scope.id("Building:qt-commercial");
    let id_car_live = scope.id("Car:qt-live");
    let id_car_tombstoned = scope.id("Car:qt-tombstoned");

    create(client, &typed_with_property(&id_commercial, "Building", "category", "commercial")).await;
    create(client, &typed_with_relationship(&id_car_live, "Car", "isParkedIn", &id_commercial)).await;
    create(client, &typed_with_relationship(&id_car_tombstoned, "Car", "isParkedIn", &id_commercial)).await;

    let filter = scoped(&scope, "Car", "isParkedIn{category==\"commercial\"}", 1);
    let before = query(client, &filter).await;
    let mut before_ids = ids_from_array(&before);
    before_ids.sort();
    let before_ids = before_ids;
    let mut expected_before = vec![id_car_live.clone(), id_car_tombstoned.clone()]; // clone: both identifiers are asserted on again after the deletion.
    expected_before.sort();
    assert_eq!(before_ids, expected_before, "both roots reach the commercial building before the delete");

    let deleted = client
        .delete_entity_attr(&id_car_tombstoned, "isParkedIn", None, DatasetSelection::Default, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("delete_entity_attr failed");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{:?}", deleted.body);

    let after = query(client, &filter).await;
    assert_eq!(ids_from_array(&after), vec![id_car_live], "the hop must not cross a tombstoned Relationship");

    // The projection is written inside the transaction that mutates the entity,
    // so the delete's effect on it is visible the moment the request returned
    // and this read needs no barrier. The entity carried exactly one attribute,
    // so the deletion must leave it with no projection row at all: an absent row
    // is what a deleted instance is, and nothing the probe has to filter out.
    let projected: Vec<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT c.value_string, c.value_uri FROM current_attribute_values c \
         JOIN entities e ON e.entity_key = c.entity_key WHERE e.id = $1",
    )
    .bind(&id_car_tombstoned)
    .fetch_all(scope.database_pool())
    .await
    .expect("current_attribute_values probe failed");
    assert!(
        projected.is_empty(),
        "the deleted Relationship must leave no projection row behind: {projected:?}"
    );
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.2.4: a `q` hop asks the broker to walk into another entity, and a
/// request where "use of Linked Entity retrieval is not specified" asked for no
/// such walk. Clause 4.5.23 makes `@none` the parameter's default, so writing it
/// is the same request as omitting it and both are refused.
#[tokio::test]
async fn q_linked_without_a_retrieving_join_is_rejected() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let filter = encode("isParkedIn{category==\"commercial\"}");

    for query in [format!("type=Car&q={filter}"), format!("type=Car&q={filter}&join={}", encode("@none"))] {
        let response = client
            .query_entities(&query, "application/ld+json", Some(TEST_SUITE_CONTEXT))
            .await
            .expect("query_entities failed");

        assert_eq!(response.status, StatusCode::BAD_REQUEST, "{:?}", response.body);
        let body = response.json_value().expect("response body must be valid JSON");
        assert_eq!(body["type"].as_str(), Some(BAD_REQUEST_DATA.as_str()), "for {query}");
    }
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.2.4 "too deep query": a two-hop chain overspends the one hop a
/// `joinLevel` of one grants, and becomes legal as soon as the declared
/// traversal reaches as deep as the filter does.
#[tokio::test]
async fn q_linked_deeper_than_the_join_level_is_rejected() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();
    let filter = encode("isParkedIn{inDistrict{zone==\"north\"}}");

    let too_deep = client
        .query_entities(
            &format!("type=Car&q={filter}&join=inline&joinLevel=1"),
            "application/ld+json",
            Some(TEST_SUITE_CONTEXT),
        )
        .await
        .expect("query_entities failed");

    assert_eq!(too_deep.status, StatusCode::BAD_REQUEST, "{:?}", too_deep.body);
    let body = too_deep.json_value().expect("response body must be valid JSON");
    assert_eq!(body["type"].as_str(), Some(BAD_REQUEST_DATA.as_str()));

    let deep_enough = client
        .query_entities(
            &format!("type=Car&q={filter}&join=inline&joinLevel=2"),
            "application/ld+json",
            Some(TEST_SUITE_CONTEXT),
        )
        .await
        .expect("query_entities failed");

    assert_eq!(deep_enough.status, StatusCode::OK, "{:?}", deep_enough.body);
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9: an unterminated hop is not a filter, and a filter that cannot be
/// parsed is invalid protocol data rather than a filter matching nothing.
#[tokio::test]
async fn q_linked_malformed_rejected() {
    let scope = TestEnvironment::from_suite("join").await.expect("isolated test environment");
    let client = scope.client();

    let response = client
        .query_entities(&format!("type=Car&q={}", encode("rel{")), "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query_entities failed");

    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{:?}", response.body);
    let body = response.json_value().expect("response body must be valid JSON");
    assert_eq!(body["type"].as_str(), Some(BAD_REQUEST_DATA.as_str()));
    scope.finish().await.expect("cleanup failed");
}
