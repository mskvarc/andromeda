//! A sub attribute match outside the window does not select.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Number, Property, Value, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with q=speed.accuracy>0.5 selects nothing between two instants when
/// only an instance observed after the window carries accuracy 0.9, and selects the Car once the window reaches
/// that instance
/// Clause: 5.7.4.4, 4.11, 4.9
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9055_a_sub_attribute_match_outside_the_window_does_not_select() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let car_id = namespace.urn("Car", 1)?;
    let car_type = namespace.entity_type("Car")?;
    let car_evolution = TemporalEntity::builder()
        .id(car_id.clone())
        .entity_type(car_type.clone())
        .property(
            "speed",
            Property::builder(10)
                .observed_at("2020-01-01T12:00:00Z".parse()?)
                .property("accuracy", Value::Number("0.1".parse()?)),
        )
        .property(
            "speed",
            Property::builder(20)
                .observed_at("2020-01-03T00:00:00Z".parse()?)
                .property("accuracy", Value::Number("0.9".parse()?)),
        )
        .build()?;
    broker.temporal().create_or_update(&car_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2020-01-01T00:00:00Z".parse()?, "2020-01-02T00:00:00Z".parse()?))
        .entity_type(&car_type)
        .q(q::attr("speed").sub_attr("accuracy").gt("0.5".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2020-01-01T00:00:00Z".parse()?, "2020-01-04T00:00:00Z".parse()?))
        .entity_type(&car_type)
        .q(q::attr("speed").sub_attr("accuracy").gt("0.5".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&car_id)));
    Ok(())
}
