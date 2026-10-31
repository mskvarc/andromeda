//! A temporal q on a sub attribute selects by an instance in the window.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Number, Property, Value, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with q=speed.accuracy>0.5 between two instants selects the Car whose
/// in-window speed instance carries accuracy 0.9 and not the one whose instance carries 0.1
/// Clause: 5.7.4.4, 4.9
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9054_a_temporal_q_on_a_sub_attribute_selects_by_an_instance_in_the_window() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let car_id = namespace.urn("Car", 1)?;
    let car_type = namespace.entity_type("Car")?;
    let car_2_id = namespace.urn("Car", 2)?;
    let car_evolution = TemporalEntity::builder()
        .id(car_id.clone())
        .entity_type(car_type.clone())
        .property(
            "speed",
            Property::builder(10)
                .observed_at("2020-01-01T12:00:00Z".parse()?)
                .property("accuracy", Value::Number("0.9".parse()?)),
        )
        .build()?;
    broker.temporal().create_or_update(&car_evolution).send().await?.created()?;
    let car_2_evolution = TemporalEntity::builder()
        .id(car_2_id)
        .entity_type(car_type.clone())
        .property(
            "speed",
            Property::builder(20)
                .observed_at("2020-01-01T12:00:00Z".parse()?)
                .property("accuracy", Value::Number("0.1".parse()?)),
        )
        .build()?;
    broker.temporal().create_or_update(&car_2_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2020-01-01T00:00:00Z".parse()?, "2020-01-02T00:00:00Z".parse()?))
        .entity_type(&car_type)
        .q(q::attr("speed").sub_attr("accuracy").gt("0.5".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&car_id)));
    Ok(())
}
