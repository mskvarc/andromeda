//! A descending order by reverses the collated sequence.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{
        Locality,
        ordering::{OrderBy, builders::EntityOrderingBuilder},
        temporal_query,
    },
};

/// Pins: Query Temporal Evolution of Entities with orderBy=id;desc and collation=und-u-kn-true answers collation30,
/// collation10, collation2
/// Clause: 4.23.1, 5.7.4.4, Table 6.18.3.2-1
/// Recorded: und-u-kn-true (RFC 6067 keyword kn) orders digit runs by value, so collation2 precedes collation10,
/// which neither byte order nor the ICU root collation does.
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9018_a_descending_order_by_reverses_the_collated_sequence() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let collation_reading_collation2_id = namespace.urn("CollationReading", "collation2")?;
    let collation_reading_type = namespace.entity_type("CollationReading")?;
    let collation_reading_collation10_id = namespace.urn("CollationReading", "collation10")?;
    let collation_reading_collation30_id = namespace.urn("CollationReading", "collation30")?;
    let collation_reading_collation2_evolution = TemporalEntity::builder()
        .id(collation_reading_collation2_id.clone())
        .entity_type(collation_reading_type.clone())
        .property("reading", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker
        .temporal()
        .create_or_update(&collation_reading_collation2_evolution)
        .send()
        .await?
        .created()?;
    let collation_reading_collation10_evolution = TemporalEntity::builder()
        .id(collation_reading_collation10_id.clone())
        .entity_type(collation_reading_type.clone())
        .property("reading", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker
        .temporal()
        .create_or_update(&collation_reading_collation10_evolution)
        .send()
        .await?
        .created()?;
    let collation_reading_collation30_evolution = TemporalEntity::builder()
        .id(collation_reading_collation30_id.clone())
        .entity_type(collation_reading_type.clone())
        .property("reading", Property::builder(1).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker
        .temporal()
        .create_or_update(&collation_reading_collation30_evolution)
        .send()
        .await?
        .created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .entity_type(&collation_reading_type)
        .ids(vec![
            collation_reading_collation2_id.clone(),
            collation_reading_collation10_id.clone(),
            collation_reading_collation30_id.clone(),
        ])
        .locality(Locality::Local)
        .ordering(EntityOrderingBuilder::from("id;desc".parse::<OrderBy>()?).collation("und-u-kn-true"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), nth(0, has_id(&collation_reading_collation30_id)));
    expect_that!(page.items(), nth(1, has_id(&collation_reading_collation10_id)));
    expect_that!(page.items(), nth(2, has_id(&collation_reading_collation2_id)));
    Ok(())
}
