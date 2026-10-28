//! Annex C.2.3: the `OffStreetParking` Entities in the normalized, concise and simplified representations, as
//! Entities, as `GeoJSON` Features and as a `GeoJSON` `FeatureCollection` (clauses 4.5.16 and 6.3.15).

use crate::{
    parking_example::{PARKINGS, create_parkings, downtown_one, downtown_one_feature, parking_features},
    printed_document::{expect_with_any_context, sort_list_by},
    served_context::{ServedContexts, linked, ok_body},
    vehicle_example::aggregated_context,
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Lossless, NgsiLdMediaType, Representation},
    query::SelectorType,
};
use serde_json::json;

/// The client of a C.2.3 test with both parkings created, and the server of its `@context`.
async fn parkings_created(namespace: &Namespace) -> Result<(ServedContexts, BrokerClient)> {
    let served = ServedContexts::start(namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, namespace).await?)?;
    create_parkings(&broker, namespace, &PARKINGS).await?;
    Ok((served, broker))
}

/// Pins: Retrieve Entity answers the `OffStreetParking` in the normalized representation as C.2.3 prints it
/// Clause: C.2.3, 4.5.1, 4.7, 5.7.1
#[gtest]
#[tokio::test]
async fn c_2_3_0001_the_normalized_parking() -> Result<()> {
    let namespace = Namespace::fresh();
    let (served, broker) = parkings_created(&namespace).await?;
    let id = namespace.urn("OffStreetParking", "Downtown1")?;
    let body = ok_body(
        broker
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .entities()
            .retrieve(&id)
            .send()
            .await,
    )?;
    expect_with_any_context(body, &downtown_one(&namespace, Representation::Normalized));
    served.stop();
    Ok(())
}

/// Pins: Retrieve Entity with format=concise answers the `OffStreetParking` in the concise representation as C.2.3
/// prints it
/// Clause: C.2.3, 4.5.2.3, 5.7.1
#[gtest]
#[tokio::test]
async fn c_2_3_0002_the_concise_parking() -> Result<()> {
    let namespace = Namespace::fresh();
    let (served, broker) = parkings_created(&namespace).await?;
    let id = namespace.urn("OffStreetParking", "Downtown1")?;
    let body = ok_body(
        broker
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .entities()
            .retrieve(&id)
            .form(Lossless::Concise)
            .send()
            .await,
    )?;
    expect_with_any_context(body, &downtown_one(&namespace, Representation::Concise));
    served.stop();
    Ok(())
}

/// Pins: Retrieve Entity with format=simplified answers the `OffStreetParking` in the simplified representation as
/// C.2.3 prints it
/// Clause: C.2.3, 4.5.4, 5.7.1
#[gtest]
#[tokio::test]
async fn c_2_3_0003_the_simplified_parking() -> Result<()> {
    let namespace = Namespace::fresh();
    let (served, broker) = parkings_created(&namespace).await?;
    let id = namespace.urn("OffStreetParking", "Downtown1")?;
    let body = ok_body(
        broker
            .with_accept(NgsiLdMediaType::JsonLd)
            .with_responses()
            .entities()
            .retrieve_simplified(&id)
            .send()
            .await,
    )?;
    expect_with_any_context(body, &downtown_one(&namespace, Representation::Simplified));
    served.stop();
    Ok(())
}

/// Pins: Retrieve Entity with Accept application/geo+json answers the `OffStreetParking` as a `GeoJSON` Feature as
/// C.2.3 prints it
/// Clause: C.2.3, 4.5.16, 6.3.15, 5.7.1
/// Corrected: one-location: Downtown1 is at the [-8.5, 41.2] the first example prints, in geometry and location
/// alike (4.5.16)
#[gtest]
#[tokio::test]
async fn c_2_3_0004_the_normalized_geojson_feature() -> Result<()> {
    let namespace = Namespace::fresh();
    let (served, broker) = parkings_created(&namespace).await?;
    let id = namespace.urn("OffStreetParking", "Downtown1")?;
    let body = ok_body(broker.with_responses().entities().retrieve_geo_json(&id).send().await)?;
    expect_with_any_context(body, &downtown_one_feature(&namespace, Representation::Normalized));
    served.stop();
    Ok(())
}

/// Pins: Retrieve Entity with Accept application/geo+json and format=concise answers a `GeoJSON` Feature with
/// concise properties as C.2.3 prints it
/// Clause: C.2.3, 4.5.16, 6.3.15, 5.7.1
/// Corrected: one-location: Downtown1 is at the [-8.5, 41.2] the first example prints, in geometry and location
/// alike (4.5.16)
#[gtest]
#[tokio::test]
async fn c_2_3_0005_the_concise_geojson_feature() -> Result<()> {
    let namespace = Namespace::fresh();
    let (served, broker) = parkings_created(&namespace).await?;
    let id = namespace.urn("OffStreetParking", "Downtown1")?;
    let body = ok_body(broker.with_responses().entities().retrieve_geo_json(&id).form(Lossless::Concise).send().await)?;
    expect_with_any_context(body, &downtown_one_feature(&namespace, Representation::Concise));
    served.stop();
    Ok(())
}

