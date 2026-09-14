//! Temporal join tests (NGSI-LD 4.5.23 applied to the Temporal API).
//!
//! A temporal answer is a slice of history, so a joined Linked Entity is
//! rendered over exactly the window the roots were rendered over, and only the
//! Relationship instances that fall inside that window name a candidate at all.
//! `join=flat` puts the Linked Entities beside the roots (clause 6.4.3.2);
//! `join=inline` nests each one under the instances that name it.
//!
//! Applying `join` to the Temporal API is a deliberate Andromeda extension:
//! clauses 5.7.3.4 and 5.7.4.4 do not list the parameter among the ones the
//! temporal operations accept.

use crate::{COMPOUND_CONTEXT, TEST_SUITE_CONTEXT, clean_database, client, create_temporal_entity, post_temporal_query, query_temporal_entities, unique_id};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use serial_test::serial;

/// The window every case below reads over.
const WINDOW: &str = "timerel=between&timeAt=2020-08-01T12:00:00Z&endTimeAt=2020-08-01T12:10:00Z";

/// A Vehicle whose speed was recorded twice inside the window.
fn vehicle_with_speed_history(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 30, "observedAt": "2020-08-01T12:03:00Z" },
            { "type": "Property", "value": 40, "observedAt": "2020-08-01T12:05:00Z" }
        ],
        "@context": [COMPOUND_CONTEXT]
    })
}

/// A Vehicle parked in `inside` throughout the window, and in `outside` at an
/// instant well beyond it.
fn vehicle_parked_across_the_window(id: &str, inside: &str, outside: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 10, "observedAt": "2020-08-01T12:03:00Z" },
            { "type": "Property", "value": 20, "observedAt": "2020-08-01T12:05:00Z" }
        ],
        "isParkedIn": [
            { "type": "Relationship", "object": inside, "observedAt": "2020-08-01T12:03:00Z" },
            { "type": "Relationship", "object": inside, "observedAt": "2020-08-01T12:05:00Z" },
            { "type": "Relationship", "object": outside, "observedAt": "2020-09-01T13:00:00Z" }
        ],
        "@context": [COMPOUND_CONTEXT]
    })
}

/// The `id` member of every entity in an array body, in response order.
fn ids_from_array(body: &Value) -> Vec<String> {
    body.as_array()
        .expect("a joined temporal response is an array")
        .iter()
        .filter_map(|entity| entity["id"].as_str().map(str::to_owned))
        .collect()
}

/// The one array member carrying `id`.
fn member_with_id<'a>(body: &'a Value, id: &str) -> &'a Value {
    let member = body
        .as_array()
        .expect("a joined temporal response is an array")
        .iter()
        .find(|entity| entity["id"].as_str() == Some(id));
    assert!(member.is_some(), "no array member with id {id}: {body}");
    member.expect("the array member was just asserted to be present")
}

/// The `observedAt` of every instance a temporal attribute member carries.
fn observed_instants(member: &Value) -> Vec<String> {
    member
        .as_array()
        .expect("a temporal attribute renders as an array of instances")
        .iter()
        .filter_map(|instance| instance["observedAt"].as_str().map(str::to_owned))
        .collect()
}

/// Clause 6.4.3.2: a flat join appends the Linked Entities after the roots, and
/// the Temporal API renders them the way it renders everything else — as the
/// history each attribute had over the requested window.
#[tokio::test]
#[serial]
async fn temporal_join_flat_appends_linked_temporal_entity() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let linked = unique_id("urn:ngsi-ld:Vehicle:tjoin-flat-B");
    let root = unique_id("urn:ngsi-ld:Vehicle:tjoin-flat-A");
    let elsewhere = unique_id("urn:ngsi-ld:Vehicle:tjoin-flat-C");

    assert_eq!(create_temporal_entity(&c, &vehicle_with_speed_history(&linked)).await, StatusCode::CREATED);
    assert_eq!(
        create_temporal_entity(&c, &vehicle_parked_across_the_window(&root, &linked, &elsewhere)).await,
        StatusCode::CREATED
    );

    let (status, body, _) = query_temporal_entities(&c, &format!("id={root}&type=Vehicle&{WINDOW}&join=flat&joinLevel=1"), Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let ids = ids_from_array(&body);
    assert!(ids.contains(&root), "the root must be present: {ids:?}");
    assert!(ids.contains(&linked), "the Linked Entity must accompany the root: {ids:?}");

    let joined = member_with_id(&body, &linked);
    assert_eq!(
        observed_instants(&joined["speed"]),
        vec!["2020-08-01T12:03:00Z".to_owned(), "2020-08-01T12:05:00Z".to_owned()],
        "the Linked Entity renders as temporal history over the same window: {joined}"
    );
}

/// An inline join nests instead of appending: every in-window instance of the
/// Relationship carries the Linked Entity's own windowed history under
/// `entity`, and the collection still holds exactly the roots it started with.
#[tokio::test]
#[serial]
async fn temporal_join_inline_nests_under_instances() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let linked = unique_id("urn:ngsi-ld:Vehicle:tjoin-inline-B");
    let root = unique_id("urn:ngsi-ld:Vehicle:tjoin-inline-A");
    let elsewhere = unique_id("urn:ngsi-ld:Vehicle:tjoin-inline-C");

    assert_eq!(create_temporal_entity(&c, &vehicle_with_speed_history(&linked)).await, StatusCode::CREATED);
    assert_eq!(
        create_temporal_entity(&c, &vehicle_parked_across_the_window(&root, &linked, &elsewhere)).await,
        StatusCode::CREATED
    );

    let (status, body, _) = query_temporal_entities(
        &c,
        &format!("id={root}&type=Vehicle&{WINDOW}&join=inline&joinLevel=1"),
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    assert_eq!(ids_from_array(&body), vec![root.clone()], "an inline join appends nothing");

    let instances = member_with_id(&body, &root)["isParkedIn"]
        .as_array()
        .expect("the Relationship renders as an array of instances")
        .clone();
    assert_eq!(instances.len(), 2, "only the in-window instances are rendered: {instances:?}");
    for instance in &instances {
        assert_eq!(
            instance["entity"]["id"].as_str(),
            Some(linked.as_str()),
            "every instance nests the Linked Entity: {instance}"
        );
        assert_eq!(
            observed_instants(&instance["entity"]["speed"]),
            vec!["2020-08-01T12:03:00Z".to_owned(), "2020-08-01T12:05:00Z".to_owned()],
            "the nested entity carries its own windowed history: {instance}"
        );
    }
}

