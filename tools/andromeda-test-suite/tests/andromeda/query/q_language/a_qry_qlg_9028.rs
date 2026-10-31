//! The extensions both engines share are executed.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Locality, q},
};

/// Pins: Query Entities with q patterns using a set inline option, a non-capturing group, a digit class inside a
/// list and the shorthand classes each selects the Entity whose name is Merc3des
/// Clause: 4.9
/// Recorded: Andromeda extension, not v1.9.1: clause 4.9 hands `RegExp` to IEEE 1003.2, which has no inline
/// options, non-capturing groups or shorthand classes; Andromeda executes (?m), (?s), (?x), (?:...), \d, \D, \w and
/// \S
#[gtest]
#[tokio::test]
async fn a_qry_qlg_9028_the_extensions_both_engines_share_are_executed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_shared_shorthand_id = namespace.urn("Building", "shared-shorthand")?;
    let building_type = namespace.entity_type("Building")?;
    let building_shared_shorthand = Entity::builder()
        .id(building_shared_shorthand_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Merc3des")
        .build()?;
    broker.entities().create(&building_shared_shorthand).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_shared_shorthand_id.clone()])
        .q(q::attr("name").matches("\"(?m)^Merc\""))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_shared_shorthand_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_shared_shorthand_id.clone()])
        .q(q::attr("name").matches("\"(?s)M.*s\""))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&building_shared_shorthand_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_shared_shorthand_id.clone()])
        .q(q::attr("name").matches("\"(?x)M e r c\""))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&building_shared_shorthand_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_shared_shorthand_id.clone()])
        .q(q::attr("name").matches("\"(?:Merc)+\""))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&building_shared_shorthand_id)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_shared_shorthand_id.clone()])
        .q(q::attr("name").matches("\"[[:alnum:]]+\""))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(1)));
    expect_that!(page_5.items(), nth(0, has_id(&building_shared_shorthand_id)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_shared_shorthand_id.clone()])
        .q(q::attr("name").matches("\"[Merc[:digit:]]+\""))
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(1)));
    expect_that!(page_6.items(), nth(0, has_id(&building_shared_shorthand_id)));
    let page_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_shared_shorthand_id.clone()])
        .q(q::attr("name").matches("\"Merc[[:digit:]]de\""))
        .send()
        .await?;
    expect_that!(page_7.items(), len(eq(1)));
    expect_that!(page_7.items(), nth(0, has_id(&building_shared_shorthand_id)));
    let page_8 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_shared_shorthand_id.clone()])
        .q(q::attr("name").matches("\"Merc\\dde\""))
        .send()
        .await?;
    expect_that!(page_8.items(), len(eq(1)));
    expect_that!(page_8.items(), nth(0, has_id(&building_shared_shorthand_id)));
    let page_9 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_shared_shorthand_id.clone()])
        .q(q::attr("name").matches("\"^\\w+$\""))
        .send()
        .await?;
    expect_that!(page_9.items(), len(eq(1)));
    expect_that!(page_9.items(), nth(0, has_id(&building_shared_shorthand_id)));
    let page_10 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_shared_shorthand_id.clone()])
        .q(q::attr("name").matches("\"^\\S+$\""))
        .send()
        .await?;
    expect_that!(page_10.items(), len(eq(1)));
    expect_that!(page_10.items(), nth(0, has_id(&building_shared_shorthand_id)));
    let page_11 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_shared_shorthand_id.clone()])
        .q(q::attr("name").matches("\"Merc[\\d]\\D\""))
        .send()
        .await?;
    expect_that!(page_11.items(), len(eq(1)));
    expect_that!(page_11.items(), nth(0, has_id(&building_shared_shorthand_id)));
    Ok(())
}
