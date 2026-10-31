//! Every posix extended regular expression is executed.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Locality, q},
};

/// Pins: Query Entities with q patterns using anchors, wildcards, bracket expressions, alternation, intervals,
/// character classes, literal hyphens and escaped special characters each selects the Entity whose name is Mercedes
/// Clause: 4.9; IEEE 1003.2
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9027_every_posix_extended_regular_expression_is_executed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_posix_match_id = namespace.urn("Building", "posix-match")?;
    let building_type = namespace.entity_type("Building")?;
    let building_posix_match = Entity::builder()
        .id(building_posix_match_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Mercedes")
        .build()?;
    broker.entities().create(&building_posix_match).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_match_id.clone()])
        .q(q::attr("name").matches("\"^Merc\""))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_posix_match_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_match_id.clone()])
        .q(q::attr("name").matches("\"M.*s$\""))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&building_posix_match_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_match_id.clone()])
        .q(q::attr("name").matches("\"Merc[e]des\""))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&building_posix_match_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_match_id.clone()])
        .q(q::attr("name").matches("\"Mer(c|k)edes\""))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&building_posix_match_id)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_match_id.clone()])
        .q(q::attr("name").matches("\"e.{2}e\""))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(1)));
    expect_that!(page_5.items(), nth(0, has_id(&building_posix_match_id)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_match_id.clone()])
        .q(q::attr("name").matches("\"[[:alpha:]]+\""))
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(1)));
    expect_that!(page_6.items(), nth(0, has_id(&building_posix_match_id)));
    let page_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_match_id.clone()])
        .q(q::attr("name").matches("\"[[:lower:]]+\""))
        .send()
        .await?;
    expect_that!(page_7.items(), len(eq(1)));
    expect_that!(page_7.items(), nth(0, has_id(&building_posix_match_id)));
    let page_8 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_match_id.clone()])
        .q(q::attr("name").matches("\"[]M]erc\""))
        .send()
        .await?;
    expect_that!(page_8.items(), len(eq(1)));
    expect_that!(page_8.items(), nth(0, has_id(&building_posix_match_id)));
    let page_9 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_match_id.clone()])
        .q(q::attr("name").matches("\"Merc{1}edes\""))
        .send()
        .await?;
    expect_that!(page_9.items(), len(eq(1)));
    expect_that!(page_9.items(), nth(0, has_id(&building_posix_match_id)));
    let page_10 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_match_id.clone()])
        .q(q::attr("name").matches("\"Mer[a-z]+es\""))
        .send()
        .await?;
    expect_that!(page_10.items(), len(eq(1)));
    expect_that!(page_10.items(), nth(0, has_id(&building_posix_match_id)));
    let page_11 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_match_id.clone()])
        .q(q::attr("name").matches("\"[-M]erc\""))
        .send()
        .await?;
    expect_that!(page_11.items(), len(eq(1)));
    expect_that!(page_11.items(), nth(0, has_id(&building_posix_match_id)));
    let page_12 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_match_id.clone()])
        .q(q::attr("name").matches("\"Merc[e-]des\""))
        .send()
        .await?;
    expect_that!(page_12.items(), len(eq(1)));
    expect_that!(page_12.items(), nth(0, has_id(&building_posix_match_id)));
    let page_13 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_match_id.clone()])
        .q(q::attr("name").matches("\"[M\\-]erc\""))
        .send()
        .await?;
    expect_that!(page_13.items(), len(eq(1)));
    expect_that!(page_13.items(), nth(0, has_id(&building_posix_match_id)));
    let page_14 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_match_id.clone()])
        .q(q::attr("name").matches("\"Merc\\.?edes\""))
        .send()
        .await?;
    expect_that!(page_14.items(), len(eq(1)));
    expect_that!(page_14.items(), nth(0, has_id(&building_posix_match_id)));
    let page_15 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_match_id.clone()])
        .q(q::attr("name").matches("\"^\\(?Merc\""))
        .send()
        .await?;
    expect_that!(page_15.items(), len(eq(1)));
    expect_that!(page_15.items(), nth(0, has_id(&building_posix_match_id)));
    let page_16 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_match_id.clone()])
        .q(q::attr("name").matches("\"Merc\\+?edes\""))
        .send()
        .await?;
    expect_that!(page_16.items(), len(eq(1)));
    expect_that!(page_16.items(), nth(0, has_id(&building_posix_match_id)));
    let page_17 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_posix_match_id.clone()])
        .q(q::attr("name").matches("\"Mercedes\\$?\""))
        .send()
        .await?;
    expect_that!(page_17.items(), len(eq(1)));
    expect_that!(page_17.items(), nth(0, has_id(&building_posix_match_id)));
    Ok(())
}
