//! Date time and date orderings past the first two hundred entities page in identifier order.

use andromeda_test_suite::{broker, identity_matcher::ids_in_order, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Date, DateTime, Entity, Input, NgsiLdMediaType, Value},
    query::{Locality, q},
};

/// Pins: Query Entities with a `DateTime` ordering, a `DateTime` range and a Date ordering over 300 Entities whose
/// matches all sort after the first 270 pages them in identifier order with their total, the range including both
/// bounds
/// Clause: 4.9, 5.5.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0054_date_time_and_date_orderings_past_the_first_two_hundred_entities_page_in_identifier_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let shift_type = namespace.entity_type("Shift")?;
    let entities = (1..=270)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Shift", format!("{n:04}"))?)
                .entity_type(shift_type.clone())
                .property(
                    format!("startsAt{namespace}").as_str(),
                    Value::from("2026-01-01T08:00:00Z".parse::<DateTime>()?),
                )
                .property(format!("onDay{namespace}").as_str(), Value::from("2026-06-15".parse::<Date>()?))
                .build()?)
        })
        .chain((271..=300).map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Shift", format!("{n:04}"))?)
                .entity_type(shift_type.clone())
                .property(
                    format!("startsAt{namespace}").as_str(),
                    Value::from(format!("2026-03-{:02}T00:00:00Z", n - 270).parse::<DateTime>()?),
                )
                .property(
                    format!("onDay{namespace}").as_str(),
                    Value::from(format!("2025-11-{:02}", n - 270).parse::<Date>()?),
                )
                .build()?)
        }))
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&shift_type)
        .q(q::attr(format!("startsAt{namespace}").as_str()).gt("2026-02-01T00:00:00Z".parse::<DateTime>()?))
        .limit(20)
        .count(ResultsCount::Include)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(30)));
    let expected_ids = (271..=290).map(|n| namespace.urn("Shift", format!("{n:04}"))).collect::<Result<Vec<_>, _>>()?;
    expect_that!(page.items(), ids_in_order(&expected_ids));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&shift_type)
        .q(q::attr(format!("startsAt{namespace}").as_str()).gt("2026-02-01T00:00:00Z".parse::<DateTime>()?))
        .limit(20)
        .offset(20)
        .locality(Locality::Local)
        .send()
        .await?;
    let expected_ids_2 = (291..=300).map(|n| namespace.urn("Shift", format!("{n:04}"))).collect::<Result<Vec<_>, _>>()?;
    expect_that!(page_2.items(), ids_in_order(&expected_ids_2));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&shift_type)
        .q(q::attr(format!("startsAt{namespace}").as_str()).within("2026-03-01T00:00:00Z".parse::<DateTime>()?, "2026-03-10T00:00:00Z".parse::<DateTime>()?))
        .limit(20)
        .count(ResultsCount::Include)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_3.total(), some(eq(10)));
    let expected_ids_3 = (271..=280).map(|n| namespace.urn("Shift", format!("{n:04}"))).collect::<Result<Vec<_>, _>>()?;
    expect_that!(page_3.items(), ids_in_order(&expected_ids_3));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&shift_type)
        .q(q::attr(format!("onDay{namespace}").as_str()).lt("2026-01-01".parse::<Date>()?))
        .limit(50)
        .count(ResultsCount::Include)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_4.total(), some(eq(30)));
    let expected_ids_4 = (271..=300).map(|n| namespace.urn("Shift", format!("{n:04}"))).collect::<Result<Vec<_>, _>>()?;
    expect_that!(page_4.items(), ids_in_order(&expected_ids_4));
    Ok(())
}
