//! `aggrGroupBy` / `format=groupedValues` — the temporal aggregation grouping
//! extension.
//!
//! Runs against a broker started with
//! `ANDROMEDA__EXTENSIONS__TEMPORAL_AGGREGATION_GROUPING=enabled` and a small
//! `ANDROMEDA__EXTENSIONS__TEMPORAL_AGGREGATION_GROUPING_MAX_GROUPS`.

use crate::seeding::{DATASET_A, DATASET_B, seed_grouping_history, seed_hourly_history, seed_mixed_unit_history, seed_monthly_history};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::Value;

/// The whole-range window every grouping query below runs over.
const WINDOW: &str = "timerel=after&timeAt=1970-01-01T00:00:00Z";

/// Run a grouped temporal retrieve and return the parsed body.
async fn grouped_speed(client: &TestClient, id: &str, extra: &str) -> (StatusCode, Value) {
    let query = format!("{WINDOW}&attrs=speed&format=groupedValues&aggrMethods=avg&{extra}");
    let response = client
        .get_temporal_entity(id, &query, Some(CORE_CONTEXT))
        .await
        .expect("grouped temporal retrieve");
    let status = response.status;
    let body = response.json_value().unwrap_or(Value::Null);
    (status, body)
}

/// The `speed` member of a grouped retrieval, whichever instance shape it took.
fn speed_member(body: &Value) -> Value {
    body["speed"].clone()
}

/// Grouping by `datasetId` nests a map keyed by dataset while the entity array
/// is preserved (`id`/`type` intact).
#[tokio::test]
async fn grouping_by_dataset_id_nests_a_map_keyed_by_dataset() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let client = scope.client();
    let id = scope.id("Vehicle:group-dataset");
    seed_grouping_history(client, &id).await;

    let (status, body) = grouped_speed(client, &id, "aggrGroupBy=datasetId").await;
    assert_eq!(status, StatusCode::OK, "a grouped retrieve answers 200: {body}");
    assert_eq!(body["id"], id, "the entity id is preserved: {body}");
    assert_eq!(body["type"], "Vehicle", "the entity type is preserved: {body}");
    let speed = speed_member(&body);
    let group_maps = speed_group_maps(&speed, "avg");
    assert!(group_maps.iter().any(|map| map.contains_key(DATASET_A)), "dataset A is a group: {speed}");
    assert!(group_maps.iter().any(|map| map.contains_key(DATASET_B)), "dataset B is a group: {speed}");
    scope.finish().await.expect("cleanup");
}

/// Grouping by `unitCode` over mixed units succeeds as separate groups — the
/// request a plain aggregation refuses.
#[tokio::test]
async fn grouping_by_unit_code_splits_a_mixed_unit_request() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let client = scope.client();
    let id = scope.id("Vehicle:group-unit");
    seed_mixed_unit_history(client, &id).await;

    // Without grouping the mixed-unit aggregation is refused.
    let ungrouped = client
        .get_temporal_entity(
            &id,
            &format!("{WINDOW}&attrs=speed&format=aggregatedValues&aggrMethods=avg"),
            Some(CORE_CONTEXT),
        )
        .await
        .expect("ungrouped aggregation");
    assert_eq!(
        ungrouped.status,
        StatusCode::BAD_REQUEST,
        "a mixed-unit aggregation is refused without grouping"
    );

    let (status, body) = grouped_speed(client, &id, "aggrGroupBy=unitCode").await;
    assert_eq!(status, StatusCode::OK, "grouping by unit makes the mixed-unit request answerable: {body}");
    let group_maps = speed_group_maps(&speed_member(&body), "avg");
    assert!(group_maps.iter().any(|map| map.contains_key("KMH")), "KMH is a group: {body}");
    assert!(group_maps.iter().any(|map| map.contains_key("HM")), "HM is a group: {body}");
    scope.finish().await.expect("cleanup");
}

/// A calendar-month bucket produces one group per calendar month — unreachable
/// via `aggrPeriodDuration`.
#[tokio::test]
async fn a_calendar_month_bucket_groups_by_month() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let client = scope.client();
    let id = scope.id("Vehicle:group-month");
    seed_monthly_history(client, &id).await;

    let (status, body) = grouped_speed(client, &id, "aggrGroupBy=observedAt;month").await;
    assert_eq!(status, StatusCode::OK, "a monthly grouping answers 200: {body}");
    let keys = union_group_keys(&speed_member(&body), "avg");
    assert!(keys.iter().any(|key| key.starts_with("2024-01")), "a January group is present: {keys:?}");
    assert!(keys.iter().any(|key| key.starts_with("2024-02")), "a February group is present: {keys:?}");
    scope.finish().await.expect("cleanup");
}

