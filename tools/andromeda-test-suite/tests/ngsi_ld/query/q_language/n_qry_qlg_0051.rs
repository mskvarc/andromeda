//! A broad pattern pages in identifier order whether the first entities match or not.

use andromeda_test_suite::{broker, identity_matcher::ids_in_order, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, Input, NgsiLdMediaType},
    query::q,
};

/// Pins: Query Entities with q label~="tail-.*" over 1,500 entities, the first 200 of which hold head- labels,
/// answers the 1,300 tail matches in identifier order, page by page; a pattern every entity matches answers the
/// first hundred
/// Clause: 4.9, 5.5.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0051_a_broad_pattern_pages_in_identifier_order_whether_the_first_entities_match_or_not() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sign_type = namespace.entity_type("Sign")?;
    let entities = (1..=200)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Sign", format!("{n:04}"))?)
                .entity_type(sign_type.clone())
                .property("label", format!("head-{n:04}"))
                .build()?)
        })
        .chain((201..=1500).map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Sign", format!("{n:04}"))?)
                .entity_type(sign_type.clone())
                .property("label", format!("tail-{n:04}"))
                .build()?)
        }))
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sign_type)
        .q(q::attr("label").matches("\"tail-.*\""))
        .limit(100)
        .offset(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(1300)));
    let expected_ids = (201..=300).map(|n| namespace.urn("Sign", format!("{n:04}"))).collect::<Result<Vec<_>, _>>()?;
    expect_that!(page.items(), ids_in_order(&expected_ids));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sign_type)
        .q(q::attr("label").matches("\"tail-.*\""))
        .limit(100)
        .offset(1200)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(1300)));
    let expected_ids_2 = (1401..=1500).map(|n| namespace.urn("Sign", format!("{n:04}"))).collect::<Result<Vec<_>, _>>()?;
    expect_that!(page_2.items(), ids_in_order(&expected_ids_2));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sign_type)
        .q(q::attr("label").matches("\"-[0-9]{4}$\""))
        .limit(100)
        .offset(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_3.total(), some(eq(1500)));
    let expected_ids_3 = (1..=100).map(|n| namespace.urn("Sign", format!("{n:04}"))).collect::<Result<Vec<_>, _>>()?;
    expect_that!(page_3.items(), ids_in_order(&expected_ids_3));
    Ok(())
}
