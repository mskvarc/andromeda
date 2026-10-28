//! ETSI `019_17`: Buildings and a Vehicle are queried with `pick` or `omit` on their attributes, on their core members,
//! or on both (clauses 5.7.2 and 4.21).

use crate::{
    suite_keywords::{create_eiffel_vehicle_pisa, delete_eiffel_vehicle_pisa},
    suite_payloads::BUILDING_VEHICLE_TERMS,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    query::{
        SelectorType,
        projection::{self, EntityMember},
    },
};
use serde_json::json;

/// Pins: Check that one can query entities with pick on attributes
/// Clause: 5.7.2, 4.21
#[gtest]
#[tokio::test]
async fn e019_17_01_query_with_pick_on_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_VEHICLE_TERMS).await?;
    create_eiffel_vehicle_pisa(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query_projected()
        .entity_type("Building,Vehicle".parse::<SelectorType>()?)
        .pick(projection::attributes(["name"]).build()?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {"name": {"type": "Property", "value": "Eiffel Tower"}},
            {"name": {"type": "Property", "value": "Pisa Tower"}},
        ]))
        .hiding("instanceId")
    );
    delete_eiffel_vehicle_pisa(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that one can query entities with omit on attributes
/// Clause: 5.7.2, 4.21
/// Changed: every `airQualityLevel` value is checked as an integer, where the Hurl file checks it only for the entities
/// whose `observedAt` differs from the expected text
/// Changed: each `observedAt` equals the text written, where the Hurl file matches the whole second and requires one of
/// the three to carry the exact text
#[gtest]
#[tokio::test]
async fn e019_17_02_query_with_omit_on_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_VEHICLE_TERMS).await?;
    create_eiffel_vehicle_pisa(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building,Vehicle".parse::<SelectorType>()?)
        .omit(projection::attributes(["name"]).build()?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", 1),
                "type": "Building",
                "subCategory": {"type": "Property", "value": "tourism"},
                "airQualityLevel": {
                    "type": "Property",
                    "value": 4,
                    "unitCode": "C62",
                    "observedAt": "2020-09-09T16:40:00.000Z",
                },
                "almostFull": {"type": "Property", "value": false},
            },
            {
                "id": namespace.urn_text("Vehicle", 2),
                "type": "Vehicle",
                "brandName": {"type": "Property", "value": "Mercedes"},
                "isParked": {
                    "type": "Relationship",
                    "object": namespace.urn_text("OffStreetParking", "Downtown1"),
                    "observedAt": "2017-07-29T12:00:04Z",
                    "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", "Bob")},
                },
            },
            {
                "id": namespace.urn_text("Building", 3),
                "type": "Building",
                "subCategory": {"type": "Property", "value": "sightseeing"},
                "airQualityLevel": {
                    "type": "Property",
                    "value": 7,
                    "unitCode": "C62",
                    "observedAt": "2023-10-10T16:40:00.000Z",
                },
                "almostFull": {"type": "Property", "value": false},
                "locatedIn": {"type": "Relationship", "object": namespace.urn_text("City", "Pisa")},
            },
        ]))
        .hiding("instanceId")
    );
    delete_eiffel_vehicle_pisa(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that one can query entities with pick on core members
/// Clause: 5.7.2, 4.21
#[gtest]
#[tokio::test]
async fn e019_17_03_query_with_pick_on_non_core_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_VEHICLE_TERMS).await?;
    create_eiffel_vehicle_pisa(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query_projected()
        .entity_type("Building,Vehicle".parse::<SelectorType>()?)
        .pick(projection::member(EntityMember::Id).build()?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {"id": namespace.urn_text("Building", 1)},
            {"id": namespace.urn_text("Vehicle", 2)},
            {"id": namespace.urn_text("Building", 3)},
        ]))
        .hiding("instanceId")
    );
    delete_eiffel_vehicle_pisa(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that one can query entities with omit on core members
/// Clause: 5.7.2, 4.21
/// Changed: each `observedAt` equals the text written, where the Hurl file matches the whole second and requires one of
/// the three to carry the exact text
#[gtest]
#[tokio::test]
async fn e019_17_04_query_with_omit_on_non_core_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_VEHICLE_TERMS).await?;
    create_eiffel_vehicle_pisa(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query_projected()
        .entity_type("Building,Vehicle".parse::<SelectorType>()?)
        .omit(projection::member(EntityMember::Id).build()?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "type": "Building",
                "name": {"type": "Property", "value": "Eiffel Tower"},
                "subCategory": {"type": "Property", "value": "tourism"},
                "airQualityLevel": {
                    "type": "Property",
                    "value": 4,
                    "unitCode": "C62",
                    "observedAt": "2020-09-09T16:40:00.000Z",
                },
                "almostFull": {"type": "Property", "value": false},
            },
            {
                "type": "Vehicle",
                "brandName": {"type": "Property", "value": "Mercedes"},
                "isParked": {
                    "type": "Relationship",
                    "object": namespace.urn_text("OffStreetParking", "Downtown1"),
                    "observedAt": "2017-07-29T12:00:04Z",
                    "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", "Bob")},
                },
            },
            {
                "type": "Building",
                "name": {"type": "Property", "value": "Pisa Tower"},
                "subCategory": {"type": "Property", "value": "sightseeing"},
                "airQualityLevel": {
                    "type": "Property",
                    "value": 7,
                    "unitCode": "C62",
                    "observedAt": "2023-10-10T16:40:00.000Z",
                },
                "almostFull": {"type": "Property", "value": false},
                "locatedIn": {"type": "Relationship", "object": namespace.urn_text("City", "Pisa")},
            },
        ]))
        .hiding("instanceId")
    );
    delete_eiffel_vehicle_pisa(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that one can query entities with pick on attributes and core members
/// Clause: 5.7.2, 4.21
#[gtest]
#[tokio::test]
async fn e019_17_05_query_with_pick_on_attributes_and_core_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_VEHICLE_TERMS).await?;
    create_eiffel_vehicle_pisa(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query_projected()
        .entity_type("Building,Vehicle".parse::<SelectorType>()?)
        .pick(projection::member(EntityMember::Id).attribute("name").build()?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {"id": namespace.urn_text("Building", 1), "name": {"type": "Property", "value": "Eiffel Tower"}},
            {"id": namespace.urn_text("Vehicle", 2)},
            {"id": namespace.urn_text("Building", 3), "name": {"type": "Property", "value": "Pisa Tower"}},
        ]))
        .hiding("instanceId")
    );
    delete_eiffel_vehicle_pisa(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that one can query entities with omit on attributes and core members
/// Clause: 5.7.2, 4.21
/// Changed: every `airQualityLevel` value is checked as an integer, where the Hurl file checks it only for the entities
/// whose `observedAt` differs from the expected text
/// Changed: each `observedAt` equals the text written, where the Hurl file matches the whole second and requires one of
/// the three to carry the exact text
#[gtest]
#[tokio::test]
async fn e019_17_06_query_with_omit_on_attributes_and_core_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_VEHICLE_TERMS).await?;
    create_eiffel_vehicle_pisa(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query_projected()
        .entity_type("Building,Vehicle".parse::<SelectorType>()?)
        .omit(projection::member(EntityMember::Type).attribute("name").build()?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", 1),
                "subCategory": {"type": "Property", "value": "tourism"},
                "airQualityLevel": {
                    "type": "Property",
                    "value": 4,
                    "unitCode": "C62",
                    "observedAt": "2020-09-09T16:40:00.000Z",
                },
                "almostFull": {"type": "Property", "value": false},
            },
            {
                "id": namespace.urn_text("Vehicle", 2),
                "brandName": {"type": "Property", "value": "Mercedes"},
                "isParked": {
                    "type": "Relationship",
                    "object": namespace.urn_text("OffStreetParking", "Downtown1"),
                    "observedAt": "2017-07-29T12:00:04Z",
                    "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", "Bob")},
                },
            },
            {
                "id": namespace.urn_text("Building", 3),
                "subCategory": {"type": "Property", "value": "sightseeing"},
                "airQualityLevel": {
                    "type": "Property",
                    "value": 7,
                    "unitCode": "C62",
                    "observedAt": "2023-10-10T16:40:00.000Z",
                },
                "almostFull": {"type": "Property", "value": false},
                "locatedIn": {"type": "Relationship", "object": namespace.urn_text("City", "Pisa")},
            },
        ]))
        .hiding("instanceId")
    );
    delete_eiffel_vehicle_pisa(&broker, &namespace).await?;
    Ok(())
}
