//! A temporal q on a nested sub attribute.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with q=speed.accuracy.method=="gps_{{ns}}" selects the Car whose
/// speed instance carries that method two levels down and not the Car whose method differs
/// Clause: 5.7.4.4, 4.9
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9056_a_temporal_q_on_a_nested_sub_attribute() -> Result<()> {
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
            Property::builder(10).observed_at("2020-01-01T12:00:00Z".parse()?).property(
                "accuracy",
                Property::builder(Value::Number("0.9".parse()?)).property("method", format!("gps_{namespace}")),
            ),
        )
        .build()?;
    broker.temporal().create_or_update(&car_evolution).send().await?.created()?;
    let car_2_evolution = TemporalEntity::builder()
        .id(car_2_id)
        .entity_type(car_type.clone())
        .property(
            "speed",
            Property::builder(20).observed_at("2020-01-01T12:00:00Z".parse()?).property(
                "accuracy",
                Property::builder(Value::Number("0.9".parse()?)).property("method", format!("manual_{namespace}")),
            ),
        )
        .build()?;
    broker.temporal().create_or_update(&car_2_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&car_type)
        .q(q::attr("speed").sub_attr("accuracy").sub_attr("method").eq(format!("gps_{namespace}").as_str()))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&car_id)));
    Ok(())
}
