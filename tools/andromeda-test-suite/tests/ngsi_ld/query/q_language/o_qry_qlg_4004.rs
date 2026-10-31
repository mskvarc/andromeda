//! A language string ordering follows the unicode collation algorithm.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::q,
};

/// Pins: q label`[en]`>"c" and label`[en]`=="b".."d" compare the en string of a `LanguageProperty` by the Unicode
/// Collation Algorithm, and an entity that holds no en entry matches neither
/// Clause: 4.9, 5.7.2.4
/// Recorded: clause 4.9 says implementations "should support the Unicode Collation Algorithm (UCA)" for comparing text
/// strings; a "should", so code point order also conforms; the target value of a `LanguageProperty` with a language is
/// the string of that language. Under UCA the order is alpha, Bravo, charlie, Delta; under code point order it is
/// Bravo, Delta, alpha, charlie
#[gtest]
#[tokio::test]
async fn o_qry_qlg_4004_a_language_string_ordering_follows_the_unicode_collation_algorithm() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let word_alpha_id = namespace.urn("Word", "alpha")?;
    let word_type = namespace.entity_type("Word")?;
    let word_bravo_id = namespace.urn("Word", "bravo")?;
    let word_charlie_id = namespace.urn("Word", "charlie")?;
    let word_delta_id = namespace.urn("Word", "delta")?;
    let word_zulu_id = namespace.urn("Word", "zulu")?;
    let entities = [
        Entity::builder()
            .id(word_alpha_id)
            .entity_type(word_type.clone())
            .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "alpha")])?))
            .build()?,
        Entity::builder()
            .id(word_bravo_id.clone())
            .entity_type(word_type.clone())
            .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Bravo")])?))
            .build()?,
        Entity::builder()
            .id(word_charlie_id.clone())
            .entity_type(word_type.clone())
            .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "charlie")])?))
            .build()?,
        Entity::builder()
            .id(word_delta_id.clone())
            .entity_type(word_type.clone())
            .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Delta")])?))
            .build()?,
        Entity::builder()
            .id(word_zulu_id)
            .entity_type(word_type.clone())
            .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "zulu")])?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&word_type)
        .q(q::attr("label").member("en").gt("c"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&word_charlie_id)));
    expect_that!(page.items(), contains(has_id(&word_delta_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&word_type)
        .q(q::attr("label").member("en").within("b", "d"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&word_bravo_id)));
    expect_that!(page_2.items(), contains(has_id(&word_charlie_id)));
    Ok(())
}
