//! Every two character escape matches the character it denotes.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::q,
};

/// Pins: Query Entities with name=="x<escape>y" for each of the eight two-character escapes of RFC 8259 section 7
/// answers 200 with the one Entity whose name holds the denoted character between x and y
/// Clause: 4.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9030_every_two_character_escape_matches_the_character_it_denotes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let escape_escape_0_id = namespace.urn("Escape", "escape-0")?;
    let escape_type = namespace.entity_type("Escape")?;
    let escape_escape_1_id = namespace.urn("Escape", "escape-1")?;
    let escape_escape_2_id = namespace.urn("Escape", "escape-2")?;
    let escape_escape_3_id = namespace.urn("Escape", "escape-3")?;
    let escape_escape_4_id = namespace.urn("Escape", "escape-4")?;
    let escape_escape_5_id = namespace.urn("Escape", "escape-5")?;
    let escape_escape_6_id = namespace.urn("Escape", "escape-6")?;
    let escape_escape_7_id = namespace.urn("Escape", "escape-7")?;
    let escape_escape_0 = Entity::builder()
        .id(escape_escape_0_id.clone())
        .entity_type(escape_type.clone())
        .property("name", "x\"y")
        .build()?;
    broker.entities().create(&escape_escape_0).send().await?.done()?;
    let escape_escape_1 = Entity::builder()
        .id(escape_escape_1_id.clone())
        .entity_type(escape_type.clone())
        .property("name", "x\\y")
        .build()?;
    broker.entities().create(&escape_escape_1).send().await?.done()?;
    let escape_escape_2 = Entity::builder()
        .id(escape_escape_2_id.clone())
        .entity_type(escape_type.clone())
        .property("name", "x/y")
        .build()?;
    broker.entities().create(&escape_escape_2).send().await?.done()?;
    let escape_escape_3 = Entity::builder()
        .id(escape_escape_3_id.clone())
        .entity_type(escape_type.clone())
        .property("name", "x\u{8}y")
        .build()?;
    broker.entities().create(&escape_escape_3).send().await?.done()?;
    let escape_escape_4 = Entity::builder()
        .id(escape_escape_4_id.clone())
        .entity_type(escape_type.clone())
        .property("name", "x\u{c}y")
        .build()?;
    broker.entities().create(&escape_escape_4).send().await?.done()?;
    let escape_escape_5 = Entity::builder()
        .id(escape_escape_5_id.clone())
        .entity_type(escape_type.clone())
        .property("name", "x\ny")
        .build()?;
    broker.entities().create(&escape_escape_5).send().await?.done()?;
    let escape_escape_6 = Entity::builder()
        .id(escape_escape_6_id.clone())
        .entity_type(escape_type.clone())
        .property("name", "x\ry")
        .build()?;
    broker.entities().create(&escape_escape_6).send().await?.done()?;
    let escape_escape_7 = Entity::builder()
        .id(escape_escape_7_id.clone())
        .entity_type(escape_type.clone())
        .property("name", "x\ty")
        .build()?;
    broker.entities().create(&escape_escape_7).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&escape_type)
        .ids(vec![escape_escape_0_id.clone()])
        .q(q::attr("name").eq("x\"y"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&escape_escape_0_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&escape_type)
        .ids(vec![escape_escape_1_id.clone()])
        .q(q::attr("name").eq("x\\y"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&escape_escape_1_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&escape_type)
        .ids(vec![escape_escape_2_id.clone()])
        .q(q::attr("name").eq("x/y"))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&escape_escape_2_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&escape_type)
        .ids(vec![escape_escape_3_id.clone()])
        .q(q::attr("name").eq("x\u{8}y"))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&escape_escape_3_id)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&escape_type)
        .ids(vec![escape_escape_4_id.clone()])
        .q(q::attr("name").eq("x\u{c}y"))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(1)));
    expect_that!(page_5.items(), nth(0, has_id(&escape_escape_4_id)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&escape_type)
        .ids(vec![escape_escape_5_id.clone()])
        .q(q::attr("name").eq("x\ny"))
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(1)));
    expect_that!(page_6.items(), nth(0, has_id(&escape_escape_5_id)));
    let page_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&escape_type)
        .ids(vec![escape_escape_6_id.clone()])
        .q(q::attr("name").eq("x\ry"))
        .send()
        .await?;
    expect_that!(page_7.items(), len(eq(1)));
    expect_that!(page_7.items(), nth(0, has_id(&escape_escape_6_id)));
    let page_8 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&escape_type)
        .ids(vec![escape_escape_7_id.clone()])
        .q(q::attr("name").eq("x\ty"))
        .send()
        .await?;
    expect_that!(page_8.items(), len(eq(1)));
    expect_that!(page_8.items(), nth(0, has_id(&escape_escape_7_id)));
    Ok(())
}
