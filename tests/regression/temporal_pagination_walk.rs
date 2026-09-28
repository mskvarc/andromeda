//! Walking a long Temporal Evolution page by page, forwards and backwards.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3.10: "for requests where the parameter lastN
//! is present, pagination shall happen 'backwards' (from the most recent to the
//! least recent timestamp in the requested time range). For requests without
//! the parameter lastN, pagination shall happen 'forwards' (from the least
//! recent to the most recent timestamp in the requested time range)." Without
//! `lastN` the broker used to apply its safety cap as an implicit `lastN`: it
//! answered with the *newest* instances while advertising a forward
//! `Content-Range` from `timeAt`, so a client resuming forwards from `range-end`
//! received nothing and never saw the older history (real-world campaign
//! finding A-19). The cap was also the largest `lastN` a client could ask for
//! (A-20).
//!
//! Every page of a response is cut at one instant across all its attributes and
//! entities, so a client resuming from the single `Content-Range` loses nothing;
//! a page never splits the instances sharing the timestamp at its edge, so a
//! forward walk meets that group again (clause 4.11 makes `after` inclusive) and
//! drops it by `instanceId` (clause 4.5.7), and a backward walk resumes with the
//! exclusive `before` and meets nothing twice.
//!
//! The base tier pins both page sizes to 19 (`ANDROMEDA__TEMPORAL__DEFAULT_LAST_N`
//! and `ANDROMEDA__TEMPORAL__MAX_LAST_N` in `docker/compose/test/base` and the
//! xtask host broker), which is what [`DEFAULT_PAGE`] and [`MAXIMUM`] mirror;
//! the shipped defaults of 100 and 1,000 are asserted by the unit tests of
//! `andromeda-temporal`.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_http_headers::ngsild_content_range::NgsiLdContentRange;
use andromeda_ngsi_ld_lexicon::rfc3339::sub_second;
use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use chrono::{DateTime, Duration, TimeZone, Utc};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashSet},
    str::FromStr,
};

/// Instances per attribute a read without `lastN` returns on the base tier.
const DEFAULT_PAGE: usize = 19;

/// The largest `lastN` the base tier answers in full.
const MAXIMUM: usize = 19;

/// The instances one create request carries.
const CHUNK: usize = 400;

/// The first instant of every series here.
fn origin() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).single().expect("valid instant")
}

/// One Property instance carrying the number `value`, observed at `at`.
fn instance(value: u64, at: DateTime<Utc>) -> Value {
    json!({"type": "Property", "value": value, "observedAt": sub_second(at)})
}

/// Create one Temporal Evolution from `instances` of each attribute, in
/// several requests when it is long: clause 5.6.11 appends to an evolution that
/// already exists.
async fn create_series(client: &TestClient, id: &str, entity_type: &str, attributes: &[(&str, Vec<Value>)]) {
    let longest = attributes.iter().map(|(_, instances)| instances.len()).max().unwrap_or(0);
    let mut start = 0;
    loop {
        let mut body = json!({"id": id, "type": entity_type, "@context": [TEST_SUITE_CONTEXT]});
        for (name, instances) in attributes {
            let chunk: Vec<Value> = instances.iter().skip(start).take(CHUNK).cloned().collect();
            if !chunk.is_empty() {
                body[*name] = Value::Array(chunk);
            }
        }
        let response = client.create_temporal_entity(&body).await.expect("create temporal entity request");
        assert!(
            response.status == StatusCode::CREATED || response.status == StatusCode::NO_CONTENT,
            "create {id} from {start}: {:?}",
            response.body
        );
        start += CHUNK;
        if start >= longest {
            break;
        }
    }
}