/// A Relationship instance outside the window is not rendered, so the entity it
/// names is never a traversal candidate — even when that entity has history of
/// its own inside the window.
#[tokio::test]
#[serial]
async fn temporal_join_window_limits_candidates() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let linked = unique_id("urn:ngsi-ld:Vehicle:tjoin-window-B");
    let root = unique_id("urn:ngsi-ld:Vehicle:tjoin-window-A");
    let elsewhere = unique_id("urn:ngsi-ld:Vehicle:tjoin-window-C");

    assert_eq!(create_temporal_entity(&c, &vehicle_with_speed_history(&linked)).await, StatusCode::CREATED);
    assert_eq!(create_temporal_entity(&c, &vehicle_with_speed_history(&elsewhere)).await, StatusCode::CREATED);
    assert_eq!(
        create_temporal_entity(&c, &vehicle_parked_across_the_window(&root, &linked, &elsewhere)).await,
        StatusCode::CREATED
    );

    let (status, body, _) = query_temporal_entities(&c, &format!("id={root}&type=Vehicle&{WINDOW}&join=flat&joinLevel=1"), Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let ids = ids_from_array(&body);
    assert!(ids.contains(&linked), "the in-window Linked Entity must be present: {ids:?}");
    assert!(!ids.contains(&elsewhere), "an entity named only outside the window must not be joined: {ids:?}");
}

/// The POST query binding carries `join` and `joinLevel` in its body exactly as
/// the GET carries them in its query string, and reaches the same traversal.
#[tokio::test]
#[serial]
async fn temporal_post_query_join() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let linked = unique_id("urn:ngsi-ld:Vehicle:tjoin-post-B");
    let root = unique_id("urn:ngsi-ld:Vehicle:tjoin-post-A");
    let elsewhere = unique_id("urn:ngsi-ld:Vehicle:tjoin-post-C");

    assert_eq!(create_temporal_entity(&c, &vehicle_with_speed_history(&linked)).await, StatusCode::CREATED);
    assert_eq!(
        create_temporal_entity(&c, &vehicle_parked_across_the_window(&root, &linked, &elsewhere)).await,
        StatusCode::CREATED
    );

    let query = json!({
        "type": "Query",
        "entities": [{ "id": root, "type": "Vehicle" }],
        "join": "flat",
        "joinLevel": 1,
        "temporalQ": {
            "timerel": "between",
            "timeAt": "2020-08-01T12:00:00Z",
            "endTimeAt": "2020-08-01T12:10:00Z"
        }
    });
    let (status, body, _) = post_temporal_query(&c, &query, Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let ids = ids_from_array(&body);
    assert!(ids.contains(&root), "the root must be present: {ids:?}");
    assert!(ids.contains(&linked), "the Linked Entity must accompany the root: {ids:?}");
    assert!(!ids.contains(&elsewhere), "an entity named only outside the window must not be joined: {ids:?}");
}

/// Clauses 5.7.3.4 and 5.7.4.4 refuse Linked Entity retrieval on the temporal
/// operations outright, raising an error where projection attributes or filter
/// conditions indicate one. The join extension above accepts the parameters and
/// bounds the references instead, by the clause 5.7.2.4 rule: a `q` hop needs a
/// join that retrieves, and `@none` is the parameter's default rather than one.
#[tokio::test]
#[serial]
async fn temporal_query_rejects_a_traversing_filter_without_a_retrieving_join() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let filter = urlencoding::encode("isParkedIn{speed>5}");
    let (status, body, _) = query_temporal_entities(&c, &format!("type=Vehicle&{WINDOW}&q={filter}"), Some(TEST_SUITE_CONTEXT)).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

/// The projection half of the same pair, enforced symmetrically: a nested
/// `pick` term describes a Linked Entity, so it is refused without a retrieving
/// join and honoured once one is requested.
#[tokio::test]
#[serial]
async fn temporal_query_bounds_a_nested_projection_by_the_requested_join() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let linked = unique_id("urn:ngsi-ld:Vehicle:tjoin-pick-B");
    let root = unique_id("urn:ngsi-ld:Vehicle:tjoin-pick-A");
    let elsewhere = unique_id("urn:ngsi-ld:Vehicle:tjoin-pick-C");

    assert_eq!(create_temporal_entity(&c, &vehicle_with_speed_history(&linked)).await, StatusCode::CREATED);
    assert_eq!(
        create_temporal_entity(&c, &vehicle_parked_across_the_window(&root, &linked, &elsewhere)).await,
        StatusCode::CREATED
    );

    let pick = urlencoding::encode("isParkedIn{speed}");
    let (status, body, _) = query_temporal_entities(&c, &format!("id={root}&type=Vehicle&{WINDOW}&pick={pick}"), Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let (status, body, _) = query_temporal_entities(
        &c,
        &format!("id={root}&type=Vehicle&{WINDOW}&pick={pick}&join=flat&joinLevel=1"),
        Some(TEST_SUITE_CONTEXT),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