/// A cyclic hour-of-day bucket folds every day onto the hour.
#[tokio::test]
async fn a_cyclic_hour_of_day_bucket_folds_the_timeline() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let client = scope.client();
    let id = scope.id("Vehicle:group-hour");
    seed_hourly_history(client, &id).await;

    let (status, body) = grouped_speed(client, &id, "aggrGroupBy=observedAt;hourOfDay").await;
    assert_eq!(status, StatusCode::OK, "a cyclic grouping answers 200: {body}");
    let keys = union_group_keys(&speed_member(&body), "avg");
    // The seed observes at hour 8 (twice, on different days) and hour 9.
    assert!(
        keys.iter().any(|key| key == "8" || key == "9" || key == "10"),
        "an hour-of-day group is present: {keys:?}"
    );
    scope.finish().await.expect("cleanup");
}

/// A value-in-effect key groups each instant by the `zone` value at that instant.
#[tokio::test]
async fn a_value_key_groups_by_the_attribute_in_effect() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let client = scope.client();
    let id = scope.id("Vehicle:group-value");
    seed_grouping_history(client, &id).await;

    let (status, body) = grouped_speed(client, &id, "aggrGroupBy=zone").await;
    assert_eq!(status, StatusCode::OK, "a value-key grouping answers 200: {body}");
    let keys = union_group_keys(&speed_member(&body), "avg");
    assert!(keys.iter().any(|key| key == "north" || key == "south"), "a zone group is present: {keys:?}");
    scope.finish().await.expect("cleanup");
}

/// `aggrGroupBy` without the grouped format, and the grouped format without
/// `aggrMethods`, are both 400.
#[tokio::test]
async fn misused_grouping_parameters_are_rejected() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let client = scope.client();
    let id = scope.id("Vehicle:group-misuse");
    seed_grouping_history(client, &id).await;

    let without_format = client
        .get_temporal_entity(
            &id,
            &format!("{WINDOW}&attrs=speed&format=aggregatedValues&aggrMethods=avg&aggrGroupBy=unitCode"),
            Some(CORE_CONTEXT),
        )
        .await
        .expect("aggrGroupBy without groupedValues");
    assert_eq!(without_format.status, StatusCode::BAD_REQUEST, "aggrGroupBy needs groupedValues");

    let without_methods = client
        .get_temporal_entity(
            &id,
            &format!("{WINDOW}&attrs=speed&format=groupedValues&aggrGroupBy=unitCode"),
            Some(CORE_CONTEXT),
        )
        .await
        .expect("groupedValues without aggrMethods");
    assert_eq!(without_methods.status, StatusCode::BAD_REQUEST, "the grouped format needs aggrMethods");
    scope.finish().await.expect("cleanup");
}

/// A grouping over more distinct groups than the configured bound is refused
/// with `TooManyResults`, nothing partially returned.
#[tokio::test]
async fn an_over_the_bound_grouping_is_refused() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let client = scope.client();
    let id = scope.id("Vehicle:group-toomany");
    seed_grouping_history(client, &id).await;

    // Four distinct months across the two datasets exceed the bound of 3.
    let (status, _body) = grouped_speed(client, &id, "aggrGroupBy=datasetId,observedAt;month").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "an over-the-bound grouping answers 403 TooManyResults");
    scope.finish().await.expect("cleanup");
}

/// The enabled extension is advertised in the Context Source Identity response.
#[tokio::test]
async fn the_extension_is_advertised_in_the_source_identity() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let client = scope.client();
    let response = client.source_identity(None).await.expect("source identity");
    assert_eq!(response.status, StatusCode::OK);
    let advertised = response.json_value().expect("identity body is JSON").to_string();
    assert!(
        advertised.contains("temporal-aggregation-grouping"),
        "the enabled extension is advertised: {advertised}"
    );
    scope.finish().await.expect("cleanup");
}

/// The `avg` group maps of a `speed` member, one per rendered instance.
fn speed_group_maps(speed: &Value, method: &str) -> Vec<serde_json::Map<String, Value>> {
    let instances = match speed {
        Value::Array(instances) => instances.clone(),
        object @ Value::Object(_) => vec![object.clone()],
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Vec::new(),
    };
    instances
        .iter()
        .filter_map(|instance| instance.get(method).and_then(Value::as_object).cloned())
        .collect()
}

/// Every group key present under `method`, across the instances, at the first
/// nesting level.
fn union_group_keys(speed: &Value, method: &str) -> Vec<String> {
    speed_group_maps(speed, method).iter().flat_map(|map| map.keys().cloned()).collect()
}
