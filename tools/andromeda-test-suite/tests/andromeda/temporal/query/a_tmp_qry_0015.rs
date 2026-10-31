//! A word class q pattern selects non ascii words over the window.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
    query::{q, temporal_query},
};

/// Pins: Query Temporal Evolution with q name~="^\w+$" selects every one-word spelling, the carons included, and
/// not the two-word place name
/// Clause: 4.9, 5.7.4.4
/// Recorded: clause 4.9 hands `RegExp` to IEEE 1003.2, which has no inline options and no shorthand classes;
/// Andromeda admits the case-insensitive option and the shorthand classes \d, \w and \s as extensions, folding case
/// by Unicode simple case folding and classifying by Unicode
#[gtest]
#[tokio::test]
async fn a_tmp_qry_0015_a_word_class_q_pattern_selects_non_ascii_words_over_the_window() -> Result<()> {
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
            .property("name", Property::builder("čebela").observed_at("2024-05-01T08:00:00Z".parse()?))
            .build()?,
        Entity::builder()
            .id(bee_id.clone())
            .entity_type(bee_type.clone())
            .property("name", Property::builder("Čebela").observed_at("2024-05-01T08:00:00Z".parse()?))
            .build()?,
        Entity::builder()
            .id(bee_2_id.clone())
            .entity_type(bee_type.clone())
            .property("name", Property::builder("ČEBELA").observed_at("2024-05-01T08:00:00Z".parse()?))
            .build()?,
        Entity::builder()
            .id(bee_3_id.clone())
            .entity_type(bee_type.clone())
            .property("name", Property::builder("ČEbela").observed_at("2024-05-01T08:00:00Z".parse()?))
            .build()?,
        Entity::builder()
            .id(bee_4_id.clone())
            .entity_type(bee_type.clone())
            .property("name", Property::builder("cebela").observed_at("2024-05-01T08:00:00Z".parse()?))
            .build()?,
        Entity::builder()
            .id(bee_5_id.clone())
            .entity_type(bee_type.clone())
            .property("name", Property::builder("Škofja Loka").observed_at("2024-05-01T08:00:00Z".parse()?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2024-01-01T00:00:00Z".parse()?))
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
    Ok(())
}
