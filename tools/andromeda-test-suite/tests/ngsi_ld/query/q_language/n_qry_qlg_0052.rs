//! A pattern without a language reads every language string past the first two hundred entities.

use andromeda_test_suite::{broker, identity_matcher::ids_in_order, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, Input, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::q,
};
use std::iter;

/// Pins: Query Entities with q label~="^tuer-02`[5-9]`" over 300 entities matches the German string of a
/// `LanguageProperty`, a plain String and a String past 3,000 characters alike, all sorting after the first 200, in
/// identifier order with their total
/// Clause: 4.9, 5.5.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0052_a_pattern_without_a_language_reads_every_language_string_past_the_first_two_hundred_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sign_type = namespace.entity_type("Sign")?;
    let sign_0260_id = namespace.urn("Sign", "0260")?;
    let sign_0270_id = namespace.urn("Sign", "0270")?;
    let entities = (1..=259)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Sign", format!("{n:04}"))?)
                .entity_type(sign_type.clone())
                .language_property(
                    "label",
                    LanguageProperty::builder(LanguageMap::try_from_pairs([("en", format!("door-{n:04}")), ("de", format!("tuer-{n:04}"))])?),
                )
                .build()?)
        })
        .chain(iter::once(Ok(Entity::builder()
            .id(sign_0260_id)
            .entity_type(sign_type.clone())
            .property("label", format!("tuer-0260 {}", "x".repeat(3000)).as_str())
            .build()?)))
        .chain((261..=269).map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Sign", format!("{n:04}"))?)
                .entity_type(sign_type.clone())
                .language_property(
                    "label",
                    LanguageProperty::builder(LanguageMap::try_from_pairs([("en", format!("door-{n:04}")), ("de", format!("tuer-{n:04}"))])?),
                )
                .build()?)
        }))
        .chain(iter::once(Ok(Entity::builder()
            .id(sign_0270_id)
            .entity_type(sign_type.clone())
            .property("label", "tuer-0270")
            .build()?)))
        .chain((271..=300).map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Sign", format!("{n:04}"))?)
                .entity_type(sign_type.clone())
                .language_property(
                    "label",
                    LanguageProperty::builder(LanguageMap::try_from_pairs([("en", format!("door-{n:04}")), ("de", format!("tuer-{n:04}"))])?),
                )
                .build()?)
        }))
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sign_type)
        .q(q::attr("label").matches("\"^tuer-02[5-9]\""))
        .limit(100)
        .offset(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(50)));
    let expected_ids = (250..=299).map(|n| namespace.urn("Sign", format!("{n:04}"))).collect::<Result<Vec<_>, _>>()?;
    expect_that!(page.items(), ids_in_order(&expected_ids));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&sign_type)
        .q(q::attr("label").matches("\"^door-029\""))
        .limit(100)
        .offset(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(10)));
    let expected_ids_2 = (290..=299).map(|n| namespace.urn("Sign", format!("{n:04}"))).collect::<Result<Vec<_>, _>>()?;
    expect_that!(page_2.items(), ids_in_order(&expected_ids_2));
    Ok(())
}
