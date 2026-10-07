//! A `datasetId` that matches none of an Entity's instances removes its
//! Attributes from a temporal query, never the Entity.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.7.4.4: only step S1 removes an Entity, when
//! "the time series, for all the concerned Attributes of an Entity, does not
//! include data corresponding to the temporal query interval". The `datasetId`
//! parameter is step S8, applied "from S7, for all Entities": it keeps "only the
//! Attribute instances whose datasetId is specified" and removes "all Attributes
//! without remaining Attribute instances". Pagination (clause 5.5.9) follows S8,
//! so the Entity is counted and paged, and the values filter (S2) and the
//! geoquery (S3) that precede S8 read the instances of every dataset.
//!
//! Narrowing the entity selection by dataset would make such an Entity vanish
//! from the answer and from `NGSILD-Results-Count`. The retrieval counterpart is
//! clause 5.7.3.4.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The core context every payload here is written against.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// A window every observation of this module falls inside.
const BEFORE_2030: &str = "timerel=before&timeAt=2030-01-01T00:00:00Z";

/// The members of a temporal entity that are not Attributes.
const ENTITY_MEMBERS: [&str; 3] = ["id", "type", "@context"];

/// Create one Temporal Evolution (clause 5.6.11) and assert it answered 201.
async fn create(client: &TestClient, body: &Value) {
    let response = client.create_temporal_entity(body).await.expect("create temporal entity request");
    assert_eq!(response.status, StatusCode::CREATED, "create {}: {:?}", body["id"], response.body);
}

/// An entity with a default-instance `speed` and a `fuel` of `dataset`, both
/// observed inside [`BEFORE_2030`].
fn speed_and_fuel(id: &str, entity_type: &str, dataset: &str) -> Value {
    json!({
        "id": id,
        "type": entity_type,
        "speed": [{"type": "Property", "value": 10, "observedAt": "2020-08-01T12:00:00Z"}],
        "fuel": [{"type": "Property", "value": 40, "observedAt": "2020-08-01T12:00:00Z", "datasetId": dataset}],
        "@context": CORE_CONTEXT
    })
}

/// Run a temporal query of `entity_type` and assert it answered 200.
async fn query(client: &TestClient, entity_type: &str, query: &str) -> TestResponse {
    let response = client
        .query_temporal_entities(&format!("type={entity_type}&{query}"), Some(CORE_CONTEXT))
        .await
        .expect("temporal query request");
    assert_eq!(response.status, StatusCode::OK, "temporal query {query}: {:?}", response.body);
    response
}

/// The entities a temporal query answered, in the order it answered them.
fn entities(response: &TestResponse) -> Vec<Value> {
    response.json::<Vec<Value>>().expect("a temporal query answers a JSON collection")
}

/// The Attribute names a temporal entity carries, sorted.
fn attributes(entity: &Value) -> Vec<&str> {
    let mut names: Vec<&str> = entity
        .as_object()
        .expect("a temporal entity is an object")
        .keys()
        .map(String::as_str)
        .filter(|name| !ENTITY_MEMBERS.contains(name))
        .collect();
    names.sort_unstable();
    names
}

