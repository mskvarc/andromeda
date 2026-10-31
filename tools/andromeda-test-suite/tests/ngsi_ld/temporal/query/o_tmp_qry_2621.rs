//! Query temporal answers one link line with the context link first.

use andromeda_test_suite::{
    broker,
    link_matcher::{link_value, relations_in_order},
    namespace::Namespace,
    outcome::Upserted,
    wire::header_values,
};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{
    http::headers::{Accept, Link, RelationType},
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities answering application/json over several pages carries one Link field
/// line holding the @context link first, then the pagination links
/// Clause: 6.3.6, 6.3.10, 5.7.4
/// Recorded: Andromeda answers the Link field on one line; RFC 9110 clause 5.3 makes one combined line and several
/// lines equivalent, and the test pins the single line
#[gtest]
#[tokio::test]
async fn o_tmp_qry_2621_query_temporal_answers_one_link_line_with_the_context_link_first() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_evolution = TemporalEntity::builder()
        .id(building_id)
        .entity_type(building_type.clone())
        .property("height", Property::builder(1).observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&building_evolution).send().await?.created()?;
    let building_2_evolution = TemporalEntity::builder()
        .id(building_2_id)
        .entity_type(building_type.clone())
        .property("height", Property::builder(2).observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&building_2_evolution).send().await?.created()?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-07-01T00:00:00Z".parse()?))
        .entity_type(&building_type)
        .limit(1)
        .offset(0)
        .send()
        .await?;
    let page = headed.outcome;
    expect_that!(header_values(headed.head.headers(), &LINK), len(eq(1)));
    expect_that!(
        headed.head.typed::<Link>().valid().and_then(|link| link.values().first()),
        some(link_value(RelationType::JsonLdContext).with_media_type("application/ld+json"))
    );
    expect_that!(
        headed.head.typed::<Link>().valid(),
        some(relations_in_order([RelationType::JsonLdContext, RelationType::Next]))
    );
    expect_that!(page.items(), len(eq(1)));
    Ok(())
}
