//! `unitCode` on each of the eight attribute families.
//!
//! Clause 4.5.2.2 lists `unitCode` among a Property's Optional members. Clause
//! 4.5.21.2 makes a normalized `ListProperty` "the same as the JSON-LD object in
//! NGSI-LD Property Representation defined in clause 4.5.2.2, with the following
//! differences", and those differences replace `value` with `valueList` without
//! removing the unit; clause 4.7.3 says a `GeoProperty` carrying further members
//! "shall be treated in the same manner as an ordinary NGSI-LD Property". Those
//! three families therefore admit a unit. Clauses 4.5.3.2, 4.5.18.2, 4.5.20.2,
//! 4.5.22.2 and 4.5.24.2 each name `unitCode` under Prohibited, so the other
//! five refuse one.
//!
//! Annex C clause C.2.2 prints a normalized Vehicle that carries `unitCode` on a
//! `ListProperty`: the member is reserved on the two specialized families, so
//! normalization does not reify it into `{"type": "Property", "value": "MMT"}`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{header::jsonld_link_header, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// A registered UN/CEFACT common code (millimetre).
const MILLIMETRE: &str = "MMT";

/// One Entity carrying `attribute` under the suite `@context`.
fn entity_with(identifier: &str, name: &str, attribute: &Value) -> Value {
    json!({
        "@context": [TEST_SUITE_CONTEXT],
        "id": identifier,
        "type": "Building",
        name: attribute
    })
}

/// The eight families, each with a `unitCode` added to an otherwise valid
/// instance, paired with the member name the attribute is written under.
fn instances_with_a_unit() -> Vec<(&'static str, Value)> {
    vec![
        ("Property", json!({"type": "Property", "value": 42, "unitCode": MILLIMETRE})),
        (
            "GeoProperty",
            json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.0, 52.0]}, "unitCode": MILLIMETRE}),
        ),
        (
            "ListProperty",
            json!({"type": "ListProperty", "valueList": [300, 300, 120, 6], "unitCode": MILLIMETRE}),
        ),
        (
            "Relationship",
            json!({"type": "Relationship", "object": "urn:ngsi-ld:Room:1", "unitCode": MILLIMETRE}),
        ),
        (
            "LanguageProperty",
            json!({"type": "LanguageProperty", "languageMap": {"en": "Great Market"}, "unitCode": MILLIMETRE}),
        ),
        ("VocabProperty", json!({"type": "VocabProperty", "vocab": "commercial", "unitCode": MILLIMETRE})),
        ("JsonProperty", json!({"type": "JsonProperty", "json": {"a": 1}, "unitCode": MILLIMETRE})),
        (
            "ListRelationship",
            json!({"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:1"], "unitCode": MILLIMETRE}),
        ),
    ]
}

/// The families clause 4.5.2.2, clause 4.5.21.2 and clause 4.7.3 give a unit.
const ADMITTING_FAMILIES: [&str; 3] = ["Property", "GeoProperty", "ListProperty"];

/// Retrieve one Entity under the suite `@context`.
async fn retrieve(client: &TestClient, identifier: &str, query: &str) -> Value {
    let retrieved = client
        .get_entity(identifier, query, "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("entity retrieval failed");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieval of {identifier}: {:?}", retrieved.body);
    retrieved.json_value().expect("an Entity body")
}

/// The three quantity-bearing families accept a unit and store it as the
/// envelope member clause 4.5.2.2 defines, not as a sub-attribute.
#[tokio::test]
async fn regression_unit_code_family_admission_stores_a_unit_on_every_quantity_bearing_family() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (family, instance) in instances_with_a_unit() {
        if !ADMITTING_FAMILIES.contains(&family) {
            continue;
        }
        let identifier = scope.id(&format!("Building:unit-{family}"));
        let created = client
            .post_entity(&entity_with(&identifier, "measure", &instance))
            .send()
            .await
            .expect("create entity failed");
        assert_eq!(created.status, StatusCode::CREATED, "{family} admits a unitCode: {:?}", created.body);

        let entity = retrieve(client, &identifier, "").await;
        assert_eq!(entity["measure"]["unitCode"], json!(MILLIMETRE), "{family}: {entity}");
        assert_eq!(entity["measure"]["type"], json!(family), "{family}: {entity}");
    }

    scope.finish().await.expect("environment cleanup");
}

