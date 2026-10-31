//! A value that is not a geometry is a null geometry.

use andromeda_test_suite::{broker, geometry_matcher::geometry, namespace::Namespace, problem::answered, value::object};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateEntity, RepresentationSpelling},
    },
    model::{AttributeName, Entity, Lossless, NgsiLdMediaType, RawJson, Value},
    query::projection::Projection,
};

/// Pins: Retrieve Entity as `GeoJSON` with `geometryProperty` naming a Property holding a string, a number, a Point
/// without coordinates or an empty `GeometryCollection` answers a Feature whose geometry is null, in every
/// representation, while location still gives the geometry
/// Clause: 4.5.16.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_9012_a_value_that_is_not_a_geometry_is_a_null_geometry() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("buildingNote", "not a geometry")
        .property("buildingCount", 42)
        .property("buildingShape", object([("type", Value::from("Point"))]))
        .property(
            "buildingCollection",
            object([("type", Value::from("GeometryCollection")), ("geometries", Value::Array(vec![]))]),
        )
        .build()?;
    // The model refuses it (the model reads it as written otherwise [at $.location (written [{"type":"GeoProperty","value":{"type":"Point","coordinates":[9.0,0.0]}}])]: {"id":"urn:ngsi-ld:Building:zqns-1","type":"Building_zqns","location":{"type":"GeoProperty","value":{"type":"Point","coordinates":[9.0,0.0]}},"buildingNote":{"type":"Property","value":"not a geometry"},"buildingCount":{"type":"Property","value":42},"buildingShape":{"type":"Property","value":{"type":"Point"}},"buildingCollection":{"type":"Property","value":{"type":"GeometryCollection","geometries":[]}}}), so `location` is written as text.
    let location: RawJson = r#"[{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [9.0, 0.0]}}]"#.parse()?;
    let document = UncheckedDocument::from_entity(&building, Lossless::Normalized).set_member("location", &location);
    let request = broker.unchecked(&CreateEntity::builder().entity(&building).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json(&building_id)
        .geometry_property(&"buildingNote".parse::<AttributeName>()?)
        .send()
        .await?;
    let feature = retrieved.body();
    expect_that!(feature.geometry, none());
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json_simplified(&building_id)
        .geometry_property(&"buildingNote".parse::<AttributeName>()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let feature_2 = retrieved_2.body();
    expect_that!(feature_2.geometry, none());
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json_simplified(&building_id)
        .omit("scope".parse::<Projection>()?)
        .geometry_property(&"buildingNote".parse::<AttributeName>()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let feature_3 = retrieved_3.body();
    expect_that!(feature_3.geometry, none());
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json(&building_id)
        .geometry_property(&"buildingCount".parse::<AttributeName>()?)
        .send()
        .await?;
    let feature_4 = retrieved_4.body();
    expect_that!(feature_4.geometry, none());
    let retrieved_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json_simplified(&building_id)
        .geometry_property(&"buildingCount".parse::<AttributeName>()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let feature_5 = retrieved_5.body();
    expect_that!(feature_5.geometry, none());
    let retrieved_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json_simplified(&building_id)
        .omit("scope".parse::<Projection>()?)
        .geometry_property(&"buildingCount".parse::<AttributeName>()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let feature_6 = retrieved_6.body();
    expect_that!(feature_6.geometry, none());
    let retrieved_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json(&building_id)
        .geometry_property(&"buildingShape".parse::<AttributeName>()?)
        .send()
        .await?;
    let feature_7 = retrieved_7.body();
    expect_that!(feature_7.geometry, none());
    let retrieved_8 = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json_simplified(&building_id)
        .geometry_property(&"buildingShape".parse::<AttributeName>()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let feature_8 = retrieved_8.body();
    expect_that!(feature_8.geometry, none());
    let retrieved_9 = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json_simplified(&building_id)
        .omit("scope".parse::<Projection>()?)
        .geometry_property(&"buildingShape".parse::<AttributeName>()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let feature_9 = retrieved_9.body();
    expect_that!(feature_9.geometry, none());
    let retrieved_10 = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json(&building_id)
        .geometry_property(&"buildingCollection".parse::<AttributeName>()?)
        .send()
        .await?;
    let feature_10 = retrieved_10.body();
    expect_that!(feature_10.geometry, none());
    let retrieved_11 = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json_simplified(&building_id)
        .geometry_property(&"buildingCollection".parse::<AttributeName>()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let feature_11 = retrieved_11.body();
    expect_that!(feature_11.geometry, none());
    let retrieved_12 = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json_simplified(&building_id)
        .omit("scope".parse::<Projection>()?)
        .geometry_property(&"buildingCollection".parse::<AttributeName>()?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let feature_12 = retrieved_12.body();
    expect_that!(feature_12.geometry, none());
    let retrieved_13 = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json(&building_id)
        .send()
        .await?;
    let feature_13 = retrieved_13.body();
    expect_that!(feature_13.geometry.as_ref(), some(geometry().of_type("Point").ordinate([0], 9)));
    Ok(())
}
