//! A Temporal Evolution written in the simplified temporal representation.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.2.20: "Alternatively it is possible to
//! specify the `EntityTemporal` by using the 'Simplified temporal representation
//! of an Entity', as defined in clause 4.5.9", which clauses 5.6.11.3 and
//! 5.6.12.3 take as the input of Create or Update and of Add Attributes. In it,
//! each attribute holds its family's plural member of `[value, instant]` pairs
//! and nothing else. Every pair was dropped, so each became an instance with no
//! value and the write was refused, while a shape clause 4.5.9 does not define
//! -- instance objects under the plural member, inheriting members written
//! beside it -- was accepted.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The window every temporal read of this module asks for.
const WINDOW: &str = "timerel=after&timeAt=2000-01-01T00:00:00Z";

/// The problem type of a request whose content is refused (clause 6.3.2).
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// The first instant every attribute is observed at.
const FIRST: &str = "2022-08-09T18:25:02Z";

/// The second instant every attribute is observed at.
const SECOND: &str = "2022-08-10T18:25:02Z";

/// One attribute of each family in the simplified temporal representation,
/// observed twice, after the examples of clause 4.5.9.
fn simplified_attributes() -> Value {
    json!({
        "name": {"type": "Property", "values": [["Joe Bloggs", FIRST], ["Bill Smith", SECOND]]},
        "location": {"type": "GeoProperty", "values": [
            [{"type": "Point", "coordinates": [13.5, 52.5]}, FIRST],
            [{"type": "Point", "coordinates": [13.25, 52.75]}, SECOND]
        ]},
        "says": {"type": "LanguageProperty", "languageMaps": [
            [{"languageMap": {"en": "yes", "fr": "oui"}}, FIRST],
            [{"languageMap": {"en": "no", "fr": "non"}}, SECOND]
        ]},
        "period": {"type": "ListProperty", "valueLists": [[["First", "Second"], FIRST], [["1st", "2nd"], SECOND]]},
        "parkingTickets": {"type": "JsonProperty", "jsons": [
            [{"json": [{"id": "85a6cc52-0589-45f9", "value": "Overstay 60 minutes"}, {"id": "x5c56s-0589-45f9", "value": "Overstay 45 minutes"}]}, FIRST],
            [{"json": [{"id": "x5c56s-0589-45f9", "value": "Overstay 45 minutes"}, {"id": "y7d21t-0589-45f9", "value": "Overstay 30 minutes"}]}, SECOND]
        ]},
        "gender": {"type": "VocabProperty", "vocabs": [[{"vocab": "Male"}, FIRST], [{"vocab": "Female"}, SECOND]]},
        "spouse": {"type": "Relationship", "objects": [["urn:ngsi-ld:Person:123455", FIRST], ["urn:ngsi-ld:Person:999999", SECOND]]},
        "membersPresent": {"type": "ListRelationship", "objectLists": [
            [["urn:ngsi-ld:Person:Alice", "urn:ngsi-ld:Person:Bob"], FIRST],
            [["urn:ngsi-ld:Person:Alice", "urn:ngsi-ld:Person:Eve"], SECOND]
        ]}
    })
}

/// The Temporal Evolution of `id` in the representation `query` selects.
async fn temporal(client: &TestClient, id: &str, query: &str) -> Value {
    let response = client
        .get_temporal_entity(id, query, Some(CORE_CONTEXT))
        .await
        .expect("retrieve temporal entity request");
    assert_eq!(response.status, StatusCode::OK, "temporal retrieval of {id}: {:?}", response.body);
    response.json_value().expect("a temporal answer is JSON")
}

/// Whether a Temporal Evolution of `id` exists.
async fn temporal_status(client: &TestClient, id: &str) -> StatusCode {
    client
        .raw(Method::GET, &format!("/ngsi-ld/v1/temporal/entities/{}", urlencoding::encode(id)))
        .send()
        .await
        .expect("retrieve temporal entity request")
        .status
}