/// Pins: Retrieve Entity with Accept application/geo+json and format=simplified answers a `GeoJSON` Feature with
/// simplified properties as C.2.3 prints it
/// Clause: C.2.3, 4.5.17, 6.3.15, 5.7.1
/// Corrected: one-location: Downtown1 is at the [-8.5, 41.2] the first example prints, in geometry and location
/// alike (4.5.16)
#[gtest]
#[tokio::test]
async fn c_2_3_0006_the_simplified_geojson_feature() -> Result<()> {
    let namespace = Namespace::fresh();
    let (served, broker) = parkings_created(&namespace).await?;
    let id = namespace.urn("OffStreetParking", "Downtown1")?;
    let body = ok_body(broker.with_responses().entities().retrieve_geo_json_simplified(&id).send().await)?;
    expect_with_any_context(body, &downtown_one_feature(&namespace, Representation::Simplified));
    served.stop();
    Ok(())
}

/// The selector of both parkings' type.
fn parking_type(namespace: &Namespace) -> Result<SelectorType> {
    Ok(namespace.suffixed("OffStreetParking").parse()?)
}

/// Expects `body` to be the `FeatureCollection` of both parkings in `representation`, in any order.
fn expect_parking_collection(mut body: serde_json::Value, namespace: &Namespace, representation: Representation) {
    // Query Entities answers its entities in no specified order (clause 5.7.2).
    sort_list_by(&mut body, "/features", "id");
    expect_with_any_context(
        body,
        &json!({"type": "FeatureCollection", "features": parking_features(namespace, representation)}),
    );
}

/// Pins: Query Entities with Accept application/geo+json answers both parkings as a `GeoJSON` `FeatureCollection`,
/// features in any order, as C.2.3 prints it
/// Clause: C.2.3, 4.5.16, 6.3.15, 5.7.2
/// Corrected: one-location: Downtown1 is at the [-8.5, 41.2] the first example prints, in geometry and location
/// alike (4.5.16)
#[gtest]
#[tokio::test]
async fn c_2_3_0007_the_normalized_geojson_feature_collection() -> Result<()> {
    let namespace = Namespace::fresh();
    let (served, broker) = parkings_created(&namespace).await?;
    let body = ok_body(
        broker
            .with_responses()
            .entities()
            .query_geo_json()
            .entity_type(parking_type(&namespace)?)
            .send()
            .await,
    )?;
    expect_parking_collection(body, &namespace, Representation::Normalized);
    served.stop();
    Ok(())
}

/// Pins: Query Entities with Accept application/geo+json and format=concise answers a `FeatureCollection` with
/// concise properties, features in any order, as C.2.3 prints it
/// Clause: C.2.3, 4.5.16, 6.3.15, 5.7.2
/// Corrected: one-location: Downtown1 is at the [-8.5, 41.2] the first example prints, in geometry and location
/// alike (4.5.16)
#[gtest]
#[tokio::test]
async fn c_2_3_0008_the_concise_geojson_feature_collection() -> Result<()> {
    let namespace = Namespace::fresh();
    let (served, broker) = parkings_created(&namespace).await?;
    let body = ok_body(
        broker
            .with_responses()
            .entities()
            .query_geo_json()
            .entity_type(parking_type(&namespace)?)
            .form(Lossless::Concise)
            .send()
            .await,
    )?;
    expect_parking_collection(body, &namespace, Representation::Concise);
    served.stop();
    Ok(())
}

/// Pins: Query Entities with Accept application/geo+json and format=simplified answers a `FeatureCollection` with
/// simplified properties, features in any order, as C.2.3 prints it
/// Clause: C.2.3, 4.5.17, 6.3.15, 5.7.2
/// Corrected: one-location: Downtown1 is at the [-8.5, 41.2] the first example prints, in geometry and location
/// alike (4.5.16)
#[gtest]
#[tokio::test]
async fn c_2_3_0009_the_simplified_geojson_feature_collection() -> Result<()> {
    let namespace = Namespace::fresh();
    let (served, broker) = parkings_created(&namespace).await?;
    let body = ok_body(
        broker
            .with_responses()
            .entities()
            .query_geo_json_simplified()
            .entity_type(parking_type(&namespace)?)
            .send()
            .await,
    )?;
    expect_parking_collection(body, &namespace, Representation::Simplified);
    served.stop();
    Ok(())
}
