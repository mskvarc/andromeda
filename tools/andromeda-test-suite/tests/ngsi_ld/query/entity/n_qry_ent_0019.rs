//! An id pattern open at the start beside a type pages its late matches in identifier order.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, Input, NgsiLdMediaType},
    query::IdPattern,
};
use std::ops::RangeInclusive;

/// The Meters whose identifiers the patterns match, all past the first two hundred.
const TAGGED_METERS: RangeInclusive<u32> = 201..=220;

/// Pins: Query Entities by type with an `idPattern` open at the identifier's start, over 300 Entities of the type
/// whose 20 matches sort after the first 200 beside 10 matching Entities of another type, pages the 20 in
/// identifier order with a total of 20, also past the page the first matches fill, case-insensitively, anchored and
/// with no match
/// Clause: 4.17, 5.5.9, 5.7.2.4, 6.3.13
#[gtest]
#[tokio::test]
async fn n_qry_ent_0019_an_id_pattern_open_at_the_start_beside_a_type_pages_its_late_matches_in_identifier_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let meter_type = namespace.entity_type("Meter")?;
    let valve_type = namespace.entity_type("Valve")?;
    let tagged_meter_id = |number: u32| namespace.urn("Meter", format!("{number:04}-tagged"));
    let meters = (1..=300).map(|number| -> Result<Entity<Input>> {
        let id = if TAGGED_METERS.contains(&number) {
            tagged_meter_id(number)?
        } else {
            namespace.urn("Meter", format!("{number:04}"))?
        };
        Ok(Entity::builder().id(id).entity_type(meter_type.clone()).build()?)
    });
    let valves = (1..=10).map(|number: u32| -> Result<Entity<Input>> {
        Ok(Entity::builder()
            .id(namespace.urn("Valve", format!("{number:04}-tagged"))?)
            .entity_type(valve_type.clone())
            .build()?)
    });
    let entities = meters.chain(valves).collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities).send().await?.done()?;
    // The first ten matches, and the total of the type's matches alone
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&meter_type)
        .id_pattern(format!(".*{namespace}-.*-tagged$").parse::<IdPattern>()?)
        .limit(10)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(20)));
    expect_that!(page.items(), len(eq(10)));
    for (position, number) in (201..=210).enumerate() {
        expect_that!(page.items(), nth(position, has_id(&tagged_meter_id(number)?)));
    }
    // A page past every match the first Entities hold
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&meter_type)
        .id_pattern(format!(".*{namespace}-.*-tagged$").parse::<IdPattern>()?)
        .limit(10)
        .offset(15)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(20)));
    expect_that!(page_2.items(), len(eq(5)));
    for (position, number) in (216..=220).enumerate() {
        expect_that!(page_2.items(), nth(position, has_id(&tagged_meter_id(number)?)));
    }
    // A case-insensitive pattern matches the same identifiers
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&meter_type)
        .id_pattern(format!("(?i).*{namespace}-.*-TAGGED$").parse::<IdPattern>()?)
        .limit(10)
        .offset(15)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_3.total(), some(eq(20)));
    expect_that!(page_3.items(), len(eq(5)));
    for (position, number) in (216..=220).enumerate() {
        expect_that!(page_3.items(), nth(position, has_id(&tagged_meter_id(number)?)));
    }
    // A pattern anchored at the identifier's start selects the same matches
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&meter_type)
        .id_pattern(format!("^urn:ngsi-ld:Meter:{namespace}-02.*-tagged$").parse::<IdPattern>()?)
        .limit(5)
        .offset(10)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_4.total(), some(eq(20)));
    expect_that!(page_4.items(), len(eq(5)));
    for (position, number) in (211..=215).enumerate() {
        expect_that!(page_4.items(), nth(position, has_id(&tagged_meter_id(number)?)));
    }
    // A pattern no identifier matches selects nothing
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&meter_type)
        .id_pattern(format!(".*{namespace}-.*-absent$").parse::<IdPattern>()?)
        .limit(10)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_5.total(), some(eq(0)));
    expect_that!(page_5.items(), len(eq(0)));
    Ok(())
}
