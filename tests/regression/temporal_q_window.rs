//! A temporal query's `q` is checked against the Attribute instances valid
//! within the temporal window, not against current state.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.7.4.4, step S2: the query is checked against
//! the "Attribute instances (during the interval defined by the temporal
//! query)", which clause 4.11 makes the values "valid within the specified
//! timeframe": the instances observed inside it, and the one still in effect
//! when it opens (Annex C clause C.5.5 relies on the latter). The broker compiled a temporal `q` against
//! the entity's current state instead, so an entity whose in-window history held
//! a matching instance was missed whenever its latest written value did not
//! match (`TQ:1` with instances 10, 200, 50: `q=v>100` answered nothing), and one
//! whose current value matched was returned although no in-window instance did.
//! Real impact: "storms with maxSustainedWind>100 in the Gulf in 2005" answered
//! an empty set.
//!
//! Each Query Term is its own existential over the window's instances of its
//! attribute, honouring the query's `datasetId` selection and `timeproperty`,
//! and the AND/OR tree combines them.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The core context every payload here is written against.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// A window every observation of this module falls inside.
const AFTER_2019: &str = "timerel=after&timeAt=2019-01-01T00:00:00Z";

/// One Property observation of `value` at `observed_at`.
fn observation(value: &Value, observed_at: &str) -> Value {
    json!({"type": "Property", "value": value, "observedAt": observed_at})
}

/// Create one Temporal Evolution (clause 5.6.11) and assert it answered 201.
async fn create(client: &TestClient, body: &Value) {
    let response = client.create_temporal_entity(body).await.expect("create temporal entity request");
    assert_eq!(response.status, StatusCode::CREATED, "create {}: {:?}", body["id"], response.body);
}

/// The identifiers a temporal query of `type` with `query` answers.
async fn matched(client: &TestClient, entity_type: &str, query: &str) -> Vec<String> {
    let response = client
        .query_temporal_entities(&format!("type={entity_type}&{query}"), Some(CORE_CONTEXT))
        .await
        .expect("temporal query request");
    assert_eq!(response.status, StatusCode::OK, "temporal query {query}: {:?}", response.body);
    let answer = response.json_value().expect("a temporal query answer is JSON");
    let mut ids: Vec<String> = answer
        .as_array()
        .expect("a query answers a collection")
        .iter()
        .map(|entity| entity["id"].as_str().expect("every entity carries its id").to_owned())
        .collect();
    ids.sort();
    ids
}

/// The campaign's minimal repro: instances 10, 200 and 50 of `v`, 50 written
/// last.
async fn three_instances(scope: &TestEnvironment, entity_type: &str) -> String {
    let id = scope.id(&format!("{entity_type}:1"));
    create(
        scope.client(),
        &json!({
            "id": id,
            "type": entity_type,
            "v": [
                observation(&json!(10), "2020-01-01T00:00:00Z"),
                observation(&json!(200), "2020-01-02T00:00:00Z"),
                observation(&json!(50), "2020-01-03T00:00:00Z")
            ],
            "@context": CORE_CONTEXT
        }),
    )
    .await;
    id
}

