//! A relationship term reads the in window objects.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Relationship, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities over `stopsAt` targeting stop A then stop B selects the Entity for
/// `stopsAt` naming stop A after 2019 and nothing for it after 2020-01-02
/// Clause: 5.7.4.4, 4.9
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9041_a_relationship_term_reads_the_in_window_objects() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tq_relationship_id = namespace.urn("TqRelationship", 1)?;
    let tq_relationship_type = namespace.entity_type("TqRelationship")?;
    let stop_a_id = namespace.urn("Stop", "a")?;
    let stop_b_id = namespace.urn("Stop", "b")?;
    let tq_relationship_evolution = TemporalEntity::builder()
        .id(tq_relationship_id.clone())
        .entity_type(tq_relationship_type.clone())
        .relationship("stopsAt", Relationship::builder(stop_a_id).observed_at("2020-01-01T00:00:00Z".parse()?))
        .relationship("stopsAt", Relationship::builder(stop_b_id).observed_at("2020-01-02T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&tq_relationship_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&tq_relationship_type)
        .q(q::attr("stopsAt").eq(format!("urn:ngsi-ld:Stop:{namespace}-a").as_str()))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&tq_relationship_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-02T00:00:00Z".parse()?))
        .entity_type(&tq_relationship_type)
        .q(q::attr("stopsAt").eq(format!("urn:ngsi-ld:Stop:{namespace}-a").as_str()))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    Ok(())
}
