//! A geometry keeps its bbox and foreign members in every representation.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    geometry_matcher::geometry,
    geometry_members::{bbox, foreign_member, geo_property_geometry, simplified_geometry},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{AttributeName, Entity, Input, Lossless, NgsiLdMediaType, Value, geometry::Geometry},
};

/// The entity as the request writes it.
const BODY: &str = r#"{
    "id": "urn:ngsi-ld:Parcel:{{ns}}-1",
    "type": "Parcel_{{ns}}",
    "area": {
        "type": "GeoProperty",
        "value": {
            "type": "Polygon",
            "coordinates": [
                [
                    [
                        0.0,
                        0.0
                    ],
                    [
                        4.0,
                        0.0
                    ],
                    [
                        4.0,
                        4.0
                    ],
                    [
                        0.0,
                        0.0
                    ]
                ]
            ],
            "bbox": [
                0.0,
                0.0,
                4.0,
                4.0
            ],
            "plotName": "plot 7",
            "survey": {
                "year": 2024,
                "marks": [
                    1,
                    2
                ]
            }
        }
    },
    "marker": {
        "type": "Property",
        "value": {
            "type": "Point",
            "coordinates": [
                1.0,
                1.0,
                3.0
            ],
            "bbox": [
                1.0,
                1.0,
                3.0,
                1.0,
                1.0,
                3.0
            ]
        }
    }
}"#;

/// The bounding box, the foreign members and the shape of the Polygon `area` in any representation.
fn expect_area(area: Option<&Geometry>) {
    expect_that!(area, some(geometry().of_type("Polygon").length([0], 4)));
    expect_that!(area.and_then(bbox), some(elements_are![eq(&0.0), eq(&0.0), eq(&4.0), eq(&4.0)]));
    expect_that!(area.and_then(|area| foreign_member(area, "plotName")), some(eq(&serde_json::json!("plot 7"))));
    expect_that!(
        area.and_then(|area| foreign_member(area, "survey")),
        some(eq(&serde_json::json!({"year": 2024, "marks": [1, 2]})))
    );
}

/// Pins: Create Entity with a `GeoProperty` Polygon carrying a bbox and two foreign members, one nested, and a
/// Property holding a Point with a three-dimensional bbox answers 201, and the bbox and foreign members read back
/// in the normalized, the simplified and the `GeoJSON` representation
/// Clause: 4.6.4, 4.7.2, 4.5.16.2
#[gtest]
#[tokio::test]
async fn n_ent_cre_9010_a_geometry_keeps_its_bbox_and_foreign_members_in_every_representation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let parcel_id = namespace.urn("Parcel", 1)?;
    let parcel = Entity::<Input>::parse(&BODY.replace("{{ns}}", namespace.token()), Lossless::Normalized)?;
    broker.entities().create(&parcel).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&parcel_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_area(geo_property_geometry(entity, "area"));
    expect_that!(
        entity,
        has_attribute(
            "marker",
            property()
                .length_at(["bbox"], 6)
                .value_member(["bbox", "2"], Value::Number("3.0".parse()?))
                .value_member(["bbox", "5"], Value::Number("3.0".parse()?))
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&parcel_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    expect_area(simplified_geometry(retrieved_2.body(), "area"));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .retrieve_geo_json(&parcel_id)
        .geometry_property(&"area".parse::<AttributeName>()?)
        .send()
        .await?;
    expect_area(retrieved_3.body().geometry.as_ref());
    Ok(())
}