/// Retrieve the Temporal Evolution of `id` under `query`.
async fn retrieve(client: &TestClient, id: &str, query: &str) -> TestResponse {
    client
        .get_temporal_entity(id, query, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve temporal entity request")
}

/// The clause 6.3.10 `Content-Range` of a partial response.
fn content_range(response: &TestResponse) -> NgsiLdContentRange {
    let raw = response
        .headers
        .get("content-range")
        .and_then(|value| value.to_str().ok())
        .expect("a 206 carries a Content-Range");
    NgsiLdContentRange::from_str(raw).expect("a clause 6.3.10 Content-Range")
}

/// The instances of `name` one normalized temporal representation carries.
fn instances_of<'a>(entity: &'a Value, name: &str) -> Vec<&'a Value> {
    match &entity[name] {
        Value::Array(instances) => instances.iter().collect(),
        Value::Null => Vec::new(),
        single @ (Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => vec![single],
    }
}

/// The instant an instance was observed at.
fn observed(instance: &Value) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(instance["observedAt"].as_str().expect("an observedAt"))
        .expect("an RFC 3339 instant")
        .with_timezone(&Utc)
}

/// The value an instance carries.
fn value_of(instance: &Value) -> u64 {
    instance["value"].as_u64().expect("a numeric value")
}

/// A long series: one `speed` instance per second, with a group of five
/// instances sharing one timestamp every 37 seconds so groups keep landing on
/// page edges, and a `fuel` series every other second, offset by one.
fn long_series() -> Vec<(&'static str, Vec<Value>)> {
    let mut speed = Vec::new();
    let mut value = 0;
    for second in 0..2350 {
        let at = origin() + Duration::seconds(second);
        let copies = if second % 37 == 0 { 5 } else { 1 };
        for _ in 0..copies {
            speed.push(instance(value, at));
            value += 1;
        }
    }
    let fuel = (0..1200)
        .map(|index| instance(100_000 + index, origin() + Duration::seconds(2 * i64::try_from(index).expect("small") + 1)))
        .collect();
    vec![("speed", speed), ("fuel", fuel)]
}

/// The values every attribute of `series` was written with.
fn written_values(series: &[(&str, Vec<Value>)]) -> BTreeSet<(String, u64)> {
    series
        .iter()
        .flat_map(|(name, instances)| instances.iter().map(|instance| ((*name).to_owned(), value_of(instance))))
        .collect()
}

/// Clause 6.3.10 forwards: a walk without `lastN` that resumes each page with
/// `timerel=after&timeAt=<range-end>` collects exactly the stored instances.
/// The only instances it meets twice are the group sharing a page's
/// `range-end`, which the inclusive `after` hands back and the walk drops by
/// `instanceId`.
#[tokio::test]
async fn regression_temporal_pagination_a_forward_walk_collects_every_instance_exactly_once() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("PagedSeries:forward");
    let series = long_series();
    create_series(client, &id, "PagedSeries", &series).await;

    let mut collected: BTreeSet<(String, u64)> = BTreeSet::new();
    let mut seen_instances: HashSet<String> = HashSet::new();
    let mut time_at = sub_second(origin() - Duration::days(1));
    let mut previous_end: Option<DateTime<Utc>> = None;
    let mut pages = 0;
    loop {
        pages += 1;
        assert!(pages < 1000, "the walk advances on every page");
        let response = retrieve(client, &id, &format!("timerel=after&timeAt={time_at}")).await;
        let entity = response.json_value().expect("a temporal entity");
        for name in ["speed", "fuel"] {
            for instance in instances_of(&entity, name) {
                let instance_id = instance["instanceId"].as_str().expect("an instanceId").to_owned();
                if seen_instances.insert(instance_id) {
                    collected.insert((name.to_owned(), value_of(instance)));
                } else {
                    assert_eq!(
                        Some(observed(instance)),
                        previous_end,
                        "only the group on the previous range-end is met twice: {instance}"
                    );
                }
            }
        }
        match response.status {
            StatusCode::OK => break,
            StatusCode::PARTIAL_CONTENT => {
                let range = content_range(&response);
                assert_eq!(range.total(), None, "a forward page reports size '*'");
                assert!(previous_end.is_none_or(|end| range.end() > end), "every page ends later than the one before");
                previous_end = Some(range.end());
                time_at = sub_second(range.end());
            }
            other => panic!("page {pages}: unexpected {other}: {:?}", response.body),
        }
    }
    assert!(pages > 50, "the history is far longer than one page, so the walk took many: {pages}");
    assert_eq!(collected, written_values(&series), "the forward walk collected exactly the stored instances");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.10 backwards: a walk with a `lastN` above the maximum that
