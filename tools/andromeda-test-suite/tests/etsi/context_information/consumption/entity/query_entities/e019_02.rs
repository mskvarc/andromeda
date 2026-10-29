//! ETSI `019_02`: entities are queried through the POST interaction, by id, by type, by id pattern, by attribute names,
//! as a `GeoJSON` collection and by several ids (clause 5.7.2).

use crate::{
    suite_keywords::{create_entity, delete_entities, feature_collection},
    suite_payloads::{bare_building, offstreet_parking, vehicle_parked},
};
use andromeda_test_suite::{
    broker,
    comparison::{ContextRule, matches_document},
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::QueryMethod,
    model::{AttributeName, CORE_CONTEXT, NgsiLdMediaType},
    query::{EntitySelector, SelectorType},
};
use serde_json::{Value, json};

/// The terms of the `@context` of the type and attribute cases.
const VEHICLE_TERMS: [&str; 4] = ["Building", "Vehicle", "brandName", "isParked"];

/// The terms of the `@context` of the `GeoJSON` case.
const GEO_TERMS: [&str; 9] = [
    "Vehicle",
    "brandName",
    "isParked",
    "providedBy",
    "OffStreetParking",
    "name",
    "availableSpotNumber",
    "reliability",
    "totalSpotNumber",
];

