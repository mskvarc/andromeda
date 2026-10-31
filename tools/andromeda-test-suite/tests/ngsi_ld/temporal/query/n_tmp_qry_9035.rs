//! The instance in effect when the window opens satisfies q.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities after 2020-01-02 noon over v observed 10, 200 and 50 selects the
/// Entity for v>100 and v==200, nothing for v==10, and renders v with the instance 50 alone
/// Clause: 5.7.4.4, 4.11, C.5.5
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9035_the_instance_in_effect_when_the_window_opens_satisfies_q() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tq_in_effect_id = namespace.urn("TqInEffect", 1)?;
    let tq_in_effect_type = namespace.entity_type("TqInEffect")?;
    let tq_in_effect_evolution = TemporalEntity::builder()
        .id(tq_in_effect_id.clone())
        .entity_type(tq_in_effect_type.clone())
        .property("v", Property::builder(10).observed_at("2020-01-01T00:00:00Z".parse()?))
        .property("v", Property::builder(200).observed_at("2020-01-02T00:00:00Z".parse()?))
        .property("v", Property::builder(50).observed_at("2020-01-03T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&tq_in_effect_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-02T12:00:00Z".parse()?))
        .entity_type(&tq_in_effect_type)
        .q(q::attr("v").gt(100))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&tq_in_effect_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-02T12:00:00Z".parse()?))
        .entity_type(&tq_in_effect_type)
        .q(q::attr("v").eq(200))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), contains(has_id(&tq_in_effect_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-02T12:00:00Z".parse()?))
        .entity_type(&tq_in_effect_type)
        .q(q::attr("v").eq(10))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-02T12:00:00Z".parse()?))
        .entity_type(&tq_in_effect_type)
        .q(q::attr("v").gt(100))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_evolution("v", all!(len(eq(1)), nth(0, any_attribute().value(50))))));
    Ok(())
}
