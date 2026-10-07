//! The value, geo and scope filters of a split temporal query, decided over the gathered parts of every entity a Context Source contributed to (clause 5.7.4.4).
//!
//! One broker holds part of each entity and a mock Context Source answers with the rest, so each test fixes exactly which part holds which instance. The forwarded request carries no filter, the local read keeps every filter in the store for the entities no source answered with, and an entity a source did answer with is kept when its gathered parts satisfy the filters.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::entity,
    oracle::{assert_status, json_body, results_count_header},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use axum::http::Method;
use serde_json::{Value, json};
use urlencoding::encode;

/// The window every test queries over.
const BEFORE_2030: &str = "timerel=before&timeAt=2030-01-01T00:00:00Z";

/// The path the mock source answers temporal queries on.
const SOURCE: &str = "/split-source";

/// A temporal `Property` instance observed at `observed_at`.
fn observed(value: i64, observed_at: &str) -> Value {
    json!({ "type": "Property", "value": value, "observedAt": observed_at })
}

/// The normalized `Property` instances a source answers with, one per observation (clause 4.5.7).
fn answered_instances(pairs: &[(i64, &str)]) -> Value {
    Value::Array(pairs.iter().map(|(value, at)| observed(*value, at)).collect())
}

/// The normalized `GeoProperty` instance a source answers with, located at `coordinates` at noon (clause 4.5.7).
fn located(coordinates: [f64; 2]) -> Value {
    json!([{ "type": "GeoProperty", "value": { "type": "Point", "coordinates": coordinates }, "observedAt": "2020-08-01T12:00:00Z" }])
}

