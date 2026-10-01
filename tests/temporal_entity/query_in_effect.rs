//! A temporal `q` reads the instance in effect when the window opens.
//!
//! Clause 5.7.4.4 step S2 checks `q` against the Attribute instances "during
//! the interval defined by the temporal query", and clause 4.11 makes those
//! the values "valid within the specified timeframe". Annex C clause C.5.5
//! reads that as including the value observed before the window and never
//! replaced: it is evaluated by `q` but not rendered. Six Meters exercise every
//! way a stream can enter the window:
//!
//! - M1: `power` 5 before the window and 9 inside it.
//! - M2: `power` 9 before the window only, still in effect when it opens.
//! - M3: `power` 9 before the window, then the attribute deleted before it
//!   opens, so the deletion is what is in effect (clause 4.5.7).
//! - M4: `power` 9 before the window and 1 at exactly its start, which clause
//!   4.11 makes inclusive, so 1 replaces 9 as the window opens.
//! - M5: two `power` instances at one instant before the window, 1 and 9; one
//!   of them satisfying the term is enough (clause 4.9).
//! - M6: `power` 9 on dataset `a` and 1 on the default dataset before the
//!   window; step S2 reads every dataset, the `datasetId` parameter being step
//!   S8.
//!
//! Every Meter also holds one `status` inside the window, which is what step
//! S1 keeps them for under `attrs=status`; `power` is only queried. The
//! deletion is a real Core deletion, recorded at the instant it is made, so
//! every instant is placed relative to when the test runs: the window opens a
//! quarter of an hour after the deletion.

use crate::{BASE_URL, client, create_temporal_entity_json, delete_entity_attr, query_temporal_entities};
use andromeda_test_harness::environment::TestEnvironment;
use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use pretty_assertions::assert_eq;
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};
use tokio::time::sleep;

/// The Meters, their instants, and the instant between their `power` writes
/// and their `status` writes.
struct Meters {
    /// The instant every observation is placed relative to: three quarters of
    /// an hour before the test started.
    base: DateTime<Utc>,
    /// The broker-side instant every `power` write and the deletion precede and
    /// every `status` write follows, for a window on `modifiedAt`.
    between_writes: DateTime<Utc>,
}

impl Meters {
    /// The instant `minutes` after the base, as a request writes it.
    fn at(&self, minutes: i64) -> String {
        minutes_after(self.base, minutes)
    }
}

/// The instant `minutes` after `base`, as a request writes it.
fn minutes_after(base: DateTime<Utc>, minutes: i64) -> String {
    (base + TimeDelta::minutes(minutes)).to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// One `power` instance observed `minutes` after `base`.
fn power(base: DateTime<Utc>, minutes: i64, value: i64) -> Value {
    json!({ "type": "Property", "value": value, "observedAt": minutes_after(base, minutes) })
}

/// Create one Meter through the Temporal API with the given `power` instances.
async fn create_meter(c: &Client, id: &str, instances: Vec<Value>) {
    let entity = json!({ "id": id, "type": "Meter", "power": instances });
    assert_eq!(
        create_temporal_entity_json(c, &entity, "application/json", None).await,
        StatusCode::CREATED,
        "{id}"
    );
}

/// Append the one in-window `status` instance to a Meter.
async fn append_status(c: &Client, id: &str, observed_at: &str) {
    let fragment = json!({ "status": { "type": "Property", "value": "on", "observedAt": observed_at } });
    let status = c
        .post(format!("{BASE_URL}/temporal/entities/{id}/attrs"))
        .header("Content-Type", "application/json")
        .json(&fragment)
        .send()
        .await
        .expect("request failed")
        .status();
    assert_eq!(status, StatusCode::NO_CONTENT, "{id}");
}

/// Write the six Meters described in the module documentation.
async fn seed(c: &Client, scope: &TestEnvironment) -> Meters {
    let base = DateTime::from_timestamp(Utc::now().timestamp(), 0).expect("a representable instant") - TimeDelta::minutes(45);
    create_meter(c, &scope.id("Meter:M1"), vec![power(base, 30, 5), power(base, 90, 9)]).await;
    create_meter(c, &scope.id("Meter:M2"), vec![power(base, 30, 9)]).await;
    create_meter(c, &scope.id("Meter:M4"), vec![power(base, 30, 9), power(base, 60, 1)]).await;
    create_meter(c, &scope.id("Meter:M5"), vec![power(base, 30, 1), power(base, 30, 9)]).await;
    let mut on_dataset = power(base, 30, 9);
    on_dataset["datasetId"] = json!("urn:ngsi-ld:Dataset:a");
    create_meter(c, &scope.id("Meter:M6"), vec![on_dataset, power(base, 50, 1)]).await;

    // Clause 4.5.7: a Core deletion is recorded in the history as an instance
    // with no value, placed at the instant it was made.
    let m3 = scope.id("Meter:M3");
    let core = json!({ "id": m3, "type": "Meter", "power": power(base, 30, 9) });
    let created = c
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/json")
        .json(&core)
        .send()
        .await
        .expect("request failed")
        .status();
    assert_eq!(created, StatusCode::CREATED);
    assert_eq!(delete_entity_attr(c, &m3, "power", None).await, StatusCode::NO_CONTENT);

    // The broker stamps `modifiedAt` on its own clock; a second either side
    // keeps every write unambiguously before or after this instant.
    sleep(Duration::from_secs(1)).await;
    let between_writes = Utc::now();
    sleep(Duration::from_secs(1)).await;

    let meters = Meters { base, between_writes };
    for meter in ["M1", "M2", "M3", "M4", "M5", "M6"] {
        append_status(c, &scope.id(&format!("Meter:{meter}")), &meters.at(75)).await;
    }
    meters
}

/// The identifiers of a query answer, in answer order.
fn ids_of(body: &Value) -> Vec<String> {
    body.as_array()
        .expect("a query answers with an array")
        .iter()
        .map(|entity| entity["id"].as_str().expect("every entity has an id").to_owned())
        .collect()
}

/// The scoped identifiers of the named Meters.
fn meters(scope: &TestEnvironment, names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| scope.id(&format!("Meter:{name}"))).collect()
}