/// The `value`s of every instance an Attribute of a normalized temporal entity
/// carries, whether it renders as one instance or as an array of them.
fn instance_values(attribute: &Value) -> Vec<&Value> {
    match attribute {
        Value::Array(instances) => instances.iter().map(|instance| &instance["value"]).collect(),
        instance @ (Value::Object(_) | Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => vec![&instance["value"]],
    }
}

/// Clause 5.7.4.4 S8: a dataset no instance carries leaves the Entity, with
/// its `id` and `type` and no Attribute; a dataset one Attribute carries keeps
/// that Attribute alone.
#[tokio::test]
async fn regression_temporal_query_dataset_filter_a_dataset_no_instance_carries_keeps_the_entity() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TqdsKeep");
    let id = scope.id(&format!("{entity_type}:1"));
    let named = scope.id("dataset:named");
    create(scope.client(), &speed_and_fuel(&id, &entity_type, &named)).await;

    let absent = urlencoding::encode(&scope.id("dataset:absent")).into_owned();
    let answer = entities(&query(scope.client(), &entity_type, &format!("{BEFORE_2030}&datasetId={absent}")).await);
    assert_eq!(answer.len(), 1, "{answer:?}");
    assert_eq!(answer[0]["id"], json!(id));
    assert_eq!(answer[0]["type"], json!(entity_type));
    assert_eq!(attributes(&answer[0]), Vec::<&str>::new(), "every Attribute is removed: {}", answer[0]);

    let encoded = urlencoding::encode(&named).into_owned();
    let narrowed = entities(&query(scope.client(), &entity_type, &format!("{BEFORE_2030}&datasetId={encoded}")).await);
    assert_eq!(narrowed.len(), 1, "{narrowed:?}");
    assert_eq!(attributes(&narrowed[0]), vec!["fuel"], "{}", narrowed[0]);

    let default = entities(&query(scope.client(), &entity_type, &format!("{BEFORE_2030}&datasetId=@none")).await);
    assert_eq!(default.len(), 1, "{default:?}");
    assert_eq!(attributes(&default[0]), vec!["speed"], "{}", default[0]);

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.9 follows S8, so an Entity S8 emptied is counted by
/// `NGSILD-Results-Count` (clause 6.3.13) and served on its page like any
/// other.
#[tokio::test]
async fn regression_temporal_query_dataset_filter_an_emptied_entity_is_counted_and_paged() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TqdsPaged");
    let named = scope.id("dataset:named");
    let carrier = scope.id(&format!("{entity_type}:a"));
    let emptied = scope.id(&format!("{entity_type}:b"));
    create(scope.client(), &speed_and_fuel(&carrier, &entity_type, &named)).await;
    create(
        scope.client(),
        &json!({
            "id": emptied,
            "type": entity_type,
            "speed": [{"type": "Property", "value": 20, "observedAt": "2020-08-01T12:00:00Z"}],
            "@context": CORE_CONTEXT
        }),
    )
    .await;

    let dataset = urlencoding::encode(&named).into_owned();
    let mut seen = Vec::new();
    for offset in 0..2 {
        let page = query(
            scope.client(),
            &entity_type,
            &format!("{BEFORE_2030}&datasetId={dataset}&count=true&limit=1&offset={offset}"),
        )
        .await;
        assert_eq!(
            page.headers.get("NGSILD-Results-Count").and_then(|value| value.to_str().ok()),
            Some("2"),
            "both entities pass S1 and are counted: {:?}",
            page.headers
        );
        let answer = entities(&page);
        assert_eq!(answer.len(), 1, "offset {offset}: {answer:?}");
        seen.extend(answer);
    }
    seen.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    let seen = seen;
    assert_eq!(seen[0]["id"], json!(carrier));
    assert_eq!(attributes(&seen[0]), vec!["fuel"], "{}", seen[0]);
    assert_eq!(seen[1]["id"], json!(emptied));
    assert_eq!(attributes(&seen[1]), Vec::<&str>::new(), "{}", seen[1]);

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.4.3 makes `attrs` a list "of which at least one shall exist in
/// order for an Entity to be selected, and also used as query projection
/// attributes": `speed` exists in the window, so the Entity is selected, and
/// S8 then removes it for carrying no instance of the named dataset.
#[tokio::test]
async fn regression_temporal_query_dataset_filter_attrs_of_another_dataset_keep_the_entity() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TqdsAttrs");
    let id = scope.id(&format!("{entity_type}:1"));
    let named = scope.id("dataset:named");
    create(scope.client(), &speed_and_fuel(&id, &entity_type, &named)).await;

    let dataset = urlencoding::encode(&named).into_owned();
    for format in ["", "&options=temporalValues"] {
        let answer = entities(&query(scope.client(), &entity_type, &format!("{BEFORE_2030}&attrs=speed&datasetId={dataset}{format}")).await);
        assert_eq!(answer.len(), 1, "{format}: {answer:?}");
        assert_eq!(answer[0]["id"], json!(id), "{format}");
        assert_eq!(answer[0]["type"], json!(entity_type), "{format}");
        assert_eq!(attributes(&answer[0]), Vec::<&str>::new(), "{format}: {}", answer[0]);
    }

    // `attrs` still selects: an Attribute the Entity never had leaves nothing
    // to select it by.
    let missing = entities(&query(scope.client(), &entity_type, &format!("{BEFORE_2030}&attrs=never&datasetId={dataset}")).await);
    assert_eq!(missing, Vec::<Value>::new());

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.19 aggregates the instances S8 left, so an Entity S8 emptied
/// answers with its `id` and `type` and no aggregate, rather than vanishing or
/// failing as ineligible.
#[tokio::test]
async fn regression_temporal_query_dataset_filter_an_aggregated_query_keeps_the_entity() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TqdsAggr");
    let id = scope.id(&format!("{entity_type}:1"));
    let named = scope.id("dataset:named");
    create(scope.client(), &speed_and_fuel(&id, &entity_type, &named)).await;

    let aggregated = "format=aggregatedValues&aggrMethods=max";
    let whole = entities(&query(scope.client(), &entity_type, &format!("{BEFORE_2030}&{aggregated}")).await);
    assert_eq!(whole.len(), 1, "{whole:?}");
    assert_eq!(attributes(&whole[0]), vec!["fuel", "speed"], "{}", whole[0]);

    let absent = urlencoding::encode(&scope.id("dataset:absent")).into_owned();
    let emptied = entities(&query(scope.client(), &entity_type, &format!("{BEFORE_2030}&{aggregated}&datasetId={absent}")).await);
    assert_eq!(emptied.len(), 1, "{emptied:?}");
    assert_eq!(emptied[0]["id"], json!(id));
    assert_eq!(attributes(&emptied[0]), Vec::<&str>::new(), "{}", emptied[0]);

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.4.4 S3 checks the geoquery "against the `GeoProperty` instances
/// that are within the interval defined by the temporal query", before S8: a
/// `location` of a named dataset selects the Entity under `datasetId=@none`,
/// and S8 then removes that `location`.
#[tokio::test]
async fn regression_temporal_query_dataset_filter_a_geoquery_reads_every_dataset() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TqdsGeo");
    let id = scope.id(&format!("{entity_type}:1"));
    create(
        scope.client(),
        &json!({
            "id": id,
            "type": entity_type,
            "speed": [{"type": "Property", "value": 10, "observedAt": "2020-08-01T12:00:00Z"}],
            "location": [{
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [-8.5, 41.2]},
                "observedAt": "2020-08-01T12:00:00Z",
                "datasetId": "urn:ngsi-ld:Dataset:gps"
            }],
            "@context": CORE_CONTEXT
        }),
    )
    .await;

    let near = "georel=near;maxDistance==2000&geometry=Point&coordinates=[-8.503,41.202]";
    let answer = entities(&query(scope.client(), &entity_type, &format!("{BEFORE_2030}&{near}&datasetId=@none")).await);
    assert_eq!(answer.len(), 1, "{answer:?}");
    assert_eq!(answer[0]["id"], json!(id));
    assert_eq!(attributes(&answer[0]), vec!["speed"], "{}", answer[0]);
    assert_eq!(instance_values(&answer[0]["speed"]), vec![&json!(10)], "{}", answer[0]);

    // The geoquery still selects: a point far from every instance matches
    // nothing, whatever the dataset.
    let far = "georel=near;maxDistance==2000&geometry=Point&coordinates=[10.0,50.0]";
    let none = entities(&query(scope.client(), &entity_type, &format!("{BEFORE_2030}&{far}&datasetId=@none")).await);
    assert_eq!(none, Vec::<Value>::new());

    scope.finish().await.expect("cleanup failed");
}

/// Step S1 is untouched: a window holding no instance removes the Entity,
/// with or without a `datasetId`.
#[tokio::test]
async fn regression_temporal_query_dataset_filter_a_window_without_data_still_removes_the_entity() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("TqdsWindow");
    let id = scope.id(&format!("{entity_type}:1"));
    create(scope.client(), &speed_and_fuel(&id, &entity_type, &scope.id("dataset:named"))).await;

    let early = "timerel=before&timeAt=2000-01-01T00:00:00Z";
    for dataset in ["", "&datasetId=@none"] {
        let answer = entities(&query(scope.client(), &entity_type, &format!("{early}{dataset}&count=true")).await);
        assert_eq!(answer, Vec::<Value>::new(), "{dataset}");
    }

    scope.finish().await.expect("cleanup failed");
}
