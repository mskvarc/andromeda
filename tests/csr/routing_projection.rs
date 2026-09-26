//! Verification matrix row "CSR routing projection".
//!
//! `context_source_registrations.information` (clause 5.2.9) stays the
//! authority for every response body; `registration_information` is the same
//! selectors flattened into the shape discovery (clause 5.9) and the forwarding
//! coverage decision (clause 5.10.2) actually filter on. The projection is
//! derived in Rust and rewritten inside the same transaction as every
//! registration mutation, so these tests check both halves: that the rows carry
//! each dimension the documents declare, and that a query on each dimension
//! still returns exactly the registrations the documents admit. An axis a row
//! does not constrain carries a sentinel rather than `NULL` (`{'*'}`, `''`,
//! `{0}`). Table 5.2.10-1 -- "At least one element of `RegistrationInfo` shall be
//! present" -- means no document can leave every axis unconstrained at once, so
//! the sentinels are reached one axis pair at a time.

use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};

/// The suite name every environment in this file is allocated under.
const SUITE: &str = "csr";

/// The routing rows one registration projects, ordered so two runs compare.
async fn routing_rows(pool: &PgPool, registration_id: &str) -> Vec<Value> {
    sqlx::query(
        "SELECT entity_types, entity_id, entity_id_pattern, property_name_keys, relationship_name_keys \
         FROM registration_information WHERE registration_id = $1 \
         ORDER BY entity_types, entity_id, entity_id_pattern",
    )
    .bind(registration_id)
    .fetch_all(pool)
    .await
    .expect("the routing projection must be readable")
    .into_iter()
    .map(|row| {
        json!({
            "entity_types": row.get::<Vec<String>, _>("entity_types"),
            "entity_id": row.get::<String, _>("entity_id"),
            "entity_id_pattern": row.get::<Option<String>, _>("entity_id_pattern"),
            "property_name_keys": row.get::<Vec<i32>, _>("property_name_keys").len(),
            "relationship_name_keys": row.get::<Vec<i32>, _>("relationship_name_keys").len(),
            "property_wildcard": row.get::<Vec<i32>, _>("property_name_keys") == vec![0],
            "relationship_wildcard": row.get::<Vec<i32>, _>("relationship_name_keys") == vec![0],
        })
    })
    .collect()
}

/// The identifiers a registration query returns.
async fn discovered(scope: &TestEnvironment, query: &str) -> Vec<String> {
    let response = scope
        .client()
        .query_registrations(query, None)
        .await
        .expect("registration query request failed");
    assert_eq!(response.status, StatusCode::OK, "registration query: {:?}", response.body);
    response
        .json_value()
        .expect("a registration query answers with JSON")
        .as_array()
        .expect("a registration query answers with an array")
        .iter()
        .filter_map(|registration| registration["id"].as_str().map(str::to_owned))
        .collect()
}

/// A registration whose one information item constrains a type, an exact
/// identifier, a property name and a relationship name at once.
fn selective_registration(id: &str, entity_id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{
            "entities": [{"type": "Device", "id": entity_id}],
            "propertyNames": ["temperature"],
            "relationshipNames": ["installedBy"]
        }],
        "operations": ["retrieveEntity", "federationOps"],
        "endpoint": "http://localhost:9/csource",
        "@context": [CORE_CONTEXT]
    })
}

