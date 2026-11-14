//! Q reads no instance observed before the window.

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

/// Pins: Query Temporal Evolution of Entities after 2020-01-02 noon over v observed 10 and 200 before the window and
/// 50 inside it selects nothing for v>100, v==200 and v==10, and the Entity for v==50, rendering v with the instance
/// 50 alone
/// Clause: 5.7.4.4, 4.11
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9035_q_reads_no_instance_observed_before_the_window() -> Result<()> {
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
    expect_that!(page.items(), is_empty());
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-02T12:00:00Z".parse()?))
        .entity_type(&tq_in_effect_type)
        .q(q::attr("v").eq(200))
        .send()
        .await?;
    expect_that!(page_2.items(), is_empty());
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
        .q(q::attr("v").eq(50))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), contains(has_id(&tq_in_effect_id)));
    expect_that!(page_4.items(), nth(0, has_evolution("v", all!(len(eq(1)), nth(0, any_attribute().value(50))))));
    Ok(())
}
