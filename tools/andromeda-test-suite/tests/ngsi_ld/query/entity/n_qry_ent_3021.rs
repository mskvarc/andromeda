//! Id takes precedence over id pattern in every post entity selector.

use andromeda_test_suite::{broker, core_context::core_context, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::QueryMethod},
    model::{Entity, NgsiLdMediaType, OneOrMany},
    query::{EntitySelector, IdPattern, builders::SelectorTypeBuilder},
};

/// Pins: Query Entities by POST applies the id and ignores the `idPattern` of an `EntitySelector` that carries
/// both, for an id given as a string and as an array, in each selector of a disjunction, and under an
/// application/ld+json body
/// Clause: 5.2.33, 5.2.23, 6.23.3.1
/// Tenant: private (the broker forwards a query of several selectors to registrations of other types too, so in the
/// shared tenant it reaches other files' sources)
#[gtest]
#[tokio::test]
async fn n_qry_ent_3021_id_takes_precedence_over_id_pattern_in_every_post_entity_selector() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let building_a1_id = namespace.urn("Building", "a1")?;
    let building_type = namespace.entity_type("Building")?;
    let building_a2_id = namespace.urn("Building", "a2")?;
    let building_b1_id = namespace.urn("Building", "b1")?;
    let building_b2_id = namespace.urn("Building", "b2")?;
    let parking_c1_id = namespace.urn("Parking", "c1")?;
    let parking_type = namespace.entity_type("Parking")?;
    let core_context_body = broker.with_context(core_context()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let entities = [
        Entity::builder().id(building_a1_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_a2_id).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_b1_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_b2_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(parking_c1_id.clone()).entity_type(parking_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // 5.2.33: "id takes precedence over idPattern"; the id is an array in the selector
    let selectors = [EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?)
        .with_ids(OneOrMany::try_from(vec![building_b1_id.clone(), building_b2_id.clone()])?)
        .with_id_pattern(format!("^urn:ngsi-ld:Building:{namespace}-a.*").parse::<IdPattern>()?)];
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .selectors(selectors)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&building_b1_id)));
    expect_that!(page.items(), contains(has_id(&building_b2_id)));
    // a pattern matching one of the listed ids and others: the answer is the listed ids
    let selectors_2 = [EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?)
        .with_ids(building_a1_id.clone())
        .with_id_pattern(format!("^urn:ngsi-ld:Building:{namespace}-.*").parse::<IdPattern>()?)];
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .selectors(selectors_2)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&building_a1_id)));
    // two selectors, each with its own id and a pattern that matches none of its ids: the union of the ids
    let selectors_3 = [
        EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?)
            .with_ids(building_b1_id.clone())
            .with_id_pattern(format!("^urn:ngsi-ld:Building:{namespace}-a.*").parse::<IdPattern>()?),
        EntitySelector::new(SelectorTypeBuilder::from(&parking_type).build()?)
            .with_ids(parking_c1_id.clone())
            .with_id_pattern(format!("^urn:ngsi-ld:Parking:{namespace}-zzz.*").parse::<IdPattern>()?),
    ];
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .selectors(selectors_3)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), contains(has_id(&building_b1_id)));
    expect_that!(page_3.items(), contains(has_id(&parking_c1_id)));
    // the same under an application/ld+json body
    let selectors_4 = [EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?)
        .with_ids(building_b2_id.clone())
        .with_id_pattern(format!("^urn:ngsi-ld:Building:{namespace}-a.*").parse::<IdPattern>()?)];
    let page_4 = core_context_body
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .selectors(selectors_4)
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&building_b2_id)));
    Ok(())
}