#[tokio::test]
async fn a_registration_projects_every_dimension_its_information_declares() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let registration_id = scope.id("ContextSourceRegistration:routing-dimensions");
    let entity_id = scope.id("Device:routing-dimensions");

    let created = client
        .create_registration(&selective_registration(&registration_id, &entity_id))
        .await
        .expect("create registration request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create registration: {:?}", created.body);

    let rows = routing_rows(scope.database_pool(), &registration_id).await;
    assert_eq!(rows.len(), 1, "one information item with one entity selector projects one row: {rows:?}");
    let row = &rows[0];
    assert_eq!(
        row["entity_types"],
        json!(["https://uri.etsi.org/ngsi-ld/default-context/Device"]),
        "the type axis carries the expanded name: {row}"
    );
    assert_eq!(row["entity_id"], json!(entity_id), "the identifier axis carries the exact identifier: {row}");
    assert_eq!(row["entity_id_pattern"], Value::Null, "the item declared no pattern: {row}");
    assert_eq!(row["property_name_keys"], json!(1), "the declared property name is interned: {row}");
    assert_eq!(row["relationship_name_keys"], json!(1), "the declared relationship name is interned: {row}");
    assert_eq!(row["property_wildcard"], json!(false), "a declared name is not the sentinel: {row}");
    // Operations stay on the registration: the group token of Table 4.20-2 is
    // a member in its own right rather than being expanded into the operations
    // it stands for.
    let operations: Option<Vec<i16>> = sqlx::query_scalar("SELECT operations FROM context_source_registrations WHERE id = $1")
        .bind(&registration_id)
        .fetch_one(scope.database_pool())
        .await
        .expect("the registration's operations must be readable");
    assert_eq!(operations, Some(vec![22, 44]), "the declared operations are stored as registry numbers");

    let deleted = client.delete_registration(&registration_id).await.expect("delete registration request failed");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete registration: {:?}", deleted.body);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn one_row_is_projected_per_information_item_and_entity_selector() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let registration_id = scope.id("ContextSourceRegistration:routing-pairs");

    let registration = json!({
        "id": registration_id,
        "type": "ContextSourceRegistration",
        "information": [
            {"entities": [{"type": "Device"}, {"type": "Building"}]},
            {"entities": [{"type": "Device", "idPattern": "urn:ngsi-ld:Device:.*"}]}
        ],
        "endpoint": "http://localhost:9/csource",
        "@context": [CORE_CONTEXT]
    });
    let created = client.create_registration(&registration).await.expect("create registration request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create registration: {:?}", created.body);

    let rows = routing_rows(scope.database_pool(), &registration_id).await;
    assert_eq!(rows.len(), 3, "two selectors plus one selector is three pairs: {rows:?}");
    assert!(
        rows.iter().any(|row| row["entity_id_pattern"] == json!("urn:ngsi-ld:Device:.*")),
        "the pattern selector projects its pattern as written: {rows:?}"
    );

    let deleted = client.delete_registration(&registration_id).await.expect("delete registration request failed");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete registration: {:?}", deleted.body);

    scope.finish().await.expect("cleanup failed");
}

/// The axes are independent, so an item declaring only attribute names leaves
/// both entity axes unconstrained and an item declaring only entities leaves
/// both attribute axes unconstrained. Each projects one row carrying that pair
/// of sentinels, and each has to stay a candidate on the dimensions it does not
/// bound.
///
/// An item bounding no axis at all is not a document a broker accepts: Table
/// 5.2.10-1 requires that "At least one element of `RegistrationInfo` shall be
/// present", and clause 5.9.2.4 makes a registration that does not meet the
/// clause 5.2.9 restrictions a `BadRequestData`.
#[tokio::test]
async fn an_item_leaving_an_axis_unconstrained_projects_its_sentinel_and_stays_a_candidate() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let entity_open_id = scope.id("ContextSourceRegistration:routing-open-entities");
    let attribute_open_id = scope.id("ContextSourceRegistration:routing-open-attributes");

    let entity_open = json!({
        "id": entity_open_id,
        "type": "ContextSourceRegistration",
        "information": [{"propertyNames": ["temperature"]}],
        "endpoint": "http://localhost:9/csource",
        "@context": [CORE_CONTEXT]
    });
    let created = client.create_registration(&entity_open).await.expect("create registration request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create registration: {:?}", created.body);

    let rows = routing_rows(scope.database_pool(), &entity_open_id).await;
    assert_eq!(rows.len(), 1, "an item declaring no entity selector projects one row: {rows:?}");
    let row = &rows[0];
    assert_eq!(row["entity_types"], json!(["*"]), "the type axis carries the wildcard sentinel: {row}");
    assert_eq!(row["entity_id"], json!(""), "the identifier axis carries the empty sentinel: {row}");
    assert_eq!(row["entity_id_pattern"], Value::Null, "{row}");
    assert_eq!(row["property_name_keys"], json!(1), "the declared property name is interned: {row}");
    assert_eq!(row["property_wildcard"], json!(false), "a declared name is not the sentinel: {row}");

    let attribute_open = json!({
        "id": attribute_open_id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Device"}]}],
        "endpoint": "http://localhost:9/csource",
        "@context": [CORE_CONTEXT]
    });
    let created = client.create_registration(&attribute_open).await.expect("create registration request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create registration: {:?}", created.body);

    let rows = routing_rows(scope.database_pool(), &attribute_open_id).await;
    assert_eq!(rows.len(), 1, "an item declaring no attribute name projects one row: {rows:?}");
    let row = &rows[0];
    assert_eq!(row["entity_id"], json!(""), "the item declared no identifier: {row}");
    assert_eq!(row["property_wildcard"], json!(true), "the property axis carries the key sentinel: {row}");
    assert_eq!(
        row["relationship_wildcard"],
        json!(true),
        "the relationship axis carries the key sentinel: {row}"
    );

    // Every dimension a row wildcards has to keep admitting it: the preselect
    // may only discard registrations no unnesting could have matched, and a
    // wildcarded axis matches everything a query narrows it to.
    let pattern_query = format!("type=Device&idPattern={}", urlencoding::encode("urn:ngsi-ld:.*"));
    for query in ["type=Device", pattern_query.as_str()] {
        assert!(
            discovered(&scope, query).await.contains(&entity_open_id),
            "a registration declaring no entity must survive the {query} preselect"
        );
    }
    assert!(
        discovered(&scope, "attrs=temperature").await.contains(&attribute_open_id),
        "a registration declaring no attribute name must survive the attrs preselect"
    );

    for id in [&entity_open_id, &attribute_open_id] {
        let deleted = client.delete_registration(id).await.expect("delete registration request failed");
        assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete registration: {:?}", deleted.body);
    }

    scope.finish().await.expect("cleanup failed");
}

