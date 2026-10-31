//! Id takes precedence over id pattern in every get parameter combination.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{IdPattern, q},
};

/// Pins: Query Entities by GET applies id and ignores `idPattern` when both are given, for one id or several,
/// whether the pattern matches none, some or only other entities, and under type and attrs as the narrowing input
/// alike
/// Clause: 5.2.33, 6.4.3.2, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_ent_3020_id_takes_precedence_over_id_pattern_in_every_get_parameter_combination() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_a1_id = namespace.urn("Building", "a1")?;
    let building_type = namespace.entity_type("Building")?;
    let building_a2_id = namespace.urn("Building", "a2")?;
    let building_b1_id = namespace.urn("Building", "b1")?;
    let building_b2_id = namespace.urn("Building", "b2")?;
    let entities = [
        Entity::builder()
            .id(building_a1_id.clone())
            .entity_type(building_type.clone())
            .property(namespace.attribute("name")?, "a1")
            .build()?,
        Entity::builder()
            .id(building_a2_id)
            .entity_type(building_type.clone())
            .property(namespace.attribute("name")?, "a2")
            .build()?,
        Entity::builder()
            .id(building_b1_id.clone())
            .entity_type(building_type.clone())
            .property(namespace.attribute("name")?, "b1")
            .build()?,
        Entity::builder()
            .id(building_b2_id.clone())
            .entity_type(building_type.clone())
            .property(namespace.attribute("name")?, "b2")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // 5.2.33: "id takes precedence over idPattern": the pattern matches no listed id
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_b1_id.clone()])
        .id_pattern(format!("^urn:ngsi-ld:Building:{namespace}-a.*").parse::<IdPattern>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_b1_id)));
    // several ids, a pattern matching none of them
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_b1_id.clone(), building_b2_id.clone()])
        .id_pattern(format!("^urn:ngsi-ld:Building:{namespace}-a.*").parse::<IdPattern>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&building_b1_id)));
    expect_that!(page_2.items(), contains(has_id(&building_b2_id)));
    // several ids, a pattern matching one of them: the other listed id is still answered
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_a1_id.clone(), building_b1_id.clone()])
        .id_pattern(format!("^urn:ngsi-ld:Building:{namespace}-a.*").parse::<IdPattern>()?)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), contains(has_id(&building_a1_id)));
    expect_that!(page_3.items(), contains(has_id(&building_b1_id)));
    // a pattern matching other entities beside the listed one does not widen the answer
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_a1_id.clone()])
        .id_pattern(format!("^urn:ngsi-ld:Building:{namespace}-.*").parse::<IdPattern>()?)
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&building_a1_id)));
    // the attribute list as the narrowing input instead of the type
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .attrs(vec![format!("name_{namespace}").parse()?])
        .ids(vec![building_b2_id.clone()])
        .id_pattern(format!("^urn:ngsi-ld:Building:{namespace}-a.*").parse::<IdPattern>()?)
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(1)));
    expect_that!(page_5.items(), nth(0, has_id(&building_b2_id)));
    // a q as the narrowing input
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .q(q::attr(&namespace.attribute("name")?).eq("b2"))
        .ids(vec![building_b2_id.clone()])
        .id_pattern(format!("^urn:ngsi-ld:Building:{namespace}-a.*").parse::<IdPattern>()?)
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(1)));
    expect_that!(page_6.items(), nth(0, has_id(&building_b2_id)));
    Ok(())
}