/// The five unitless families refuse a unit as `BadRequestData`, naming the
/// member rather than reporting a storage fault.
#[tokio::test]
async fn regression_unit_code_family_admission_refuses_a_unit_on_every_unitless_family() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (family, instance) in instances_with_a_unit() {
        if ADMITTING_FAMILIES.contains(&family) {
            continue;
        }
        let identifier = scope.id(&format!("Building:unitless-{family}"));
        let refused = client
            .post_entity(&entity_with(&identifier, "measure", &instance))
            .send()
            .await
            .expect("create entity failed");
        assert_eq!(refused.status, StatusCode::BAD_REQUEST, "{family} is unitless: {:?}", refused.body);
        let problem = refused.json_value().expect("a problem details body");
        assert_eq!(
            problem["type"],
            json!("https://uri.etsi.org/ngsi-ld/errors/BadRequestData"),
            "{family}: {problem}"
        );
        assert!(
            problem["detail"].as_str().is_some_and(|detail| detail.contains("unitCode")),
            "{family}: the refusal has to name the member, got {problem}"
        );
    }

    scope.finish().await.expect("environment cleanup");
}

/// The unit stays an envelope member across the representations: it renders as a
/// bare string in the normalized and concise forms rather than as the reified
/// Property a sub-attribute would become.
#[tokio::test]
async fn regression_unit_code_family_admission_renders_the_unit_as_an_envelope_member() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let identifier = scope.id("Building:unit-representations");
    let created = client
        .post_entity(&entity_with(
            &identifier,
            "depths",
            &json!({"type": "ListProperty", "valueList": [300, 120], "unitCode": MILLIMETRE}),
        ))
        .send()
        .await
        .expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let normalized = retrieve(client, &identifier, "").await;
    assert_eq!(normalized["depths"]["unitCode"], json!(MILLIMETRE), "{normalized}");
    assert_eq!(normalized["depths"]["valueList"], json!([300, 120]), "{normalized}");

    let concise = retrieve(client, &identifier, "options=concise").await;
    assert_eq!(concise["depths"]["unitCode"], json!(MILLIMETRE), "{concise}");
    assert!(concise["depths"].get("type").is_none(), "the concise form drops `type`: {concise}");

    scope.finish().await.expect("environment cleanup");
}

/// The unit reaches the temporal history of a `ListProperty` and comes back from
/// it unchanged: clause 4.5.9 keeps every instance's own members, and the unit
/// travels in a dedicated column that the write and read sides have to agree on.
#[tokio::test]
async fn regression_unit_code_family_admission_round_trips_a_list_property_unit_through_history() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let identifier = scope.id("Building:unit-history");
    let created = client
        .post_entity(&entity_with(
            &identifier,
            "depths",
            &json!({"type": "ListProperty", "valueList": [300, 120], "unitCode": MILLIMETRE, "observedAt": "2024-01-01T00:00:00Z"}),
        ))
        .send()
        .await
        .expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let retrieved = client
        .raw(Method::GET, &format!("temporal/entities/{identifier}"))
        .header("accept", "application/json")
        .header("link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("valid JSON-LD Link header"))
        .send()
        .await
        .expect("temporal retrieval failed");
    assert_eq!(retrieved.status, StatusCode::OK, "temporal retrieval: {:?}", retrieved.body);
    let evolution = retrieved.json_value().expect("a temporal Entity body");
    let instances = evolution["depths"].as_array().expect("an array of instances");
    assert_eq!(instances.len(), 1, "{evolution}");
    assert_eq!(instances[0]["unitCode"], json!(MILLIMETRE), "{evolution}");
    assert_eq!(instances[0]["valueList"], json!([300, 120]), "{evolution}");

    scope.finish().await.expect("environment cleanup");
}

/// Clause 4.5.2.2 draws the unit from the UN/CEFACT common codes, so one outside
/// that list is refused on a `ListProperty` exactly as on a Property -- the
/// admission concerns which families may carry a unit, not which codes are
/// units.
#[tokio::test]
async fn regression_unit_code_family_admission_still_refuses_a_string_that_is_no_common_code() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (family, instance) in [
        ("Property", json!({"type": "Property", "value": 42, "unitCode": "NOTACODE"})),
        ("ListProperty", json!({"type": "ListProperty", "valueList": [1], "unitCode": "NOTACODE"})),
        (
            "GeoProperty",
            json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.0, 52.0]}, "unitCode": "NOTACODE"}),
        ),
    ] {
        let identifier = scope.id(&format!("Building:unknown-unit-{family}"));
        let refused = client
            .post_entity(&entity_with(&identifier, "measure", &instance))
            .send()
            .await
            .expect("create entity failed");
        assert_eq!(
            refused.status,
            StatusCode::BAD_REQUEST,
            "{family} must refuse a code outside the common-code list: {:?}",
            refused.body
        );
    }

    scope.finish().await.expect("environment cleanup");
}