/// Clause 5.7.4.4 S2: an in-window instance that is not the latest one still
/// satisfies the Query Term.
#[tokio::test]
async fn regression_temporal_q_window_an_earlier_in_window_instance_matches() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TqEarlier");
    let id = three_instances(&scope, &entity_type).await;

    for q in ["v>100", "v==200", "v==50", "v==10", "v<20", "v==10..60"] {
        assert_eq!(
            matched(scope.client(), &entity_type, &format!("q={q}&{AFTER_2019}")).await,
            vec![id.clone()],
            "q={q}"
        );
    }
    for q in ["v>300", "v==11", "v<5"] {
        assert!(matched(scope.client(), &entity_type, &format!("q={q}&{AFTER_2019}")).await.is_empty(), "q={q}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Only the instances valid within the window count: an instance replaced
/// before or at `timeAt` does not satisfy the term, whatever it held.
#[tokio::test]
async fn regression_temporal_q_window_an_instance_outside_the_window_does_not_match() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TqOutside");
    let id = three_instances(&scope, &entity_type).await;

    // 200 was replaced by 50 on 2020-01-03: a window opening then holds 50 alone.
    let late = "timerel=after&timeAt=2020-01-03T00:00:00Z";
    assert_eq!(matched(scope.client(), &entity_type, &format!("q=v>100&{late}")).await, Vec::<String>::new());
    assert_eq!(matched(scope.client(), &entity_type, &format!("q=v==50&{late}")).await, vec![id.clone()]);
    // A window closing before 2020-01-03 leaves out the latest value, 50.
    let early = "timerel=before&timeAt=2020-01-03T00:00:00Z";
    assert_eq!(matched(scope.client(), &entity_type, &format!("q=v==50&{early}")).await, Vec::<String>::new());
    assert_eq!(matched(scope.client(), &entity_type, &format!("q=v==200&{early}")).await, vec![id.clone()]);
    let between = "timerel=between&timeAt=2020-01-02T00:00:00Z&endTimeAt=2020-01-03T00:00:00Z";
    assert_eq!(matched(scope.client(), &entity_type, &format!("q=v>100&{between}")).await, vec![id]);

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.11: the instance in effect when the window opens is valid within
/// it. `v` was 200 from 2020-01-02 until 50 replaced it on 2020-01-03, so a
/// window opening on 2020-01-02 at noon saw 200 -- while only 50 is rendered.
#[tokio::test]
async fn regression_temporal_q_window_the_instance_in_effect_when_the_window_opens_matches() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TqInEffect");
    let id = three_instances(&scope, &entity_type).await;

    let noon = "timerel=after&timeAt=2020-01-02T12:00:00Z";
    assert_eq!(matched(scope.client(), &entity_type, &format!("q=v>100&{noon}")).await, vec![id.clone()]);
    assert_eq!(matched(scope.client(), &entity_type, &format!("q=v==200&{noon}")).await, vec![id.clone()]);
    assert_eq!(
        matched(scope.client(), &entity_type, &format!("q=v==10&{noon}")).await,
        Vec::<String>::new(),
        "10 was replaced before the window"
    );

    let response = scope
        .client()
        .query_temporal_entities(&format!("type={entity_type}&q=v>100&{noon}"), Some(CORE_CONTEXT))
        .await
        .expect("temporal query request");
    let answer = response.json_value().expect("a temporal query answer is JSON");
    let rendered: Vec<&Value> = answer[0]["v"]
        .as_array()
        .expect("the in-window instances")
        .iter()
        .map(|instance| &instance["value"])
        .collect();
    assert_eq!(rendered, vec![&json!(50)], "the instance in effect is evaluated, never rendered: {answer}");

    scope.finish().await.expect("cleanup failed");
}

/// Annex C clause C.5.5's shape: an Attribute observed once before the window
/// and never since still answers `q`, although the projection leaves it out.
#[tokio::test]
async fn regression_temporal_q_window_an_attribute_observed_only_before_the_window_answers_q() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TqBrand");
    let id = scope.id(&format!("{entity_type}:1"));
    create(
        scope.client(),
        &json!({
            "id": id,
            "type": entity_type,
            "brand": [observation(&json!("Volvo"), "2018-08-01T11:00:00Z")],
            "speed": [observation(&json!(120), "2018-08-01T12:03:00Z")],
            "@context": CORE_CONTEXT
        }),
    )
    .await;

    let window = "attrs=speed&timerel=between&timeAt=2018-08-01T12:00:00Z&endTimeAt=2018-08-01T13:00:00Z";
    assert_eq!(
        matched(scope.client(), &entity_type, &format!("q=brand!=\"Mercedes\"&{window}")).await,
        vec![id.clone()]
    );
    assert_eq!(
        matched(scope.client(), &entity_type, &format!("q=brand==\"Volvo\"&{window}")).await,
        vec![id.clone()]
    );
    assert_eq!(
        matched(scope.client(), &entity_type, &format!("q=brand==\"Mercedes\"&{window}")).await,
        Vec::<String>::new()
    );

    scope.finish().await.expect("cleanup failed");
}