/// The `id` strings of a temporal array body.
fn ids(body: &Value) -> Vec<String> {
    body.as_array()
        .map(|entries| entries.iter().filter_map(|entry| entry["id"].as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

/// The temporal entity with the given id from a temporal array body.
fn temporal_entity(body: &Value, id: &str) -> Value {
    body.as_array()
        .and_then(|entries| entries.iter().find(|entry| entry["id"].as_str() == Some(id)))
        .cloned()
        .unwrap_or(Value::Null)
}

/// The values a `temporalValues` attribute holds, in pair order.
fn pair_values(attribute: &Value) -> Vec<Value> {
    attribute["values"]
        .as_array()
        .map(|pairs| pairs.iter().map(|pair| pair[0].clone()).collect())
        .unwrap_or_default()
}

/// Create the local part of an entity on the front broker.
async fn create_local(topology: &MultiBrokerTopology<BrokerRole>, id: &str, vtype: &str, attributes: &[(&str, Value)]) {
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    assert_status(
        &front.client().create_temporal_entity(&entity(id, vtype, attributes)).await.expect("local part"),
        201,
    );
}

/// Register the mock as an inclusive temporal source of `vtype` that answers
/// the next temporal query with `answer`.
async fn register_source(topology: &MultiBrokerTopology<BrokerRole>, mock: &ContextSourceAdapter, vtype: &str, answer: &Value) {
    register(topology, mock, vtype).await;
    mock.set_stub_with_body(Method::GET, &source_path(), 200, answer).await;
}

/// Register the mock as an inclusive temporal source of `vtype`.
async fn register(topology: &MultiBrokerTopology<BrokerRole>, mock: &ContextSourceAdapter, vtype: &str) {
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let registration_id = topology.id("registration:split-settlement");
    assert_status(
        &front
            .client()
            .create_registration(
                &CsrBuilder::new(&registration_id, &mock.url(SOURCE))
                    .mode("inclusive")
                    .entity_type(vtype)
                    .operations(&["queryTemporal"])
                    .build(),
            )
            .await
            .expect("register"),
        201,
    );
}

/// The temporal query resource the mock source answers on.
fn source_path() -> String {
    format!("{SOURCE}/ngsi-ld/v1/temporal/entities")
}

/// Run a split temporal query over `vtype` and return its body and
/// `NGSILD-Results-Count`.
async fn split_query(topology: &MultiBrokerTopology<BrokerRole>, vtype: &str, filters: &str) -> (Value, Option<usize>) {
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let response = front
        .client()
        .query_temporal_entities(&format!("type={}&{BEFORE_2030}&splitEntities=true{filters}", encode(vtype)), None)
        .await
        .expect("query");
    assert_status(&response, 200);
    (json_body(&response), results_count_header(&response))
}

/// Clause 5.7.4.4: "If no values filter query is provided, then S5 is equal to S4". With no filter nothing is evaluated, so a `temporalValues` entity -- whose plural `values` member no write would accept -- passes through.
#[tokio::test]
async fn distop_split_temporal_query_temporal_values_without_filter_keeps_every_entity() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let local = topology.id("Vehicle:split24");
    create_local(&topology, &local, &vtype, &[("speed", json!([observed(60, "2020-08-01T12:00:00Z")]))]).await;
    register_source(&topology, &mock, &vtype, &json!([])).await;

    let (body, _) = split_query(&topology, &vtype, "&options=temporalValues").await;
    pretty_assertions::assert_eq!(ids(&body), vec![local.clone()], "{body}");
    pretty_assertions::assert_eq!(pair_values(&temporal_entity(&body, &local)["speed"]), vec![json!(60)]);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Clause 4.18 reifies the Scope in the temporal representation. With no filter the entity carrying a Scope history is not re-read at all, so it stays in the answer.
#[tokio::test]
async fn distop_split_temporal_query_scope_history_without_filter_keeps_the_entity() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let local = topology.id("Vehicle:split25");
    create_local(
        &topology,
        &local,
        &vtype,
        &[
            ("speed", json!([observed(60, "2020-08-01T12:00:00Z")])),
            (
                "scope",
                json!([{ "type": "Property", "value": "/Madrid", "observedAt": "2020-08-01T12:00:00Z" }]),
            ),
        ],
    )
    .await;
    register_source(&topology, &mock, &vtype, &json!([])).await;

    let (body, _) = split_query(&topology, &vtype, "").await;
    pretty_assertions::assert_eq!(ids(&body), vec![local.clone()], "{body}");
    assert!(temporal_entity(&body, &local).get("scope").is_some(), "the reified Scope is rendered: {body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Clause 5.7.4.4: the values filter is "checked against all the Attribute instances" of the aggregated entity, so a term no part satisfies alone holds once the source's instance joins the local one.
#[tokio::test]
async fn distop_split_temporal_query_q_true_only_on_the_merge_keeps_the_entity() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let held = topology.id("Vehicle:split26");
    create_local(&topology, &held, &vtype, &[("speed", json!([observed(40, "2020-08-01T12:00:00Z")]))]).await;
    register_source(
        &topology,
        &mock,
        &vtype,
        &json!([{ "id": held, "type": vtype, "speed": answered_instances(&[(60, "2020-08-01T13:00:00Z")]) }]),
    )
    .await;

    let (body, _) = split_query(&topology, &vtype, &format!("&options=temporalValues&q={}", encode("speed>50"))).await;
    pretty_assertions::assert_eq!(ids(&body), vec![held.clone()], "{body}");
    let mut values = pair_values(&temporal_entity(&body, &held)["speed"]);
    values.sort_by_key(Value::as_i64);
    pretty_assertions::assert_eq!(values, vec![json!(40), json!(60)], "both parts' instances: {body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Clause 5.7.4.4 conjoins the values filter and the geoquery over the aggregated entity, so each may hold in a different part; an entity whose gathered location lies outside the geofence fails the geoquery whatever its speed.
#[tokio::test]
async fn distop_split_temporal_query_conjunction_across_parts_keeps_the_entity() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let inside = topology.id("Vehicle:split27-inside");
    let outside = topology.id("Vehicle:split27-outside");
    for id in [&inside, &outside] {
        create_local(&topology, id, &vtype, &[("speed", json!([observed(60, "2020-08-01T12:00:00Z")]))]).await;
    }
    register_source(
        &topology,
        &mock,
        &vtype,
        &json!([
            { "id": inside, "type": vtype, "location": located([8.0, 44.0]) },
            { "id": outside, "type": vtype, "location": located([9.0, 45.0]) }
        ]),
    )
    .await;

    let filters = format!(
        "&options=temporalValues&q={}&georel={}&geometry=Point&coordinates={}",
        encode("speed>50"),
        encode("near;maxDistance==1000"),
        encode("[8.0,44.0]")
    );
    let (body, _) = split_query(&topology, &vtype, &filters).await;
    pretty_assertions::assert_eq!(ids(&body), vec![inside], "{body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An entity whose gathered instances satisfy no term of the values filter is dropped, while one whose gathered instances do is kept.
#[tokio::test]
async fn distop_split_temporal_query_failing_merge_drops_the_entity() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let failing = topology.id("Vehicle:split28-failing");
    let passing = topology.id("Vehicle:split28-passing");
    for id in [&failing, &passing] {
        create_local(&topology, id, &vtype, &[("speed", json!([observed(40, "2020-08-01T12:00:00Z")]))]).await;
    }
    register_source(
        &topology,
        &mock,
        &vtype,
        &json!([
            { "id": failing, "type": vtype, "speed": answered_instances(&[(45, "2020-08-01T13:00:00Z")]) },
            { "id": passing, "type": vtype, "speed": answered_instances(&[(60, "2020-08-01T13:00:00Z")]) }
        ]),
    )
    .await;

    let (body, _) = split_query(&topology, &vtype, &format!("&options=temporalValues&q={}", encode("speed>50"))).await;
    pretty_assertions::assert_eq!(ids(&body), vec![passing], "{body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The source is asked for normalized instances and answers them (clause 4.5.7): a `LanguageProperty`, `JsonProperty` and `VocabProperty` instance each holds its value in the member of its family. An existence filter on the attribute only the source holds keeps the entity, whichever family the instance belongs to.
#[tokio::test]
async fn distop_split_temporal_query_normalized_answers_decide_the_filter() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let held = [
        (
            topology.id("Vehicle:split32-language"),
            "says",
            json!([{ "type": "LanguageProperty", "languageMap": { "en": "yes" }, "observedAt": "2020-08-01T13:00:00Z" }]),
        ),
        (
            topology.id("Vehicle:split32-json"),
            "settings",
            json!([{ "type": "JsonProperty", "json": { "mode": "eco" }, "observedAt": "2020-08-01T13:00:00Z" }]),
        ),
        (
            topology.id("Vehicle:split32-vocab"),
            "gender",
            json!([{ "type": "VocabProperty", "vocab": "Male", "observedAt": "2020-08-01T13:00:00Z" }]),
        ),
    ];
    let mut answer = Vec::new();
    for (id, name, attribute) in &held {
        create_local(&topology, id, &vtype, &[("speed", json!([observed(40, "2020-08-01T12:00:00Z")]))]).await;
        let mut part = json!({ "id": id, "type": vtype });
        part[*name] = attribute.clone(); // clone: the mock answers with the attribute the case names
        answer.push(part);
    }
    register_source(&topology, &mock, &vtype, &Value::Array(answer)).await;

    let (body, _) = split_query(&topology, &vtype, &format!("&options=temporalValues&q={}", encode("says|settings|gender"))).await;
    let mut kept = ids(&body);
    kept.sort_unstable();
    let mut expected: Vec<String> = held.iter().map(|(id, _, _)| id.clone()).collect(); // clone: the expectation outlives the cases
    expected.sort_unstable();
    pretty_assertions::assert_eq!(kept, expected, "{body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An entity no source answered with has no part outside this broker, so the store's own filtering decides it, and the count totals what the filters kept (clause 6.3.13).
#[tokio::test]
async fn distop_split_temporal_query_uncontributed_local_entity_is_filtered_in_sql() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let slow = topology.id("Vehicle:split29-slow");
    let fast = topology.id("Vehicle:split29-fast");
    create_local(&topology, &slow, &vtype, &[("speed", json!([observed(40, "2020-08-01T12:00:00Z")]))]).await;
    create_local(&topology, &fast, &vtype, &[("speed", json!([observed(60, "2020-08-01T12:00:00Z")]))]).await;
    register_source(&topology, &mock, &vtype, &json!([])).await;

    let (body, count) = split_query(&topology, &vtype, &format!("&count=true&q={}", encode("speed>50"))).await;
    pretty_assertions::assert_eq!(ids(&body), vec![fast], "{body}");
    pretty_assertions::assert_eq!(count, Some(1));

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A normalized `speed` instance the mock answers with, identified by its value.
fn answered_speed(value: i64, observed_at: &str) -> Value {
    json!({ "type": "Property", "value": value, "observedAt": observed_at, "instanceId": format!("urn:ngsi-ld:instance:split:{value}") })
}

/// The first value of the aggregate `method` of `attribute` holds.
fn aggregate(entity: &Value, attribute: &str, method: &str) -> Value {
    entity[attribute][method][0][0].clone()
}

/// The aggregated filters every aggregation test asks with.
fn aggregated_filters() -> String {
    format!("&format=aggregatedValues&aggrMethods={}&q={}", encode("avg,totalCount"), encode("speed>0"))
}

/// Clause 5.7.4.4: an entity held in several parts is rendered with the "aggregated values of Attribute instances" of the aggregated entity, so the source is asked for its instances -- not for an aggregate of its own -- and the aggregate is one over the local and answered instances together.
#[tokio::test]
async fn distop_split_temporal_query_aggregated_merges_instances_before_aggregating() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let held = topology.id("Vehicle:split30");
    create_local(
        &topology,
        &held,
        &vtype,
        &[("speed", json!([observed(10, "2020-08-01T12:00:00Z"), observed(20, "2020-08-01T13:00:00Z")]))],
    )
    .await;
    register_source(
        &topology,
        &mock,
        &vtype,
        &json!([{ "id": held, "type": vtype, "speed": [answered_speed(30, "2020-08-01T14:00:00Z")] }]),
    )
    .await;

    let (body, _) = split_query(&topology, &vtype, &aggregated_filters()).await;
    let entity = temporal_entity(&body, &held);
    pretty_assertions::assert_eq!(aggregate(&entity, "speed", "totalCount"), json!(3), "{body}");
    pretty_assertions::assert_eq!(aggregate(&entity, "speed", "avg"), json!(20), "{body}");
    let forwarded = mock.drain_requests().await;
    for request in &forwarded {
        let query = request.uri.query().unwrap_or_default();
        assert!(
            !query.contains("aggrMethods") && !query.contains("format"),
            "the source is asked for instances: {query}"
        );
    }

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Clause 6.3.10: a source cuts its instances at its page size with a 206 and a `Content-Range`; the aggregate counts every instance of the window, so the source's history is read past the cut, each instance once.
#[tokio::test]
async fn distop_split_temporal_query_aggregated_walks_the_source_history() {
    let topology = topology::single().await.expect("single-broker topology");
    let mock = ContextSourceAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let remote = topology.id("Vehicle:split31");
    register(&topology, &mock, &vtype).await;
    let cut = json!([{ "id": remote, "type": vtype, "speed": [answered_speed(1, "2020-08-01T12:00:00Z"), answered_speed(2, "2020-08-01T13:00:00Z")] }]);
    mock.set_stub_with_body_and_headers(
        Method::GET,
        &source_path(),
        206,
        &cut,
        &[("content-range", "DateTime 2020-08-01T12:00:00Z-2020-08-01T13:00:00Z/4")],
    )
    .await;
    let rest = json!([{ "id": remote, "type": vtype, "speed": [
        answered_speed(2, "2020-08-01T13:00:00Z"),
        answered_speed(3, "2020-08-01T14:00:00Z"),
        answered_speed(4, "2020-08-01T15:00:00Z")
    ] }]);
    mock.set_stub_with_body(Method::GET, &source_path(), 200, &rest).await;

    let (body, _) = split_query(&topology, &vtype, &aggregated_filters()).await;
    let entity = temporal_entity(&body, &remote);
    pretty_assertions::assert_eq!(aggregate(&entity, "speed", "totalCount"), json!(4), "{body}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