/// Clauses 5.6.11.3 and 5.2.20: a Temporal Evolution written in the
/// simplified temporal representation is recorded pair by pair, and reads
/// back in that representation as it was written, for every family.
#[tokio::test]
async fn regression_r11_a_simplified_temporal_evolution_is_recorded_and_reads_back_as_written() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("SimplifiedProbe:create");
    let mut evolution = simplified_attributes();
    evolution["id"] = json!(id);
    evolution["type"] = json!("SimplifiedProbe");
    evolution["@context"] = json!(CORE_CONTEXT);

    let created = client.create_temporal_entity(&evolution).await.expect("create temporal entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let normalized = temporal(client, &id, WINDOW).await;
    assert_eq!(normalized["name"][1]["value"], json!("Bill Smith"), "{normalized}");
    assert_eq!(normalized["name"][1]["observedAt"], json!(SECOND), "{normalized}");
    assert_eq!(normalized["says"][0]["languageMap"], json!({"en": "yes", "fr": "oui"}), "{normalized}");
    let simplified = temporal(client, &id, &format!("{WINDOW}&format=temporalValues")).await;
    let written = simplified_attributes();
    for (name, attribute) in written.as_object().expect("an object of attributes") {
        assert_eq!(&simplified[name], attribute, "{name} in {simplified}");
    }

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.6.12.3: Add Attributes takes a Fragment of `EntityTemporal`, so the
/// pairs of an object naming a `datasetId` are added as instances of that
/// dataset.
#[tokio::test]
async fn regression_r11_add_attributes_records_simplified_pairs_in_their_dataset() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("SimplifiedProbe:add");
    let evolution = json!({
        "id": id,
        "type": "SimplifiedProbe",
        "speed": [{"type": "Property", "value": 1, "observedAt": "2024-01-01T00:00:00Z"}],
        "@context": CORE_CONTEXT
    });
    let created = client.create_temporal_entity(&evolution).await.expect("create temporal entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let added = client
        .raw(Method::POST, &format!("/ngsi-ld/v1/temporal/entities/{}/attrs", urlencoding::encode(&id)))
        .header("content-type", "application/ld+json")
        .json(&json!({
            "speed": {
                "type": "Property",
                "datasetId": "urn:ngsi-ld:Dataset:radar",
                "values": [[2, "2024-01-02T00:00:00Z"], [3, "2024-01-03T00:00:00Z"]]
            },
            "@context": CORE_CONTEXT
        }))
        .send()
        .await
        .expect("temporal add attributes request");
    assert_eq!(added.status, StatusCode::NO_CONTENT, "{:?}", added.body);

    let normalized = temporal(client, &id, WINDOW).await;
    let mut radar: Vec<(Value, Value)> = normalized["speed"]
        .as_array()
        .expect("the speed history")
        .iter()
        .filter(|instance| instance["datasetId"] == json!("urn:ngsi-ld:Dataset:radar"))
        .map(|instance| (instance["value"].clone(), instance["observedAt"].clone())) // clone: the pairs outlive the answer they are read from
        .collect();
    radar.sort_by_key(|(value, _)| value.as_i64());
    assert_eq!(
        radar,
        vec![(json!(2), json!("2024-01-02T00:00:00Z")), (json!(3), json!("2024-01-03T00:00:00Z"))],
        "{normalized}"
    );

    scope.finish().await.expect("environment cleanup");
}

/// Clause 4.5.9 makes every element a pair of exactly two elements, wraps a
/// `LanguageProperty` value in a one-member object, and has the object "only
/// contain" its plural member: each departure is refused with 400 and no
/// Temporal Evolution is created. The instance objects under `values` the
/// broker once accepted are among them.
#[tokio::test]
async fn regression_r11_a_malformed_simplified_attribute_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    for (local, attribute) in [
        ("member-beside", json!({"type": "Property", "unitCode": "KMH", "values": [[55, FIRST]]})),
        ("instance-element", json!({"type": "Property", "values": [{"value": 55, "observedAt": FIRST}]})),
        ("three-elements", json!({"type": "Property", "values": [[55, FIRST, SECOND]]})),
        ("instant-not-a-string", json!({"type": "Property", "values": [[55, 1_660_069_502]]})),
        (
            "unwrapped-language-map",
            json!({"type": "LanguageProperty", "languageMaps": [[{"en": "yes"}, FIRST]]}),
        ),
    ] {
        let id = scope.id(&format!("SimplifiedProbe:{local}"));
        let evolution = json!({"id": id, "type": "SimplifiedProbe", "speed": attribute, "@context": CORE_CONTEXT});

        let refused = client.create_temporal_entity(&evolution).await.expect("create temporal entity request");

        let body = refused.json_value().expect("a problem detail is JSON");
        assert_eq!(refused.status, StatusCode::BAD_REQUEST, "{local}: {body}");
        assert_eq!(body["type"], BAD_REQUEST_DATA, "{local}: {body}");
        assert_eq!(temporal_status(client, &id).await, StatusCode::NOT_FOUND, "{local}");
    }

    scope.finish().await.expect("environment cleanup");
}
