//! A three dimensional location instance reads back with its altitude.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        GeoProperty,
        NgsiLdMediaType,
        Value,
        evolution::TemporalEntity,
        geometry::{Position, Shape},
    },
};

/// Pins: Retrieve Temporal Evolution of an Entity answers a three-ordinate location instance with all three
/// ordinates
/// Clause: 4.5.7, 4.7.1, 5.6.11, 5.7.3
#[gtest]
#[tokio::test]
async fn n_tmp_ret_0003_a_three_dimensional_location_instance_reads_back_with_its_altitude() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_evolution = TemporalEntity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_with_altitude(13.4, 52.52, 300.0)?)).observed_at("2020-08-01T12:03:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&building_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "location",
            all!(
                len(eq(1)),
                nth(
                    0,
                    any_attribute()
                        .value_member(["type"], "Point")
                        .length_at(["coordinates"], 3)
                        .value_member(["coordinates", "0"], Value::Number("13.4".parse()?))
                        .value_member(["coordinates", "1"], Value::Number("52.52".parse()?))
                        .value_member(["coordinates", "2"], 300)
                )
            )
        )
    );
    Ok(())
}
