//! A case insensitive pattern selects every case form of a non ascii word.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::q,
};

/// Pins: q name~="(?i)^čebela$" selects the four case forms of čebela, the caron-bearing capital folded with the
/// ASCII letters, and neither the word without its caron nor another word
/// Clause: 4.9, 5.7.2.4
/// Recorded: clause 4.9 hands `RegExp` to IEEE 1003.2, which has no inline options and no shorthand classes;
/// Andromeda admits the case-insensitive option and the shorthand classes \d, \w and \s as extensions, folding case
/// by Unicode simple case folding and classifying by Unicode
#[gtest]
#[tokio::test]
async fn a_qry_qlg_0041_a_case_insensitive_pattern_selects_every_case_form_of_a_non_ascii_word() -> Result<()> {
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
        .q(q::attr("name").matches("\"(?i)^čebela$\""))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(4)));
    expect_that!(page.items(), contains(has_id(&bee_0_id)));
    expect_that!(page.items(), contains(has_id(&bee_id)));
    expect_that!(page.items(), contains(has_id(&bee_2_id)));
    expect_that!(page.items(), contains(has_id(&bee_3_id)));
    expect_that!(page.items(), not(contains(has_id(&bee_4_id))));
    expect_that!(page.items(), not(contains(has_id(&bee_5_id))));
    Ok(())
}
