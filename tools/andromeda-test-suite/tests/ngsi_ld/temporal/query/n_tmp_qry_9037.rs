//! Each query term is satisfied anywhere in the window.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities over a observed 1 then 9 and b observed red then blue selects the
/// Entity for a==1;b=="blue", a==9;b=="red", a==7|b=="red" and (a>5;b=="red")|a==100, and nothing for
/// a==1;b=="green" and a==7|b=="green"
/// Clause: 5.7.4.4, 4.9
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9037_each_query_term_is_satisfied_anywhere_in_the_window() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tq_tree_id = namespace.urn("TqTree", 1)?;
    let tq_tree_type = namespace.entity_type("TqTree")?;
    let tq_tree_evolution = TemporalEntity::builder()
        .id(tq_tree_id.clone())
        .entity_type(tq_tree_type.clone())
        .property("a", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
        .property("a", Property::builder(9).observed_at("2020-01-02T00:00:00Z".parse()?))
        .property("b", Property::builder("red").observed_at("2020-01-01T00:00:00Z".parse()?))
        .property("b", Property::builder("blue").observed_at("2020-01-02T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&tq_tree_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&tq_tree_type)
        .q(q::attr("a").eq(1).and(q::attr("b").eq("blue")))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&tq_tree_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&tq_tree_type)
        .q(q::attr("a").eq(9).and(q::attr("b").eq("red")))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), contains(has_id(&tq_tree_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&tq_tree_type)
        .q(q::attr("a").eq(7).or(q::attr("b").eq("red")))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), contains(has_id(&tq_tree_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&tq_tree_type)
        .q(q::attr("a").gt(5).and(q::attr("b").eq("red")).or(q::attr("a").eq(100)))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), contains(has_id(&tq_tree_id)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&tq_tree_type)
        .q(q::attr("a").eq(1).and(q::attr("b").eq("green")))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(0)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&tq_tree_type)
        .q(q::attr("a").eq(7).or(q::attr("b").eq("green")))
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(0)));
    Ok(())
}
