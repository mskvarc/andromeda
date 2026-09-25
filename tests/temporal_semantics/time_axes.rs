//! Independent time axes and missing observed time.
//!
//! Clause 5.2.21 admits `observedAt`, `createdAt`, `modifiedAt` and `deletedAt`
//! as the `timeproperty` a temporal query selects, and clause 4.11 makes
//! `observedAt` the default. Each is stored in its own column and answered from
//! that column alone; the operational write clock is never one of them and
//! never stands in for a missing value.

use crate::{
    attribute_iri,
    client,
    create_entity,
    database,
    delete_entity_attribute,
    get_temporal_entity,
    rendered_instances,
    stored_history::{StoredHistoryRow, stored_attribute_history},
};
use andromeda_test_harness::environment::TestEnvironment;
use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use pretty_assertions::{assert_eq, assert_ne};
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The observation instant the fixture declares, far from every write instant.
const DECLARED_OBSERVED_AT: &str = "2021-01-01T00:00:00Z";

/// Render one instant in the spelling a temporal query parameter takes.
fn query_instant(instant: DateTime<Utc>) -> String {
    instant.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// A `between` window one second either side of the given instant.
fn window_around(instant: DateTime<Utc>) -> String {
    format!(
        "timerel=between&timeAt={}&endTimeAt={}",
        query_instant(instant - TimeDelta::seconds(1)),
        query_instant(instant + TimeDelta::seconds(1))
    )
}

/// A `between` window one second either side of the declared observation.
fn declared_observation_window() -> String {
    let observed = DECLARED_OBSERVED_AT.parse::<DateTime<Utc>>().expect("a valid fixture instant");
    window_around(observed)
}

/// The single stored row of one attribute, failing the test when it is absent.
fn only_row(rows: &[StoredHistoryRow], attribute: &str) -> StoredHistoryRow {
    assert_eq!(rows.len(), 1, "{attribute} must have recorded exactly one history row");
    rows[0].clone()
}

/// Clause 4.11 and clause 5.2.21: four deliberately different timestamps, each
/// selected only by the `timeproperty` that names its own column.
#[tokio::test]
async fn temporal_semantics_independent_time_axes_each_select_only_their_own_column() {
    let scope = TestEnvironment::from_suite("temporal_semantics").await.expect("isolated test environment");
    let http = client();
    let pool = database().await;
    let entity_id = scope.id("Vehicle:time_axes");

    let created = create_entity(
        &http,
        &json!({
            "id": entity_id,
            "type": "Vehicle",
            "speed": { "type": "Property", "value": 55, "observedAt": DECLARED_OBSERVED_AT }
        }),
    )
    .await;
    assert_eq!(created, StatusCode::CREATED);
    assert_eq!(delete_entity_attribute(&http, &entity_id, "speed").await, StatusCode::NO_CONTENT);

    let rows = stored_attribute_history(&pool, &entity_id, &attribute_iri("speed")).await;
    assert_eq!(rows.len(), 2, "the observation and its tombstone are two rows");
    let observation = rows[0].clone();
    let tombstone = rows[1].clone();

    let observed_at = observation.observed_at.expect("the observation declares observedAt");
    let created_at = observation.created_at.expect("the broker assigns createdAt");
    let modified_at = observation.modified_at.expect("the broker assigns modifiedAt");
    let deleted_at = tombstone.deleted_at.expect("a tombstone carries deletedAt");
    assert_ne!(observed_at, created_at, "the declared and the assigned instants differ");
    assert_ne!(created_at, deleted_at, "the deletion happened after the creation");
    assert!(
        tombstone.created_at.is_none() && tombstone.modified_at.is_none() && tombstone.observed_at.is_none(),
        "a tombstone carries a value on the deletion axis alone"
    );

    // The default axis window sees the declared observation and nothing else.
    let (status, body) = get_temporal_entity(&http, &entity_id, &declared_observation_window()).await;
    assert_eq!(status, StatusCode::OK);
    let observed_window = rendered_instances(&body, "speed");
    assert_eq!(observed_window.len(), 1, "only the declared observation lies in the observedAt window");
    assert_eq!(observed_window[0]["value"], json!(55));

    // The same window on every other axis holds nothing: no other axis carries
    // a value anywhere near the declared observation instant.
    for timeproperty in ["createdAt", "modifiedAt", "deletedAt"] {
        let (status, body) = get_temporal_entity(&http, &entity_id, &format!("timeproperty={timeproperty}&{}", declared_observation_window())).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            rendered_instances(&body, "speed").is_empty(),
            "{timeproperty} must not answer from the observedAt column"
        );
    }

    // The assignment axes answer from their own column, and each returns the
    // observation that carries a value there rather than the tombstone, which
    // carries none.
    for (timeproperty, instant) in [("createdAt", created_at), ("modifiedAt", modified_at)] {
        let (status, body) = get_temporal_entity(&http, &entity_id, &format!("timeproperty={timeproperty}&{}", window_around(instant))).await;
        assert_eq!(status, StatusCode::OK);
        let matched = rendered_instances(&body, "speed");
        assert_eq!(matched.len(), 1, "{timeproperty} answers from its own column");
        assert_eq!(matched[0]["value"], json!(55), "{timeproperty} returns the instance that carries that instant");
    }

    // The deletion window overlaps the creation and modification instants, and
    // still returns only the tombstone: each axis reads one column and no other.
    let (status, body) = get_temporal_entity(&http, &entity_id, &format!("timeproperty=deletedAt&{}", window_around(deleted_at))).await;
    assert_eq!(status, StatusCode::OK);
    let deleted_window = rendered_instances(&body, "speed");
    assert_eq!(deleted_window.len(), 1, "only the tombstone lies on the deletedAt axis");
    assert!(deleted_window[0].get("deletedAt").is_some(), "the tombstone renders its deletion instant");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.11: an instance recorded without `observedAt` matches no
/// `observedAt` window, and the write clock does not invent one for it.
#[tokio::test]
async fn temporal_semantics_missing_observed_time_is_never_taken_from_the_write_clock() {
    let scope = TestEnvironment::from_suite("temporal_semantics").await.expect("isolated test environment");
    let http = client();
    let pool = database().await;
    let entity_id = scope.id("Vehicle:missing_observed");

    let created = create_entity(
        &http,
        &json!({
            "id": entity_id,
            "type": "Vehicle",
            "speed": { "type": "Property", "value": 55 }
        }),
    )
    .await;
    assert_eq!(created, StatusCode::CREATED);

    let rows = stored_attribute_history(&pool, &entity_id, &attribute_iri("speed")).await;
    let row = only_row(&rows, "speed");
    assert!(row.observed_at.is_none(), "no observedAt was declared, so none is stored");
    let modified_at = row.modified_at.expect("the broker assigns modifiedAt");
    assert!(row.created_at.is_some(), "the broker assigns createdAt");

    // The default axis covers everything storage has ever recorded and still
    // matches nothing, because the row carries no value on that axis.
    let (status, body) = get_temporal_entity(&http, &entity_id, "timerel=after&timeAt=1970-01-01T00:00:00Z").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        rendered_instances(&body, "speed").is_empty(),
        "an unbounded observedAt query must not match an instance that has no observedAt"
    );

    // The axis the instance does carry a value on matches it.
    let (status, body) = get_temporal_entity(&http, &entity_id, &format!("timeproperty=modifiedAt&{}", window_around(modified_at))).await;
    assert_eq!(status, StatusCode::OK);
    let modified_window = rendered_instances(&body, "speed");
    assert_eq!(modified_window.len(), 1, "the modifiedAt window matches the instance");
    assert_eq!(modified_window[0]["value"], json!(55));
    assert!(
        modified_window[0].get("observedAt").is_none(),
        "an absent observedAt is never rendered from the write clock"
    );
    scope.finish().await.expect("cleanup failed");
}

