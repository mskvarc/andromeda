//! The temporal values representation keeps the altitude of each instance.

use andromeda_test_suite::{broker, geometry_matcher::geometry, namespace::Namespace, outcome::Upserted, simplified_pairs::geo_pairs};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{
        GeoProperty,
        NgsiLdMediaType,
        Value,
        evolution::TemporalEntity,
        geometry::{Position, Shape},
    },
    query::temporal_query,
};

/// Pins: the temporalValues representation of a location with two three-ordinate instances carries each value with all
/// three ordinates, in time order
/// Clause: 4.5.7, 4.7.1, 5.6.12, 5.7.3
#[gtest]
#[tokio::test]
async fn n_tmp_trl_0003_the_temporal_values_representation_keeps_the_altitude_of_each_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_evolution = TemporalEntity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_with_altitude(13.4, 52.52, 300.0)?)).observed_at("2020-08-01T12:03:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&building_evolution).send().await?.created()?;
    let instances = TemporalEntity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_with_altitude(13.4, 52.52, 350.0)?)).observed_at("2020-08-01T12:05:00Z".parse()?),
        )
        .build()?
        .attributes;
    broker.temporal().add_attributes(&building_id, &instances).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_values(&building_id)
        .temporal_q(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    let pairs = geo_pairs(retrieved.body(), "location").or_fail()?;
    expect_that!(pairs, len(eq(2)));
    for (index, altitude) in ["300.0", "350.0"].into_iter().enumerate() {
        let pair = pairs.get(index).or_fail()?;
        expect_that!(
            pair.value.as_geometry(),
            some(
                geometry()
                    .of_type("Point")
                    .length([], 3)
                    .ordinate([0], Value::Number("13.4".parse()?))
                    .ordinate([1], Value::Number("52.52".parse()?))
                    .ordinate([2], Value::Number(altitude.parse()?))
            )
        );
    }
    Ok(())
}
