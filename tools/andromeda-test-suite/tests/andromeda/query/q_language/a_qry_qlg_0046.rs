//! A word class selects non ascii words.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::q,
};

/// Pins: q name~="^\w+$" selects every one-word spelling, the carons included, q name~="^\w+\s\w+$" selects only
/// the two-word place name, and q name!~="^\w+$" selects only the place name
/// Clause: 4.9, 5.7.2.4
/// Recorded: clause 4.9 hands `RegExp` to IEEE 1003.2, which has no inline options and no shorthand classes;
/// Andromeda admits the case-insensitive option and the shorthand classes \d, \w and \s as extensions, folding case
/// by Unicode simple case folding and classifying by Unicode
#[gtest]
#[tokio::test]
async fn a_qry_qlg_0046_a_word_class_selects_non_ascii_words() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let bee_0_id = namespace.urn("Bee", "0")?;
    let bee_type = namespace.entity_type("Bee")?;
    let bee_id = namespace.urn("Bee", 1)?;
    let bee_2_id = namespace.urn("Bee", 2)?;
    let bee_3_id = namespace.urn("Bee", 3)?;
    let bee_4_id = namespace.urn("Bee", 4)?;
    let bee_5_id = namespace.urn("Bee", 5)?;
    let entities = [
        Entity::builder()
            .id(bee_0_id.clone())
            .entity_type(bee_type.clone())
            .property("name", "čebela")
            .build()?,
        Entity::builder()
            .id(bee_id.clone())
            .entity_type(bee_type.clone())
            .property("name", "Čebela")
            .build()?,
        Entity::builder()
            .id(bee_2_id.clone())
            .entity_type(bee_type.clone())
            .property("name", "ČEBELA")
            .build()?,
        Entity::builder()
            .id(bee_3_id.clone())
            .entity_type(bee_type.clone())
            .property("name", "ČEbela")
            .build()?,
        Entity::builder()
            .id(bee_4_id.clone())
            .entity_type(bee_type.clone())
            .property("name", "cebela")
            .build()?,
        Entity::builder()
            .id(bee_5_id.clone())
            .entity_type(bee_type.clone())
            .property("name", "Škofja Loka")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&bee_type)
        .q(q::attr("name").matches("\"^\\w+$\""))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(5)));
    expect_that!(page.items(), contains(has_id(&bee_0_id)));
    expect_that!(page.items(), contains(has_id(&bee_id)));
    expect_that!(page.items(), contains(has_id(&bee_2_id)));
    expect_that!(page.items(), contains(has_id(&bee_3_id)));
    expect_that!(page.items(), contains(has_id(&bee_4_id)));
    expect_that!(page.items(), not(contains(has_id(&bee_5_id))));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&bee_type)
        .q(q::attr("name").matches("\"^\\w+\\s\\w+$\""))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&bee_5_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&bee_type)
        .q(q::attr("name").not_matches("\"^\\w+$\""))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&bee_5_id)));
    Ok(())
}
