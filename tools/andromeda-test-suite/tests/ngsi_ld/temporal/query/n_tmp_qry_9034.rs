//! An instance replaced outside the window does not satisfy q.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities over v observed 10, 200 and 50 selects nothing for v>100 and the
/// Entity for v==50 after 2020-01-03, nothing for v==50 and the Entity for v==200 before 2020-01-03, and the Entity
/// for v>100 between 2020-01-02 and 2020-01-03
/// Clause: 5.7.4.4, 4.11
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9034_an_instance_replaced_outside_the_window_does_not_satisfy_q() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tq_outside_id = namespace.urn("TqOutside", 1)?;
    let tq_outside_type = namespace.entity_type("TqOutside")?;
    let tq_outside_evolution = TemporalEntity::builder()
        .id(tq_outside_id.clone())
        .entity_type(tq_outside_type.clone())
        .property("v", Property::builder(10).observed_at("2020-01-01T00:00:00Z".parse()?))
        .property("v", Property::builder(200).observed_at("2020-01-02T00:00:00Z".parse()?))
        .property("v", Property::builder(50).observed_at("2020-01-03T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&tq_outside_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-03T00:00:00Z".parse()?))
        .entity_type(&tq_outside_type)
        .q(q::attr("v").gt(100))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-03T00:00:00Z".parse()?))
        .entity_type(&tq_outside_type)
        .q(q::attr("v").eq(50))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), contains(has_id(&tq_outside_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::before("2020-01-03T00:00:00Z".parse()?))
        .entity_type(&tq_outside_type)
        .q(q::attr("v").eq(50))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::before("2020-01-03T00:00:00Z".parse()?))
        .entity_type(&tq_outside_type)
        .q(q::attr("v").eq(200))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), contains(has_id(&tq_outside_id)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2020-01-02T00:00:00Z".parse()?, "2020-01-03T00:00:00Z".parse()?))
        .entity_type(&tq_outside_type)
        .q(q::attr("v").gt(100))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(1)));
    expect_that!(page_5.items(), contains(has_id(&tq_outside_id)));
    Ok(())
}
