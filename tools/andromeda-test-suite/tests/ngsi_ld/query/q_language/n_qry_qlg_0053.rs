//! A negated pattern over more than two hundred entities needs the attribute.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, Input, NgsiLdMediaType},
    query::q,
};
use std::iter;

/// Pins: Query Entities with q label!~="^drop" over 250 entities answers the 230 whose label does not match, in
/// identifier order, and never the ten without a label
/// Clause: 4.9, 5.5.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0053_a_negated_pattern_over_more_than_two_hundred_entities_needs_the_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sign_type = namespace.entity_type("Sign")?;
    let sign_0240_id = namespace.urn("Sign", "0240")?;
    let sign_0201_id = namespace.urn("Sign", "0201")?;
    let sign_0202_id = namespace.urn("Sign", "0202")?;
    let sign_0203_id = namespace.urn("Sign", "0203")?;
    let sign_0204_id = namespace.urn("Sign", "0204")?;
    let sign_0205_id = namespace.urn("Sign", "0205")?;
    let sign_0206_id = namespace.urn("Sign", "0206")?;
    let sign_0207_id = namespace.urn("Sign", "0207")?;
    let sign_0208_id = namespace.urn("Sign", "0208")?;
    let sign_0209_id = namespace.urn("Sign", "0209")?;
    let sign_0210_id = namespace.urn("Sign", "0210")?;
    let sign_0211_id = namespace.urn("Sign", "0211")?;
    let sign_0212_id = namespace.urn("Sign", "0212")?;
    let sign_0213_id = namespace.urn("Sign", "0213")?;
    let sign_0214_id = namespace.urn("Sign", "0214")?;
    let sign_0215_id = namespace.urn("Sign", "0215")?;
    let sign_0216_id = namespace.urn("Sign", "0216")?;
    let sign_0217_id = namespace.urn("Sign", "0217")?;
    let sign_0218_id = namespace.urn("Sign", "0218")?;
    let sign_0219_id = namespace.urn("Sign", "0219")?;
    let sign_0220_id = namespace.urn("Sign", "0220")?;
    let sign_0221_id = namespace.urn("Sign", "0221")?;
    let sign_0222_id = namespace.urn("Sign", "0222")?;
    let sign_0223_id = namespace.urn("Sign", "0223")?;
    let sign_0224_id = namespace.urn("Sign", "0224")?;
    let sign_0225_id = namespace.urn("Sign", "0225")?;
    let sign_0226_id = namespace.urn("Sign", "0226")?;
    let sign_0227_id = namespace.urn("Sign", "0227")?;
    let sign_0228_id = namespace.urn("Sign", "0228")?;
    let sign_0229_id = namespace.urn("Sign", "0229")?;
    let entities = (1..=229)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Sign", format!("{n:04}"))?)
                .entity_type(sign_type.clone())
                .property("label", format!("keep-{n:04}"))
                .build()?)
        })
        .chain((230..=239).map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Sign", format!("{n:04}"))?)
                .entity_type(sign_type.clone())
                .property("label", format!("drop-{n:04}"))
                .build()?)
        }))
        .chain(iter::once(Ok(Entity::builder()
            .id(sign_0240_id.clone())
            .entity_type(sign_type.clone())
            .property("label", "keep-0240")
            .build()?)))
        .chain((241..=250).map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Sign", format!("{n:04}"))?)
                .entity_type(sign_type.clone())
                .property("height", n)
                .build()?)
        }))
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sign_type)
        .q(q::attr("label").not_matches("\"^drop\""))
        .limit(100)
        .offset(200)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(230)));
    expect_that!(page.items(), len(eq(30)));
    expect_that!(page.items(), nth(0, has_id(&sign_0201_id)));
    expect_that!(page.items(), nth(1, has_id(&sign_0202_id)));
    expect_that!(page.items(), nth(2, has_id(&sign_0203_id)));
    expect_that!(page.items(), nth(3, has_id(&sign_0204_id)));
    expect_that!(page.items(), nth(4, has_id(&sign_0205_id)));
    expect_that!(page.items(), nth(5, has_id(&sign_0206_id)));
    expect_that!(page.items(), nth(6, has_id(&sign_0207_id)));
    expect_that!(page.items(), nth(7, has_id(&sign_0208_id)));
    expect_that!(page.items(), nth(8, has_id(&sign_0209_id)));
    expect_that!(page.items(), nth(9, has_id(&sign_0210_id)));
    expect_that!(page.items(), nth(10, has_id(&sign_0211_id)));
    expect_that!(page.items(), nth(11, has_id(&sign_0212_id)));
    expect_that!(page.items(), nth(12, has_id(&sign_0213_id)));
    expect_that!(page.items(), nth(13, has_id(&sign_0214_id)));
    expect_that!(page.items(), nth(14, has_id(&sign_0215_id)));
    expect_that!(page.items(), nth(15, has_id(&sign_0216_id)));
    expect_that!(page.items(), nth(16, has_id(&sign_0217_id)));
    expect_that!(page.items(), nth(17, has_id(&sign_0218_id)));
    expect_that!(page.items(), nth(18, has_id(&sign_0219_id)));
    expect_that!(page.items(), nth(19, has_id(&sign_0220_id)));
    expect_that!(page.items(), nth(20, has_id(&sign_0221_id)));
    expect_that!(page.items(), nth(21, has_id(&sign_0222_id)));
    expect_that!(page.items(), nth(22, has_id(&sign_0223_id)));
    expect_that!(page.items(), nth(23, has_id(&sign_0224_id)));
    expect_that!(page.items(), nth(24, has_id(&sign_0225_id)));
    expect_that!(page.items(), nth(25, has_id(&sign_0226_id)));
    expect_that!(page.items(), nth(26, has_id(&sign_0227_id)));
    expect_that!(page.items(), nth(27, has_id(&sign_0228_id)));
    expect_that!(page.items(), nth(28, has_id(&sign_0229_id)));
    expect_that!(page.items(), nth(29, has_id(&sign_0240_id)));
    Ok(())
}