/// resumes each page with `timerel=before&timeAt=<range-end>` collects exactly
/// the stored instances and never meets one twice, `before` being exclusive.
#[tokio::test]
async fn regression_temporal_pagination_a_backward_walk_collects_every_instance_exactly_once() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("PagedSeries:backward");
    let series = long_series();
    create_series(client, &id, "PagedSeries", &series).await;

    let mut collected: BTreeSet<(String, u64)> = BTreeSet::new();
    let mut time_at = sub_second(origin() + Duration::days(1));
    let mut previous_end: Option<DateTime<Utc>> = None;
    let mut pages = 0;
    loop {
        pages += 1;
        assert!(pages < 1000, "the walk advances on every page");
        let response = retrieve(client, &id, &format!("lastN=1000&timerel=before&timeAt={time_at}")).await;
        let entity = response.json_value().expect("a temporal entity");
        for name in ["speed", "fuel"] {
            for instance in instances_of(&entity, name) {
                assert!(
                    collected.insert((name.to_owned(), value_of(instance))),
                    "a backward walk meets no instance twice: {instance}"
                );
            }
        }
        match response.status {
            StatusCode::OK => break,
            StatusCode::PARTIAL_CONTENT => {
                let range = content_range(&response);
                assert_eq!(range.total(), Some(1000), "a backward page reports the requested lastN as its size");
                assert!(previous_end.is_none_or(|end| range.end() < end), "every page ends earlier than the one before");
                previous_end = Some(range.end());
                time_at = sub_second(range.end());
            }
            other => panic!("page {pages}: unexpected {other}: {:?}", response.body),
        }
    }
    assert!(pages > 50, "the history is far longer than one page, so the walk took many: {pages}");
    assert_eq!(collected, written_values(&series), "the backward walk collected exactly the stored instances");

    scope.finish().await.expect("cleanup failed");
}

/// Thirty `speed` instances, one per minute from the origin.
fn short_series() -> Vec<(&'static str, Vec<Value>)> {
    vec![(
        "speed",
        (0..30)
            .map(|minute| instance(minute, origin() + Duration::minutes(i64::try_from(minute).expect("small"))))
            .collect(),
    )]
}

/// The instant of the `index`-th instance of [`short_series`].
fn minute(index: i64) -> DateTime<Utc> {
    origin() + Duration::minutes(index)
}

