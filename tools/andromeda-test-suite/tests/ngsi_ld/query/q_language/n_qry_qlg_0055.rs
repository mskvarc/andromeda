//! A bare existence past the first two hundred entities pages in identifier order without the expired.

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
    model::{Entity, Input, NgsiLdMediaType, Property},
    query::{Locality, q},
};

/// Pins: Query Entities with the bare existence of an Attribute over 300 Entities, of which the last 30 hold it and
/// ten after the first 200 hold it only past its `expiresAt`, pages the 30 in identifier order with a total of 30,
/// alone, beside a comparison and inside a disjunction
/// Clause: 4.9, 4.22, 5.5.9, 6.3.13
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0055_a_bare_existence_past_the_first_two_hundred_entities_pages_in_identifier_order_without_the_expired() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_0001_id = namespace.urn("Vehicle", "0001")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_0271_id = namespace.urn("Vehicle", "0271")?;
    let vehicle_0272_id = namespace.urn("Vehicle", "0272")?;
    let vehicle_0273_id = namespace.urn("Vehicle", "0273")?;
    let vehicle_0274_id = namespace.urn("Vehicle", "0274")?;
    let entities = (1..=200)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", format!("{n:04}"))?)
                .entity_type(vehicle_type.clone())
                .property(format!("fuel{namespace}").as_str(), n)
                .build()?)
        })
        .chain((201..=210).map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", format!("{n:04}"))?)
                .entity_type(vehicle_type.clone())
                .property(format!("fuel{namespace}").as_str(), n)
                .property(
                    format!("speed{namespace}").as_str(),
                    Property::builder(80).expires_at("2020-01-01T00:00:00Z".parse()?),
                )
                .build()?)
        }))
        .chain((211..=270).map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", format!("{n:04}"))?)
                .entity_type(vehicle_type.clone())
                .property(format!("fuel{namespace}").as_str(), n)
                .build()?)
        }))
        .chain((271..=300).map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", format!("{n:04}"))?)
                .entity_type(vehicle_type.clone())
                .property(format!("fuel{namespace}").as_str(), n)
                .property(format!("speed{namespace}").as_str(), 80)
                .build()?)
        }))
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .q(q::attr(format!("speed{namespace}").as_str()).exists())
        .limit(10)
        .count(ResultsCount::Include)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(30)));
    let expected_ids = (271..=280)
        .map(|n| namespace.urn("Vehicle", format!("{n:04}")))
        .collect::<Result<Vec<_>, _>>()?;
    expect_that!(page.items(), ids_in_order(&expected_ids));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .q(q::attr(format!("speed{namespace}").as_str()).exists())
        .limit(20)
        .offset(20)
        .locality(Locality::Local)
        .send()
        .await?;
    let expected_ids_2 = (291..=300)
        .map(|n| namespace.urn("Vehicle", format!("{n:04}")))
        .collect::<Result<Vec<_>, _>>()?;
    expect_that!(page_2.items(), ids_in_order(&expected_ids_2));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .q(q::attr(format!("fuel{namespace}").as_str()).exists())
        .limit(10)
        .count(ResultsCount::Include)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_3.total(), some(eq(300)));
    let expected_ids_3 = (1..=10).map(|n| namespace.urn("Vehicle", format!("{n:04}"))).collect::<Result<Vec<_>, _>>()?;
    expect_that!(page_3.items(), ids_in_order(&expected_ids_3));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .q(q::attr(format!("speed{namespace}").as_str())
            .exists()
            .and(q::attr(format!("fuel{namespace}").as_str()).gt(290)))
        .limit(20)
        .count(ResultsCount::Include)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_4.total(), some(eq(10)));
    let expected_ids_4 = (291..=300)
        .map(|n| namespace.urn("Vehicle", format!("{n:04}")))
        .collect::<Result<Vec<_>, _>>()?;
    expect_that!(page_4.items(), ids_in_order(&expected_ids_4));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .q(q::attr(format!("speed{namespace}").as_str())
            .exists()
            .or(q::attr(format!("fuel{namespace}").as_str()).eq(1)))
        .limit(5)
        .count(ResultsCount::Include)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_5.total(), some(eq(31)));
    expect_that!(page_5.items(), len(eq(5)));
    expect_that!(page_5.items(), nth(0, has_id(&vehicle_0001_id)));
    expect_that!(page_5.items(), nth(1, has_id(&vehicle_0271_id)));
    expect_that!(page_5.items(), nth(2, has_id(&vehicle_0272_id)));
    expect_that!(page_5.items(), nth(3, has_id(&vehicle_0273_id)));
    expect_that!(page_5.items(), nth(4, has_id(&vehicle_0274_id)));
    Ok(())
}
