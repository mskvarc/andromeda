//! An earlier in window instance satisfies q.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities after 2019 over v observed 10, 200 and 50 selects the Entity for
/// v>100, v==200, v==50, v==10, v<20 and v==10..60, and nothing for v>300, v==11 and v<5
/// Clause: 5.7.4.4, 4.9, 4.11
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9033_an_earlier_in_window_instance_satisfies_q() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tq_earlier_id = namespace.urn("TqEarlier", 1)?;
    let tq_earlier_type = namespace.entity_type("TqEarlier")?;
    let tq_earlier_evolution = TemporalEntity::builder()
        .id(tq_earlier_id.clone())
        .entity_type(tq_earlier_type.clone())
        .property("v", Property::builder(10).observed_at("2020-01-01T00:00:00Z".parse()?))
        .property("v", Property::builder(200).observed_at("2020-01-02T00:00:00Z".parse()?))
        .property("v", Property::builder(50).observed_at("2020-01-03T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&tq_earlier_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&tq_earlier_type)
        .q(q::attr("v").gt(100))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&tq_earlier_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&tq_earlier_type)
        .q(q::attr("v").eq(200))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), contains(has_id(&tq_earlier_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&tq_earlier_type)
        .q(q::attr("v").eq(50))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), contains(has_id(&tq_earlier_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&tq_earlier_type)
        .q(q::attr("v").eq(10))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), contains(has_id(&tq_earlier_id)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&tq_earlier_type)
        .q(q::attr("v").lt(20))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(1)));
    expect_that!(page_5.items(), contains(has_id(&tq_earlier_id)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&tq_earlier_type)
        .q(q::attr("v").within(10, 60))
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(1)));
    expect_that!(page_6.items(), contains(has_id(&tq_earlier_id)));
    let page_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&tq_earlier_type)
        .q(q::attr("v").gt(300))
        .send()
        .await?;
    expect_that!(page_7.items(), len(eq(0)));
    let page_8 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&tq_earlier_type)
        .q(q::attr("v").eq(11))
        .send()
        .await?;
    expect_that!(page_8.items(), len(eq(0)));
    let page_9 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&tq_earlier_type)
        .q(q::attr("v").lt(5))
        .send()
        .await?;
    expect_that!(page_9.items(), len(eq(0)));
    Ok(())
}
