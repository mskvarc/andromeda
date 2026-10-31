//! A hop past the first two hundred entities pages in identifier order without the expired links.

use andromeda_test_suite::{
    broker,
    identity_matcher::{has_id, ids_in_order},
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, Input, ListRelationship, NgsiLdMediaType, Relationship},
    query::{Join, Locality, q},
};

/// Stops the batch creates; only those past the first two hundred link to the Line the inner term keeps.
const STOPS: i32 = 300;

/// Pins: Query Entities with a q hop over 300 Entities, of which 40 after the first 200 link to an Entity the inner
/// term keeps through a single object, an array of objects and an `objectList`, while ten more link to it only past
/// their `expiresAt`, pages the 40 in identifier order with a total of 40, under a type list, beside a comparison,
/// inside a disjunction and with no linked match
/// Clause: 4.5.23, 4.9, 4.22, 5.5.9, 6.3.13
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_0013_a_hop_past_the_first_two_hundred_entities_pages_in_identifier_order_without_the_expired_links() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let line_a_id = namespace.urn("Line", "a")?;
    let line_type = namespace.entity_type("Line")?;
    let line_b_id = namespace.urn("Line", "b")?;
    let line_c_id = namespace.urn("Line", "c")?;
    let tram_type = namespace.entity_type("Tram")?;
    let stop_type = namespace.entity_type("Stop")?;
    let stop_id = |seq: i32| namespace.urn("Stop", format!("{seq:04}"));
    let stop = |seq: i32| -> Result<Entity<Input>> {
        let builder = Entity::builder().id(stop_id(seq)?).entity_type(stop_type.clone()).property("seq", seq);
        let builder = match seq {
            201..=220 => builder.relationship("servedBy", line_a_id.clone()),
            221..=230 => builder.relationship("servedBy", Relationship::builder(line_a_id.clone()).expires_at("2020-01-01T00:00:00Z".parse()?)),
            231..=240 => builder.relationship("servedBy", vec![line_b_id.clone(), line_c_id.clone()]),
            241..=250 => builder.list_relationship("servedBy", ListRelationship::builder(vec![line_b_id.clone(), line_c_id.clone()])),
            _ => builder.relationship("servedBy", line_b_id.clone()),
        };
        Ok(builder.build()?)
    };
    let mut entities = vec![
        Entity::builder()
            .id(line_a_id.clone())
            .entity_type(line_type.clone())
            .property("headSign", "north")
            .build()?,
        Entity::builder()
            .id(line_b_id.clone())
            .entity_type(line_type.clone())
            .property("headSign", "south")
            .build()?,
        Entity::builder()
            .id(line_c_id.clone())
            .entity_type(tram_type)
            .property("headSign", "north")
            .build()?,
    ];
    entities.extend((1..=STOPS).map(stop).collect::<Result<Vec<_>>>()?);
    let entities = entities;
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&stop_type)
        .q(q::attr("servedBy").linked("headSign").eq("north"))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .limit(20)
        .count(ResultsCount::Include)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(40)));
    let expected_ids = (201..=220).map(|n| namespace.urn("Stop", format!("{n:04}"))).collect::<Result<Vec<_>, _>>()?;
    expect_that!(page.items(), ids_in_order(&expected_ids));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&stop_type)
        .q(q::attr("servedBy").linked("headSign").eq("north"))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .limit(20)
        .offset(20)
        .count(ResultsCount::Include)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(40)));
    let expected_ids_2 = (231..=250).map(|n| namespace.urn("Stop", format!("{n:04}"))).collect::<Result<Vec<_>, _>>()?;
    expect_that!(page_2.items(), ids_in_order(&expected_ids_2));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&stop_type)
        .q(q::attr("servedBy")
            .linked_typed([&line_type, &namespace.entity_type("Bus")?], "headSign")
            .eq("north"))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .limit(30)
        .count(ResultsCount::Include)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_3.total(), some(eq(20)));
    let expected_ids_3 = (201..=220).map(|n| namespace.urn("Stop", format!("{n:04}"))).collect::<Result<Vec<_>, _>>()?;
    expect_that!(page_3.items(), ids_in_order(&expected_ids_3));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&stop_type)
        .q(q::attr("servedBy").linked("headSign").eq("north").and(q::attr("seq").gt(245)))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .limit(10)
        .count(ResultsCount::Include)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_4.total(), some(eq(5)));
    let expected_ids_4 = (246..=250).map(|n| namespace.urn("Stop", format!("{n:04}"))).collect::<Result<Vec<_>, _>>()?;
    expect_that!(page_4.items(), ids_in_order(&expected_ids_4));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&stop_type)
        .q(q::attr("servedBy").linked("headSign").eq("north").or(q::attr("seq").eq(1)))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .limit(5)
        .count(ResultsCount::Include)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_5.total(), some(eq(41)));
    expect_that!(page_5.items(), len(eq(5)));
    expect_that!(page_5.items(), nth(0, has_id(&stop_id(1)?)));
    expect_that!(page_5.items(), nth(1, has_id(&stop_id(201)?)));
    expect_that!(page_5.items(), nth(2, has_id(&stop_id(202)?)));
    expect_that!(page_5.items(), nth(3, has_id(&stop_id(203)?)));
    expect_that!(page_5.items(), nth(4, has_id(&stop_id(204)?)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&stop_type)
        .q(q::attr("servedBy").linked("headSign").eq("east"))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .limit(10)
        .count(ResultsCount::Include)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_6.total(), some(eq(0)));
    expect_that!(page_6.items(), len(eq(0)));
    Ok(())
}
