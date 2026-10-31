//! A q on expires at selects by its datetime.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, Entity, NgsiLdMediaType, Property},
    query::q,
};

/// Pins: Query Entities with q `temperature.expiresAt>2150-01-01T00:00:00Z` selects the attribute expiring in 2199
/// and not the one expiring in 2098
/// Clause: 4.9, 4.22, 4.6.3
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9004_a_q_on_expires_at_selects_by_its_datetime() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_expires_soon_id = namespace.urn("Building", "expires-soon")?;
    let building_type = namespace.entity_type("Building")?;
    let building_expires_late_id = namespace.urn("Building", "expires-late")?;
    let building_expires_soon = Entity::builder()
        .id(building_expires_soon_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", Property::builder(20).expires_at("2098-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&building_expires_soon).send().await?.done()?;
    let building_expires_late = Entity::builder()
        .id(building_expires_late_id.clone())
        .entity_type(building_type.clone())
        .property("temperature", Property::builder(20).expires_at("2199-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&building_expires_late).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_expires_late_id.clone()])
        .q(q::attr("temperature").sub_attr("expiresAt").gt("2150-01-01T00:00:00Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_expires_soon_id.clone()])
        .q(q::attr("temperature").sub_attr("expiresAt").gt("2150-01-01T00:00:00Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    Ok(())
}
