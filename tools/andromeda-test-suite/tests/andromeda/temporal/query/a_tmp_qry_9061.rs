//! A temporal q on a sub attribute is answered at once.

use andromeda_test_suite::{broker, config::BrokerRole, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Number, Property, Value, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: on a broker that defers its history, Query Temporal Evolution with q=speed.accuracy>0.5 sent right after
/// Create or Update Temporal Evolution wrote speed with accuracy 0.9 answers the Entity, and with
/// q=speed.accuracy>0.95 answers nothing
/// Clause: 5.6.11, 5.7.4.4
/// Recorded: the deferred temporal append mode is Andromeda's; clause 5.7.4.4 step S2 checks the query against the
/// Attribute instances in the window, and the instance a write recorded is one of them from the moment the write is
/// acknowledged
#[gtest]
#[tokio::test]
async fn a_tmp_qry_9061_a_temporal_q_on_a_sub_attribute_is_answered_at_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_not_recorded = broker::client(BrokerRole::NotRecorded)?;
    let car_id = namespace.urn("Car", 1)?;
    let car_type = namespace.entity_type("Car")?;
    let car = TemporalEntity::builder()
        .id(car_id.clone())
        .entity_type(car_type.clone())
        .property(
            "speed",
            Property::builder(42)
                .observed_at("2026-03-01T10:00:00Z".parse()?)
                .property("accuracy", Value::Number("0.9".parse()?)),
        )
        .build()?;
    broker_not_recorded.temporal().create_or_update(&car).send().await?.created()?;
    let page = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .entity_type(&car_type)
        .q(q::attr("speed").sub_attr("accuracy").gt("0.5".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&car_id)));
    let page_2 = broker_not_recorded
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2026-01-01T00:00:00Z".parse()?))
        .entity_type(&car_type)
        .q(q::attr("speed").sub_attr("accuracy").gt("0.95".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    Ok(())
}