/// The suite's rendering never exposes the operational write clock as a
/// Temporal Property: clause 5.2.21 admits four names, and `recordedAt` is not
/// one of them.
#[tokio::test]
async fn temporal_semantics_the_write_clock_is_never_rendered_as_a_temporal_property() {
    let scope = TestEnvironment::from_suite("temporal_semantics").await.expect("isolated test environment");
    let http = client();
    let entity_id = scope.id("Vehicle:write_clock");

    let created = create_entity(
        &http,
        &json!({
            "id": entity_id,
            "type": "Vehicle",
            "speed": { "type": "Property", "value": 55, "observedAt": DECLARED_OBSERVED_AT }
        }),
    )
    .await;
    assert_eq!(created, StatusCode::CREATED);

    let (status, body) = get_temporal_entity(&http, &entity_id, "").await;
    assert_eq!(status, StatusCode::OK);
    let instances = rendered_instances(&body, "speed");
    assert_eq!(instances.len(), 1);
    let members = instances[0].as_object().expect("an instance renders as an object");
    for rejected in ["recordedAt", "recorded_at", "writeId", "writeOrdinal"] {
        assert!(!members.contains_key(rejected), "{rejected} is not a rendered member");
    }
    assert_eq!(
        members.get("observedAt").and_then(Value::as_str),
        Some(DECLARED_OBSERVED_AT),
        "the declared observation instant renders unchanged"
    );
    scope.finish().await.expect("cleanup failed");
}
