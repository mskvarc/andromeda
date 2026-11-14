//! An attribute observed only before the window answers no q.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with attrs=speed between 2018-08-01 12:00 and 13:00, over an Entity
/// whose brand Volvo is observed at 11:00 and one whose brand Volvo is observed at 12:10, both with speed observed at
/// 12:03, selects only the second for brand!="Mercedes" and brand=="Volvo", and nothing for brand=="Mercedes"; q is
/// checked against the instances the temporal query keeps, and an instance observed before the window is not one
/// Clause: 5.7.4.4, 4.11
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9036_an_attribute_observed_only_before_the_window_answers_no_q() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tq_brand_id = namespace.urn("TqBrand", 1)?;
    let tq_brand_type = namespace.entity_type("TqBrand")?;
    let tq_brand_evolution = TemporalEntity::builder()
        .id(tq_brand_id)
        .entity_type(tq_brand_type.clone())
        .property("brand", Property::builder("Volvo").observed_at("2018-08-01T11:00:00Z".parse()?))
        .property("speed", Property::builder(120).observed_at("2018-08-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&tq_brand_evolution).send().await?.created()?;
    let tq_brand_inside_id = namespace.urn("TqBrand", 2)?;
    let tq_brand_inside_evolution = TemporalEntity::builder()
        .id(tq_brand_inside_id.clone())
        .entity_type(tq_brand_type.clone())
        .property("brand", Property::builder("Volvo").observed_at("2018-08-01T12:10:00Z".parse()?))
        .property("speed", Property::builder(120).observed_at("2018-08-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&tq_brand_inside_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2018-08-01T12:00:00Z".parse()?, "2018-08-01T13:00:00Z".parse()?))
        .entity_type(&tq_brand_type)
        .q(q::attr("brand").ne("Mercedes"))
        .attrs(vec!["speed".parse()?])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&tq_brand_inside_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2018-08-01T12:00:00Z".parse()?, "2018-08-01T13:00:00Z".parse()?))
        .entity_type(&tq_brand_type)
        .q(q::attr("brand").eq("Volvo"))
        .attrs(vec!["speed".parse()?])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), contains(has_id(&tq_brand_inside_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2018-08-01T12:00:00Z".parse()?, "2018-08-01T13:00:00Z".parse()?))
        .entity_type(&tq_brand_type)
        .q(q::attr("brand").eq("Mercedes"))
        .attrs(vec!["speed".parse()?])
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    Ok(())
}