/// Clause 5.7.4.4 S2 with clause 4.11 and Annex C clause C.5.5: M1 is selected
/// by its in-window 9, M2 by the 9 still in effect when the window opens, M5
/// by the 9 among its two instances at one instant, M6 by the 9 of its dataset
/// `a` stream. M3 holds a deletion when the window opens and M4 the 1 that
/// replaced its 9 at the inclusive start. `power` is evaluated, not rendered:
/// each Meter answers with its one in-window `status` alone.
#[tokio::test]
async fn temporal_query_q_admits_the_instance_in_effect_when_the_window_opens() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let meters_written = seed(&c, &scope).await;

    let query = format!(
        "type=Meter&q=power>8&attrs=status&timerel=between&timeAt={}&endTimeAt={}&lastN=1",
        meters_written.at(60),
        meters_written.at(120)
    );
    let (status, body, _) = query_temporal_entities(&c, &scope, &query, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let selected: BTreeSet<String> = ids_of(&body).into_iter().collect();
    assert_eq!(selected, meters(&scope, &["M1", "M2", "M5", "M6"]));

    for entity in body.as_array().expect("a query answers with an array") {
        assert!(entity.get("power").is_none(), "{entity}");
        let instances: Vec<&Value> = match &entity["status"] {
            Value::Array(instances) => instances.iter().collect(),
            single @ Value::Object(_) => vec![single],
            other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => panic!("status must be rendered: {other}"),
        };
        let observed: Vec<&str> = instances
            .iter()
            .map(|instance| instance["observedAt"].as_str().expect("an observedAt"))
            .collect();
        assert_eq!(observed, vec![meters_written.at(75).as_str()], "{entity}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.9: the count totals the entities the pages hold, and the pages
/// follow the identifier order.
#[tokio::test]
async fn temporal_query_q_count_matches_the_page() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let meters_written = seed(&c, &scope).await;

    let query = format!(
        "type=Meter&q=power>8&attrs=status&timerel=between&timeAt={}&endTimeAt={}&lastN=1&count=true&limit=2",
        meters_written.at(60),
        meters_written.at(120)
    );
    let (status, first, headers) = query_temporal_entities(&c, &scope, &query, None).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let count = headers
        .get("NGSILD-Results-Count")
        .expect("a counted query carries the count")
        .to_str()
        .expect("a textual count");
    assert_eq!(count, "4");
    let (status, second, _) = query_temporal_entities(&c, &scope, &format!("{query}&offset=2"), None).await;
    assert_eq!(status, StatusCode::OK, "{second}");

    let expected: Vec<String> = meters(&scope, &["M1", "M2", "M5", "M6"]).into_iter().collect();
    assert_eq!(ids_of(&first), expected[..2].to_vec());
    assert_eq!(ids_of(&second), expected[2..].to_vec());

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.2.21: the same rule holds on `modifiedAt`, where an instance is
/// placed at the instant the broker wrote it. Every `power` instance and the
/// deletion were written before the window opens and every `status` after it,
/// so each Meter's in-effect `power` values are those of its last `power`
/// write: M4's 9 and 1 were written together and both are in effect, and M3
/// holds its deletion. Only M3 is left out.
#[tokio::test]
async fn temporal_query_q_on_modified_at_uses_the_same_rule() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let meters_written = seed(&c, &scope).await;

    let query = format!(
        "type=Meter&q=power>8&attrs=status&timeproperty=modifiedAt&timerel=after&timeAt={}",
        meters_written.between_writes.to_rfc3339_opts(SecondsFormat::Millis, true)
    );
    let (status, body, _) = query_temporal_entities(&c, &scope, &query, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let selected: BTreeSet<String> = ids_of(&body).into_iter().collect();
    assert_eq!(selected, meters(&scope, &["M1", "M2", "M4", "M5", "M6"]));

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.11: an `after` interval opens at its instant exactly as `between`
/// does, so it admits the same instances in effect.
#[tokio::test]
async fn temporal_query_q_with_after_admits_the_prior_instance() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let meters_written = seed(&c, &scope).await;

    let query = format!("type=Meter&q=power>8&attrs=status&timerel=after&timeAt={}", meters_written.at(60));
    let (status, body, _) = query_temporal_entities(&c, &scope, &query, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let selected: BTreeSet<String> = ids_of(&body).into_iter().collect();
    assert_eq!(selected, meters(&scope, &["M1", "M2", "M5", "M6"]));

    scope.finish().await.expect("cleanup failed");
}