/// Each Query Term is its own existential over the window, combined by the
/// AND/OR tree: two terms need not hold at the same instant.
#[tokio::test]
async fn regression_temporal_q_window_and_and_or_combine_per_term_existentials() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TqTree");
    let id = scope.id(&format!("{entity_type}:1"));
    create(
        scope.client(),
        &json!({
            "id": id,
            "type": entity_type,
            "a": [observation(&json!(1), "2020-01-01T00:00:00Z"), observation(&json!(9), "2020-01-02T00:00:00Z")],
            "b": [observation(&json!("red"), "2020-01-01T00:00:00Z"), observation(&json!("blue"), "2020-01-02T00:00:00Z")],
            "@context": CORE_CONTEXT
        }),
    )
    .await;

    for q in ["a==1;b==\"blue\"", "a==9;b==\"red\"", "a==7|b==\"red\"", "(a>5;b==\"red\")|a==100"] {
        assert_eq!(
            matched(scope.client(), &entity_type, &format!("q={q}&{AFTER_2019}")).await,
            vec![id.clone()],
            "q={q}"
        );
    }
    for q in ["a==1;b==\"green\"", "a==7|b==\"green\""] {
        assert!(matched(scope.client(), &entity_type, &format!("q={q}&{AFTER_2019}")).await.is_empty(), "q={q}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.5: the instances a term is checked against are those of the
/// `datasetId`s the query selected.
#[tokio::test]
async fn regression_temporal_q_window_the_dataset_selection_narrows_the_instances() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TqDataset");
    let id = scope.id(&format!("{entity_type}:1"));
    create(
        scope.client(),
        &json!({
            "id": id,
            "type": entity_type,
            "v": [
                observation(&json!(10), "2020-01-01T00:00:00Z"),
                {"type": "Property", "value": 200, "observedAt": "2020-01-02T00:00:00Z", "datasetId": "urn:ngsi-ld:Dataset:sensor"}
            ],
            "@context": CORE_CONTEXT
        }),
    )
    .await;

    assert_eq!(matched(scope.client(), &entity_type, &format!("q=v>100&{AFTER_2019}")).await, vec![id.clone()]);
    assert_eq!(
        matched(scope.client(), &entity_type, &format!("q=v>100&datasetId=@none&{AFTER_2019}")).await,
        Vec::<String>::new()
    );
    assert_eq!(
        matched(
            scope.client(),
            &entity_type,
            &format!("q=v>100&datasetId=urn:ngsi-ld:Dataset:sensor&{AFTER_2019}")
        )
        .await,
        vec![id]
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.11: the window is on the requested Temporal Property. Over
/// `modifiedAt` the instances are the entity's Core writes, so a value written
/// before the current one still satisfies the term, which a current-state
/// reading of `q` never allowed.
#[tokio::test]
async fn regression_temporal_q_window_a_modified_at_window_reads_every_recorded_write() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TqModified");
    let id = scope.id(&format!("{entity_type}:1"));
    for value in [200, 5] {
        let response = scope
            .client()
            .batch_upsert(&json!([{"id": id, "type": entity_type, "v": {"type": "Property", "value": value}, "@context": CORE_CONTEXT}]))
            .await
            .expect("batch upsert request");
        assert!(response.status.is_success(), "{:?}", response.body);
    }

    let window = "timeproperty=modifiedAt&timerel=after&timeAt=2000-01-01T00:00:00Z";
    assert_eq!(matched(scope.client(), &entity_type, &format!("q=v>100&{window}")).await, vec![id.clone()]);
    assert_eq!(matched(scope.client(), &entity_type, &format!("q=v==5&{window}")).await, vec![id]);
    // The default `observedAt` axis holds nothing for writes that carried none.
    assert_eq!(
        matched(scope.client(), &entity_type, &format!("q=v>100&{AFTER_2019}")).await,
        Vec::<String>::new()
    );

    scope.finish().await.expect("cleanup failed");
}

/// A term on a sub-attribute reaches the sub-attributes the in-window instance
/// itself carried.
#[tokio::test]
async fn regression_temporal_q_window_a_sub_attribute_term_reads_the_instance_that_carried_it() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TqSubAttribute");
    let id = scope.id(&format!("{entity_type}:1"));
    create(
        scope.client(),
        &json!({
            "id": id,
            "type": entity_type,
            "v": [
                {"type": "Property", "value": 1, "observedAt": "2020-01-01T00:00:00Z", "quality": {"type": "Property", "value": "high"}},
                {"type": "Property", "value": 2, "observedAt": "2020-01-02T00:00:00Z", "quality": {"type": "Property", "value": "low"}}
            ],
            "@context": CORE_CONTEXT
        }),
    )
    .await;

    assert_eq!(
        matched(scope.client(), &entity_type, &format!("q=v.quality==\"high\"&{AFTER_2019}")).await,
        vec![id.clone()]
    );
    let late = "timerel=after&timeAt=2020-01-02T00:00:00Z";
    assert_eq!(
        matched(scope.client(), &entity_type, &format!("q=v.quality==\"high\"&{late}")).await,
        Vec::<String>::new()
    );
    assert_eq!(matched(scope.client(), &entity_type, &format!("q=v.quality==\"low\"&{late}")).await, vec![id]);

    scope.finish().await.expect("cleanup failed");
}

/// A Relationship's in-window objects are the targets a term compares.
#[tokio::test]
async fn regression_temporal_q_window_a_relationship_term_reads_the_in_window_objects() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TqRelationship");
    let id = scope.id(&format!("{entity_type}:1"));
    create(
        scope.client(),
        &json!({
            "id": id,
            "type": entity_type,
            "stopsAt": [
                {"type": "Relationship", "object": "urn:ngsi-ld:Stop:A", "observedAt": "2020-01-01T00:00:00Z"},
                {"type": "Relationship", "object": "urn:ngsi-ld:Stop:B", "observedAt": "2020-01-02T00:00:00Z"}
            ],
            "@context": CORE_CONTEXT
        }),
    )
    .await;

    assert_eq!(
        matched(scope.client(), &entity_type, &format!("q=stopsAt==\"urn:ngsi-ld:Stop:A\"&{AFTER_2019}")).await,
        vec![id]
    );
    let late = "timerel=after&timeAt=2020-01-02T00:00:00Z";
    assert_eq!(
        matched(scope.client(), &entity_type, &format!("q=stopsAt==\"urn:ngsi-ld:Stop:A\"&{late}")).await,
        Vec::<String>::new()
    );

    scope.finish().await.expect("cleanup failed");
}

