//! A post query orders identifiers under the collation.

use andromeda_test_suite::{broker, identity_matcher::ids_in_order, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{
        Locality,
        ordering::{OrderBy, builders::EntityOrderingBuilder},
        temporal_query,
    },
};

/// Pins: Query Temporal Evolution of Entities through POST with ordering `orderBy` id and collation und-u-kn-true
/// answers collation2, collation10, collation30, and without the collation collation10, collation2, collation30
/// Clause: 4.23.1, 5.2.23, 6.24.1
/// Recorded: und-u-kn-true (RFC 6067 keyword kn) orders digit runs by value, so collation2 precedes collation10,
/// which neither byte order nor the ICU root collation does.
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9019_a_post_query_orders_identifiers_under_the_collation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let collation_reading_type = namespace.entity_type("CollationReading")?;
    let ids = ["collation2", "collation10", "collation30"]
        .into_iter()
        .map(|suffix| namespace.urn("CollationReading", suffix))
        .collect::<Result<Vec<_>, _>>()?;
    for id in &ids {
        let reading = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(collation_reading_type.clone())
            .property("reading", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
            .build()?;
        broker.temporal().create_or_update(&reading).send().await?.created()?;
    }
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let collated = json
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&collation_reading_type)
        .ids(ids.clone())
        .locality(Locality::Local)
        .ordering(EntityOrderingBuilder::from("id".parse::<OrderBy>()?).collation("und-u-kn-true"))
        .send()
        .await?;
    expect_that!(collated.items(), ids_in_order(&[ids[0].clone(), ids[1].clone(), ids[2].clone()]));
    let bytewise = json
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&collation_reading_type)
        .ids(ids.clone())
        .locality(Locality::Local)
        .ordering("id".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(bytewise.items(), ids_in_order(&[ids[1].clone(), ids[0].clone(), ids[2].clone()]));
    Ok(())
}