/// The preselect may only narrow: every dimension has to return exactly the
/// registrations the documents admit, never one fewer.
#[tokio::test]
async fn a_query_on_each_dimension_returns_exactly_the_registrations_the_documents_admit() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let selective_id = scope.id("ContextSourceRegistration:routing-selective");
    let other_id = scope.id("ContextSourceRegistration:routing-other");
    let entity_id = scope.id("Device:routing-selective");

    let created = client
        .create_registration(&selective_registration(&selective_id, &entity_id))
        .await
        .expect("create registration request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create registration: {:?}", created.body);

    let other = json!({
        "id": other_id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Vehicle"}], "propertyNames": ["speed"]}],
        "endpoint": "http://localhost:9/csource",
        "@context": [CORE_CONTEXT]
    });
    let created_other = client.create_registration(&other).await.expect("create registration request failed");
    assert_eq!(created_other.status, StatusCode::CREATED, "create registration: {:?}", created_other.body);

    let by_type = discovered(&scope, "type=Device").await;
    assert!(by_type.contains(&selective_id), "the type dimension must admit the declaring registration");
    assert!(!by_type.contains(&other_id), "a registration declaring another type is not a candidate");

    // The pattern axis is compared as text rather than evaluated: discovery asks
    // which registrations advertise a pattern, not which patterns match.
    let by_pattern = discovered(&scope, &format!("type=Device&idPattern={}", urlencoding::encode("urn:ngsi-ld:Device:.*"))).await;
    assert!(
        !by_pattern.contains(&other_id),
        "a registration declaring another type is not a candidate on the pattern dimension"
    );

    let by_attribute = discovered(&scope, "attrs=temperature").await;
    assert!(
        by_attribute.contains(&selective_id),
        "the attribute dimension must admit the registration declaring the property"
    );
    assert!(
        !by_attribute.contains(&other_id),
        "a registration declaring another attribute is not a candidate"
    );

    let by_relationship = discovered(&scope, "attrs=installedBy").await;
    assert!(
        by_relationship.contains(&selective_id),
        "a declared relationship name is a candidate on the same dimension as a property name"
    );

    for id in [&selective_id, &other_id] {
        let deleted = client.delete_registration(id).await.expect("delete registration request failed");
        assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete registration: {:?}", deleted.body);
    }

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn updating_and_deleting_a_registration_moves_its_payload_and_its_routing_rows_together() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let registration_id = scope.id("ContextSourceRegistration:routing-mutation");
    let entity_id = scope.id("Device:routing-mutation");

    let created = client
        .create_registration(&selective_registration(&registration_id, &entity_id))
        .await
        .expect("create registration request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create registration: {:?}", created.body);

    let update = json!({"information": [{"entities": [{"type": "Vehicle"}], "propertyNames": ["speed"]}]});
    let updated = client
        .update_registration(&registration_id, &update, None)
        .await
        .expect("update registration request failed");
    assert_eq!(updated.status, StatusCode::NO_CONTENT, "update registration: {:?}", updated.body);

    let rows = routing_rows(scope.database_pool(), &registration_id).await;
    assert_eq!(rows.len(), 1, "the rewritten projection is a set, not an accumulation: {rows:?}");
    assert_eq!(
        rows[0]["entity_types"],
        json!(["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"]),
        "the rows follow the payload they were derived from: {rows:?}"
    );
    assert_eq!(rows[0]["entity_id"], json!(""), "the replaced item declares no identifier: {rows:?}");
    assert_eq!(
        rows[0]["relationship_name_keys"],
        json!(0),
        "a name declared on one axis leaves the other empty, not wildcarded: {rows:?}"
    );

    let retrieved = client
        .retrieve_registration(&registration_id, None)
        .await
        .expect("retrieve registration request failed");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieve registration: {:?}", retrieved.body);
    let body = retrieved.json_value().expect("a registration answers with JSON");
    assert_eq!(
        body["information"][0]["entities"][0]["type"], "Vehicle",
        "the document is the authority for the response: {body}"
    );

    let deleted = client.delete_registration(&registration_id).await.expect("delete registration request failed");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete registration: {:?}", deleted.body);
    let after_delete = routing_rows(scope.database_pool(), &registration_id).await;
    assert!(after_delete.is_empty(), "deleting the registration removes its routing rows: {after_delete:?}");

    scope.finish().await.expect("cleanup failed");
}
