//! CSR information rows: `registration_information` holds every selector a
//! registration declares.
//!
//! The rows are the one store of a registration's `information` (clause
//! 5.2.9): every response body is assembled from them, and discovery (clause
//! 5.10) and the forwarding coverage decision (clause 5.10.2) filter on them.
//! They are rewritten inside the same transaction as every registration
//! mutation, so these tests check both halves: that the rows carry each axis
//! the registration declares, and that a query on each axis returns exactly
//! the registrations the registration admits. An axis a row does not constrain
//! is `NULL`. Table 5.2.10-1 ("At least one element of `RegistrationInfo`
//! shall be present") means no item leaves every axis unconstrained at once,
//! so the absent axes are reached one pair at a time.

use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};

/// The suite name every environment in this file is allocated under.
const SUITE: &str = "csr";

/// The information rows of one registration, ordered so two runs compare.
async fn information_rows(pool: &PgPool, registration_id: &str) -> Vec<Value> {
    sqlx::query(
        "SELECT entity_types, entity_id, entity_id_pattern, property_name_keys, relationship_name_keys \
         FROM registration_information WHERE registration_id = $1 \
         ORDER BY item_ordinal, selector_ordinal",
    )
    .bind(registration_id)
    .fetch_all(pool)
    .await
    .expect("the information rows must be readable")
    .into_iter()
    .map(|row| {
        json!({
            "entity_types": row.try_get::<Option<Vec<String>>, _>("entity_types").expect("the entity_types column decodes"),
            "entity_id": row.try_get::<Option<String>, _>("entity_id").expect("the entity_id column decodes"),
            "entity_id_pattern": row.try_get::<Option<String>, _>("entity_id_pattern").expect("the entity_id_pattern column decodes"),
            "property_name_keys": row.try_get::<Option<Vec<i32>>, _>("property_name_keys").expect("the property_name_keys column decodes").map(|keys| keys.len()),
            "relationship_name_keys": row.try_get::<Option<Vec<i32>>, _>("relationship_name_keys").expect("the relationship_name_keys column decodes").map(|keys| keys.len()),
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
async fn a_registration_stores_every_axis_its_information_declares() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let registration_id = scope.id("ContextSourceRegistration:routing-dimensions");
    let entity_id = scope.id("Device:routing-dimensions");

    let created = client
        .create_registration(&selective_registration(&registration_id, &entity_id))
        .await
        .expect("create registration request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create registration: {:?}", created.body);

    let rows = information_rows(scope.database_pool(), &registration_id).await;
    assert_eq!(rows.len(), 1, "one information item with one entity selector is one row: {rows:?}");
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
async fn one_row_is_stored_per_information_item_and_entity_selector() {
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

    let rows = information_rows(scope.database_pool(), &registration_id).await;
    assert_eq!(rows.len(), 3, "two selectors plus one selector is three pairs: {rows:?}");
    assert!(
        rows.iter().any(|row| row["entity_id_pattern"] == json!("urn:ngsi-ld:Device:.*")),
        "the pattern selector keeps its pattern as written: {rows:?}"
    );

    let deleted = client.delete_registration(&registration_id).await.expect("delete registration request failed");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete registration: {:?}", deleted.body);

    scope.finish().await.expect("cleanup failed");
}

/// The axes are independent, so an item declaring only attribute names leaves
/// both entity axes unconstrained and an item declaring only entities leaves
/// both attribute axes unconstrained. Each is one row with that pair absent,
/// and each has to stay a candidate on the axes it does not bound.
///
/// An item bounding no axis at all is not a document a broker accepts: Table
/// 5.2.10-1 requires that "At least one element of `RegistrationInfo` shall be
/// present", and clause 5.9.2.4 makes a registration that does not meet the
/// clause 5.2.9 restrictions a `BadRequestData`.
#[tokio::test]
async fn an_item_leaving_an_axis_unconstrained_stores_it_absent_and_stays_a_candidate() {
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

    let rows = information_rows(scope.database_pool(), &entity_open_id).await;
    assert_eq!(rows.len(), 1, "an item declaring no entity selector is one row: {rows:?}");
    let row = &rows[0];
    assert_eq!(row["entity_types"], Value::Null, "the type axis is absent: {row}");
    assert_eq!(row["entity_id"], Value::Null, "the identifier axis is absent: {row}");
    assert_eq!(row["entity_id_pattern"], Value::Null, "{row}");
    assert_eq!(row["property_name_keys"], json!(1), "the declared property name is interned: {row}");

    let attribute_open = json!({
        "id": attribute_open_id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Device"}]}],
        "endpoint": "http://localhost:9/csource",
        "@context": [CORE_CONTEXT]
    });
    let created = client.create_registration(&attribute_open).await.expect("create registration request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create registration: {:?}", created.body);

    let rows = information_rows(scope.database_pool(), &attribute_open_id).await;
    assert_eq!(rows.len(), 1, "an item declaring no attribute name is one row: {rows:?}");
    let row = &rows[0];
    assert_eq!(row["entity_id"], Value::Null, "the item declared no identifier: {row}");
    assert_eq!(row["property_name_keys"], Value::Null, "the property axis is absent: {row}");
    assert_eq!(row["relationship_name_keys"], Value::Null, "the relationship axis is absent: {row}");

    // An axis a row leaves absent matches everything a query narrows it to
    // (clause 5.12).
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

    // Clause 5.12 fifth bullet: a queried pattern admits a registered pattern
    // without evaluating either, so only the type decides here.
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
async fn updating_and_deleting_a_registration_moves_its_information_rows_with_it() {
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

    let rows = information_rows(scope.database_pool(), &registration_id).await;
    assert_eq!(rows.len(), 1, "the rewritten rows replace the old ones: {rows:?}");
    assert_eq!(
        rows[0]["entity_types"],
        json!(["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"]),
        "the rows follow the information written: {rows:?}"
    );
    assert_eq!(rows[0]["entity_id"], Value::Null, "the replaced item declares no identifier: {rows:?}");
    assert_eq!(
        rows[0]["relationship_name_keys"],
        Value::Null,
        "a name declared on one axis leaves the other absent: {rows:?}"
    );

    let retrieved = client
        .retrieve_registration(&registration_id, None)
        .await
        .expect("retrieve registration request failed");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieve registration: {:?}", retrieved.body);
    let body = retrieved.json_value().expect("a registration answers with JSON");
    assert_eq!(
        body["information"][0]["entities"][0]["type"], "Vehicle",
        "the response is assembled from the rows: {body}"
    );

    let deleted = client.delete_registration(&registration_id).await.expect("delete registration request failed");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete registration: {:?}", deleted.body);
    let after_delete = information_rows(scope.database_pool(), &registration_id).await;
    assert!(
        after_delete.is_empty(),
        "deleting the registration removes its information rows: {after_delete:?}"
    );

    scope.finish().await.expect("cleanup failed");
}