/// A read without `lastN` is the default page of the *least* recent instances,
/// and its `Content-Range` runs forwards from `timeAt` to the most recent
/// instant it carries.
#[tokio::test]
async fn regression_temporal_pagination_a_read_without_last_n_is_the_oldest_default_page() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("PagedSeries:default-page");
    create_series(client, &id, "PagedSeries", &short_series()).await;

    let time_at = origin() - Duration::days(1);
    let response = retrieve(client, &id, &format!("timerel=after&timeAt={}", sub_second(time_at))).await;
    assert_eq!(response.status, StatusCode::PARTIAL_CONTENT, "{:?}", response.body);
    let entity = response.json_value().expect("a temporal entity");
    let values: Vec<u64> = instances_of(&entity, "speed").into_iter().map(value_of).collect();
    assert_eq!(
        values,
        (0..u64::try_from(DEFAULT_PAGE).expect("small")).collect::<Vec<_>>(),
        "the least recent instances come first"
    );

    let range = content_range(&response);
    assert_eq!(range.start(), time_at, "clause 6.3.10: a forward after-page starts at timeAt");
    assert_eq!(range.end(), minute(i64::try_from(DEFAULT_PAGE).expect("small") - 1));
    assert_eq!(range.total(), None);

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.10: a `lastN` above the maximum is narrowed to the maximum and
/// paged backwards, the `Content-Range` size still the requested number; a
/// `lastN` within it is the complete answer, a 200 with no `Content-Range`.
#[tokio::test]
async fn regression_temporal_pagination_last_n_above_the_maximum_is_paged_and_within_it_answered_in_full() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("PagedSeries:maximum");
    create_series(client, &id, "PagedSeries", &short_series()).await;

    let above = retrieve(client, &id, "lastN=5000&timerel=after&timeAt=2020-01-01T00:00:00Z").await;
    assert_eq!(above.status, StatusCode::PARTIAL_CONTENT, "{:?}", above.body);
    let entity = above.json_value().expect("a temporal entity");
    let values: Vec<u64> = instances_of(&entity, "speed").into_iter().map(value_of).collect();
    let newest_first = u64::try_from(30 - MAXIMUM).expect("small");
    assert_eq!(values, (newest_first..30).collect::<Vec<_>>(), "the most recent instances, up to the maximum");
    let range = content_range(&above);
    assert_eq!(range.total(), Some(5000), "clause 6.3.10: size is the requested lastN");
    assert_eq!(range.start(), minute(29), "a backward after-page starts at its most recent instant");
    assert_eq!(range.end(), minute(i64::try_from(newest_first).expect("small")));

    let within = retrieve(client, &id, "lastN=10&timerel=after&timeAt=2020-01-01T00:00:00Z").await;
    assert_eq!(within.status, StatusCode::OK, "{:?}", within.body);
    assert!(within.headers.get("content-range").is_none(), "a complete answer carries no Content-Range");
    let entity = within.json_value().expect("a temporal entity");
    assert_eq!(instances_of(&entity, "speed").len(), 10);

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.10 fixes `range-start` and `range-end` per direction and per
/// `timerel`: forwards from `timeAt` (`after`, `between`) or the least recent
/// instant (`before`) to the most recent one; backwards from `timeAt`
/// (`before`), `endTimeAt` (`between`) or the most recent instant (`after`) to
/// the least recent one.
#[tokio::test]
async fn regression_temporal_pagination_content_range_follows_direction_and_timerel() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("PagedSeries:content-range");
    create_series(client, &id, "PagedSeries", &short_series()).await;

    let before = origin() + Duration::days(1);
    let after = origin() - Duration::days(1);
    let forward_end = minute(i64::try_from(DEFAULT_PAGE).expect("small") - 1);
    let backward_end = minute(i64::try_from(30 - MAXIMUM).expect("small"));
    let cases = [
        (format!("timerel=after&timeAt={}", sub_second(after)), after, forward_end, None),
        (format!("timerel=before&timeAt={}", sub_second(before)), minute(0), forward_end, None),
        (
            format!("timerel=between&timeAt={}&endTimeAt={}", sub_second(after), sub_second(before)),
            after,
            forward_end,
            None,
        ),
        (format!("lastN=25&timerel=before&timeAt={}", sub_second(before)), before, backward_end, Some(25)),
        (
            format!("lastN=25&timerel=between&timeAt={}&endTimeAt={}", sub_second(after), sub_second(before)),
            before,
            backward_end,
            Some(25),
        ),
        (
            format!("lastN=25&timerel=after&timeAt={}", sub_second(after)),
            minute(29),
            backward_end,
            Some(25),
        ),
    ];
    for (query, start, end, size) in cases {
        let response = retrieve(client, &id, &query).await;
        assert_eq!(response.status, StatusCode::PARTIAL_CONTENT, "{query}: {:?}", response.body);
        let range = content_range(&response);
        assert_eq!((range.start(), range.end(), range.total()), (start, end, size), "{query}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// One response carries one `Content-Range`, so a query page is cut at one
/// instant across all its entities: an entity whose history lies entirely
/// beyond the cut is still in the page, its attribute present and empty, and
/// its instances arrive on the next page.
#[tokio::test]
async fn regression_temporal_pagination_a_query_page_is_cut_at_one_instant_across_entities() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("PagedFleet");
    let early = scope.id("PagedFleet:early");
    let late = scope.id("PagedFleet:late");
    create_series(client, &early, &entity_type, &short_series()).await;
    create_series(
        client,
        &late,
        &entity_type,
        &[(
            "speed",
            (0..3)
                .map(|index| {
                    instance(
                        500 + index,
                        origin() + Duration::days(200) + Duration::minutes(i64::try_from(index).expect("small")),
                    )
                })
                .collect(),
        )],
    )
    .await;

    let query = format!("type={entity_type}&timerel=after&timeAt=2020-01-01T00:00:00Z");
    let response = client
        .query_temporal_entities(&query, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query temporal entities request");
    assert_eq!(response.status, StatusCode::PARTIAL_CONTENT, "{:?}", response.body);
    let entities = response.json_value().expect("a temporal query answer");
    let entities = entities.as_array().expect("a collection");
    assert_eq!(entities.len(), 2, "both entities are in the page: {entities:?}");
    let by_id = |id: &str| entities.iter().find(|entity| entity["id"] == json!(id)).expect("the entity is in the page");
    assert_eq!(instances_of(by_id(&early), "speed").len(), DEFAULT_PAGE);
    assert_eq!(by_id(&late)["speed"], json!([]), "the entity beyond the cut keeps its attribute, empty");
    let range = content_range(&response);
    assert_eq!(
        range.end(),
        minute(i64::try_from(DEFAULT_PAGE).expect("small") - 1),
        "the one range ends at the common cut"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.9 renders an attribute in `temporalValues` as its family's member
/// holding "as many array elements as Property instances ... being
/// represented", so the attribute of an entity whose history lies entirely
/// beyond the common cut stays, with its `type` and no pair. The renderer
/// dropped it, so a client reading every entity of the page by attribute -- ETSI
/// `021_15_07` -- found no `speed` on the later entity.
#[tokio::test]
async fn regression_temporal_pagination_a_temporal_values_page_keeps_an_emptied_attribute_with_its_type() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_type = scope.entity_type("PagedValuesFleet");
    let early = scope.id("PagedValuesFleet:early");
    let late = scope.id("PagedValuesFleet:late");
    create_series(client, &early, &entity_type, &short_series()).await;
    create_series(client, &late, &entity_type, &[("speed", vec![instance(500, origin() + Duration::days(200))])]).await;

    let query = format!("type={entity_type}&timerel=after&timeAt=2020-01-01T00:00:00Z&options=temporalValues");
    let response = client
        .query_temporal_entities(&query, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query temporal entities request");
    assert_eq!(response.status, StatusCode::PARTIAL_CONTENT, "{:?}", response.body);
    let entities = response.json_value().expect("a temporal query answer");
    let entities = entities.as_array().expect("a collection");
    let by_id = |id: &str| entities.iter().find(|entity| entity["id"] == json!(id)).expect("the entity is in the page");
    assert_eq!(by_id(&early)["speed"]["values"].as_array().map(Vec::len), Some(DEFAULT_PAGE));
    assert_eq!(
        by_id(&late)["speed"],
        json!({"type": "Property", "values": []}),
        "the entity beyond the cut keeps its attribute, typed and empty"
    );

    scope.finish().await.expect("cleanup failed");
}

/// A page never splits the instances sharing the timestamp at its edge, so a
/// group larger than the maximum could not be paged at all: it is refused with
/// `TooManyResults` (clause 5.5.x, 403) rather than rendered without bound.
#[tokio::test]
async fn regression_temporal_pagination_a_simultaneous_group_above_the_maximum_is_too_many_results() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("PagedSeries:oversized-group");
    let group = u64::try_from(MAXIMUM + 6).expect("small");
    let mut speed: Vec<Value> = (0..group).map(|value| instance(value, origin())).collect();
    speed.extend((0..30).map(|index| instance(1000 + index, origin() + Duration::minutes(1 + i64::try_from(index).expect("small")))));
    create_series(client, &id, "PagedSeries", &[("speed", speed)]).await;

    let response = retrieve(client, &id, "timerel=after&timeAt=2020-01-01T00:00:00Z").await;
    assert_eq!(response.status, StatusCode::FORBIDDEN, "{:?}", response.body);
    let problem = response.json_value().expect("a problem details body");
    assert_eq!(problem["type"], json!("https://uri.etsi.org/ngsi-ld/errors/TooManyResults"), "{problem}");

    scope.finish().await.expect("cleanup failed");
}