/// The E8 shape from the campaign: a temporal `q` and a temporal geoquery
/// together select a storm whose strong-wind instance and in-area instance both
/// lie inside the window, though its last observation is weak and elsewhere.
#[tokio::test]
async fn regression_temporal_q_window_q_and_a_geoquery_combine_over_the_window() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TqStorm");
    let id = scope.id(&format!("{entity_type}:1"));
    let point = |lon: f64, lat: f64| json!({"type": "Point", "coordinates": [lon, lat]});
    create(
        scope.client(),
        &json!({
            "id": id,
            "type": entity_type,
            "maxSustainedWind": [
                observation(&json!(150), "2005-08-28T00:00:00Z"),
                observation(&json!(30), "2005-08-31T00:00:00Z")
            ],
            "location": [
                {"type": "GeoProperty", "value": point(-89.0, 26.0), "observedAt": "2005-08-28T00:00:00Z"},
                {"type": "GeoProperty", "value": point(-80.0, 40.0), "observedAt": "2005-08-31T00:00:00Z"}
            ],
            "@context": CORE_CONTEXT
        }),
    )
    .await;

    let gulf = "georel=within&geometry=Polygon&coordinates=[[[-98,18],[-80,18],[-80,31],[-98,31],[-98,18]]]";
    let year = "timerel=between&timeAt=2005-01-01T00:00:00Z&endTimeAt=2006-01-01T00:00:00Z";
    assert_eq!(
        matched(scope.client(), &entity_type, &format!("q=maxSustainedWind>100&{gulf}&{year}")).await,
        vec![id]
    );
    let next_year = "timerel=between&timeAt=2006-01-01T00:00:00Z&endTimeAt=2007-01-01T00:00:00Z";
    assert_eq!(
        matched(scope.client(), &entity_type, &format!("q=maxSustainedWind>100&{gulf}&{next_year}")).await,
        Vec::<String>::new()
    );

    scope.finish().await.expect("cleanup failed");
}
