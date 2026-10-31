//! A sub attribute term reads the instance that carried it.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities over v observed with quality high then low selects the Entity for
/// v.quality=="high" after 2019, nothing for it after 2020-01-02, and the Entity for v.quality=="low" after
/// 2020-01-02
/// Clause: 5.7.4.4, 4.9
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9040_a_sub_attribute_term_reads_the_instance_that_carried_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tq_sub_attribute_id = namespace.urn("TqSubAttribute", 1)?;
    let tq_sub_attribute_type = namespace.entity_type("TqSubAttribute")?;
    let tq_sub_attribute_evolution = TemporalEntity::builder()
        .id(tq_sub_attribute_id.clone())
        .entity_type(tq_sub_attribute_type.clone())
        .property(
            "v",
            Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?).property("quality", "high"),
        )
        .property(
            "v",
            Property::builder(2).observed_at("2020-01-02T00:00:00Z".parse()?).property("quality", "low"),
        )
        .build()?;
    broker.temporal().create_or_update(&tq_sub_attribute_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&tq_sub_attribute_type)
        .q(q::attr("v").sub_attr("quality").eq("high"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&tq_sub_attribute_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-02T00:00:00Z".parse()?))
        .entity_type(&tq_sub_attribute_type)
        .q(q::attr("v").sub_attr("quality").eq("high"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-02T00:00:00Z".parse()?))
        .entity_type(&tq_sub_attribute_type)
        .q(q::attr("v").sub_attr("quality").eq("low"))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), contains(has_id(&tq_sub_attribute_id)));
    Ok(())
}