/// The `isParked` Relationship of a Vehicle, as the Hurl expectations name it: observed at 2017-07-29T12:00:04Z, its
/// `providedBy` written under the default vocabulary.
fn is_parked(namespace: &Namespace) -> Value {
    json!({
        "type": "Relationship",
        "object": namespace.urn_text("OffStreetParking", "Downtown1"),
        "observedAt": "2017-07-29T12:00:04Z",
        "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", "Bob")},
    })
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-<suffix>` as the POST interaction answers it with the vocabulary of the case.
fn vehicle_expected(namespace: &Namespace, suffix: &str) -> Result<Value> {
    Ok(json!({
        "id": namespace.urn_text("Vehicle", suffix),
        "type": namespace.expanded_term("Vehicle")?,
        namespace.expanded_term("brandName")?: {"type": "Property", "value": "Mercedes"},
        namespace.expanded_term("isParked")?: is_parked(namespace),
    }))
}

/// Pins: Check that one can query one entity via POST Interaction based on id
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_02_01_query_one_entity_via_post_interaction_based_on_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    create_entity(&broker, &vocabulary, &bare_building(&namespace, 1)?).await?;
    create_entity(&broker, &vocabulary, &bare_building(&namespace, 2)?).await?;
    create_entity(&broker, &vocabulary, &bare_building(&namespace, 3)?).await?;
    let received = broker
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .entities()
        .query()
        .method(QueryMethod::Post)
        .ids(vec![namespace.urn("Building", 1)?])
        .entity_type(namespace.expanded_term("Building")?.as_str())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": namespace.urn_text("Building", 1),
            "type": namespace.expanded_term("Building")?,
        }]))
        .hiding("instanceId")
    );
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// Pins: Check that one can query several entities via POST Interaction based on the entity type
/// Clause: 5.7.2
/// Corrected: `fix(019_01_02)`: key the query-by-type expectations by expanded iris (5.5.7)
/// Changed: providedBy stays out of the file vocabulary: the case reads it without a @context, so it expands under the
/// core default vocabulary as in the Robot run
/// Changed: observedAt is compared as written, where the case matches the whole-second pattern
#[gtest]
#[tokio::test]
async fn e019_02_02_query_several_entities_via_post_interaction_based_on_the_entities_types() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &VEHICLE_TERMS).await?;
    create_entity(&broker, &vocabulary, &bare_building(&namespace, 1)?).await?;
    create_entity(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    create_entity(&broker, &vocabulary, &vehicle_parked(&namespace, 2)?).await?;
    let received = broker
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(namespace.expanded_term("Vehicle")?.as_str())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!([
        with_body_context(vehicle_expected(&namespace, "1")?),
        with_body_context(vehicle_expected(&namespace, "2")?),
    ]);
    expect_that!(
        &body,
        matches_document(expected).hiding("instanceId").context_rule(ContextRule::AnyCoreContextVersion)
    );
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Vehicle", 1)?, &namespace.urn("Vehicle", 2)?],
    )
    .await;
    Ok(())
}

/// Adds the core `@context` a Vehicle answer carries in its body.
fn with_body_context(mut vehicle: Value) -> Value {
    vehicle["@context"] = json!(CORE_CONTEXT.as_str());
    vehicle
}

/// Pins: Check that one can query several entities via POST Interaction based on the given id pattern
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_02_03_query_several_entities_via_post_interaction_based_on_the_given_id_pattern() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    create_entity(&broker, &vocabulary, &bare_building(&namespace, 1)?).await?;
    create_entity(&broker, &vocabulary, &bare_building(&namespace, 2)?).await?;
    let received = broker
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(namespace.expanded_term("Building")?.as_str())
        .id_pattern(namespace.id_pattern("Building")?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let building = namespace.expanded_term("Building")?;
    expect_that!(
        &body,
        matches_document(json!([
            {"id": namespace.urn_text("Building", 2), "type": building},
            {"id": namespace.urn_text("Building", 1), "type": building},
        ]))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can query several entities via POST Interaction based on attribute names
/// Clause: 5.7.2
/// Corrected: `fix(019_02_04)`: correct the isparked context drift to the suite iri (5.5.7)
/// Changed: providedBy stays out of the file vocabulary: the case reads it without a @context, so it expands under the
/// core default vocabulary as in the Robot run
/// Changed: observedAt is compared as written, where the case matches the whole-second pattern
/// Tenant: private (a query selected by attribute names alone names no entity type, id or idPattern, so clause 5.12
/// matches it with every registration of the tenant that names those attributes or no attribute at all, and the broker
/// forwards it to each; in the shared tenant another file's context source then answers it beside the case's own
/// entities)
#[gtest]
#[tokio::test]
async fn e019_02_04_query_several_entities_via_post_interaction_based_on_attribute_names() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &VEHICLE_TERMS).await?;
    create_entity(&tenant, &vocabulary, &bare_building(&namespace, 1)?).await?;
    create_entity(&tenant, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    let attrs: Vec<AttributeName> = vec![namespace.expanded_term("brandName")?.parse()?, namespace.expanded_term("isParked")?.parse()?];
    let received = tenant
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .entities()
        .query()
        .method(QueryMethod::Post)
        .attrs(attrs)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(&body, matches_document(json!([vehicle_expected(&namespace, "1")?])).hiding("instanceId"));
    delete_entities(&tenant, &[&namespace.urn("Building", 1)?, &namespace.urn("Vehicle", 1)?]).await;
    Ok(())
}

/// Pins: Check that one can query entities via POST Interaction asking for a `GeoJSON` representation
/// Clause: 5.7.2
/// Changed: the response's `@context` may sit on the collection, on each feature, or on both, as the case allows
/// Changed: observedAt and the number values are compared as written, where the case compares them by a hash blind to
/// the number type
#[gtest]
#[tokio::test]
async fn e019_02_05_query_via_post_asking_for_geojson() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &GEO_TERMS).await?;
    create_entity(&broker, &vocabulary, &vehicle_parked(&namespace, "019-02-05")?).await?;
    create_entity(&broker, &vocabulary, &offstreet_parking(&namespace, "019-02-05")?).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .entities()
        .query_geo_json()
        .method(QueryMethod::Post)
        .entity_type(format!("{},{}", namespace.expanded_term("Vehicle")?, namespace.expanded_term("OffStreetParking")?).parse::<SelectorType>()?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let url = vocabulary.url().to_string();
    let features = || [vehicle_feature(&namespace), parking_feature(&namespace)];
    expect_that!(
        &body,
        any!(
            matches_document(feature_collection(features(), &url, true, false)).hiding("instanceId"),
            matches_document(feature_collection(features(), &url, false, true)).hiding("instanceId"),
            matches_document(feature_collection(features(), &url, true, true)).hiding("instanceId"),
        )
    );
    delete_entities(
        &broker,
        &[&namespace.urn("Vehicle", "019-02-05")?, &namespace.urn("OffStreetParking", "019-02-05")?],
    )
    .await;
    Ok(())
}

/// The Feature of the case's Vehicle, which has no location: a `null` geometry, compact property names as the `GeoJSON`
/// representation under the case's vocabulary writes them.
fn vehicle_feature(namespace: &Namespace) -> Value {
    json!({
        "id": namespace.urn_text("Vehicle", "019-02-05"),
        "type": "Feature",
        "geometry": null,
        "properties": {
            "type": "Vehicle",
            "brandName": {"type": "Property", "value": "Mercedes"},
            "isParked": is_parked(namespace),
        },
    })
}

/// The Feature of the case's `OffStreetParking`, located at the point -8.5, 41.2.
fn parking_feature(namespace: &Namespace) -> Value {
    json!({
        "id": namespace.urn_text("OffStreetParking", "019-02-05"),
        "type": "Feature",
        "geometry": {"type": "Point", "coordinates": [-8.5, 41.2]},
        "properties": {
            "type": "OffStreetParking",
            "name": {"type": "Property", "value": "Downtown One"},
            "availableSpotNumber": {
                "type": "Property",
                "value": 121,
                "observedAt": "2017-07-29T12:05:02Z",
                "reliability": {"type": "Property", "value": 0.7},
                "providedBy": {"type": "Relationship", "object": namespace.urn_text("Camera", "C1")},
            },
            "totalSpotNumber": {"type": "Property", "value": 200},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [-8.5, 41.2]}},
        },
    })
}

/// Pins: Check that one can query two entities via POST Interaction based on ids
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_02_06_query_two_entities_via_post_interaction_based_on_ids() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    create_entity(&broker, &vocabulary, &bare_building(&namespace, 1)?).await?;
    create_entity(&broker, &vocabulary, &bare_building(&namespace, 2)?).await?;
    create_entity(&broker, &vocabulary, &bare_building(&namespace, 3)?).await?;
    let received = broker
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .entities()
        .query()
        .method(QueryMethod::Post)
        .selectors([
            EntitySelector::new(namespace.expanded_term("Building")?.parse::<SelectorType>()?).with_ids(namespace.urn("Building", 1)?),
            EntitySelector::new(namespace.expanded_term("Building")?.parse::<SelectorType>()?).with_ids(namespace.urn("Building", 2)?),
        ])
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let building_iri = namespace.expanded_term("Building")?;
    expect_that!(
        &body,
        matches_document(json!([
            {"id": namespace.urn_text("Building", 1), "type": building_iri},
            {"id": namespace.urn_text("Building", 2), "type": building_iri},
        ]))
        .hiding("instanceId")
    );
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}
