//! String ordering operators follow the unicode collation algorithm.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::q,
};

/// Pins: q name>"c", name<"b", name>="charlie" and name<="alpha" compare strings by the Unicode Collation
/// Algorithm, so Bravo and an accented elan sort among the lower-case words and not before and after all of them
/// Clause: 4.9, 5.7.2.4
/// Recorded: clause 4.9 says implementations "should support the Unicode Collation Algorithm (UCA)" for comparing text
/// strings; a "should", so code point order also conforms. Under UCA the order is alpha, Bravo, charlie, Delta, elan;
/// under code point order it is Bravo, Delta, alpha, charlie, elan
#[gtest]
#[tokio::test]
async fn o_qry_qlg_4001_string_ordering_operators_follow_the_unicode_collation_algorithm() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let word_alpha_id = namespace.urn("Word", "alpha")?;
    let word_type = namespace.entity_type("Word")?;
    let word_bravo_id = namespace.urn("Word", "bravo")?;
    let word_charlie_id = namespace.urn("Word", "charlie")?;
    let word_delta_id = namespace.urn("Word", "delta")?;
    let word_elan_id = namespace.urn("Word", "elan")?;
    let entities = [
        Entity::builder()
            .id(word_alpha_id.clone())
            .entity_type(word_type.clone())
            .property("name", "alpha")
            .build()?,
        Entity::builder()
            .id(word_bravo_id)
            .entity_type(word_type.clone())
            .property("name", "Bravo")
            .build()?,
        Entity::builder()
            .id(word_charlie_id.clone())
            .entity_type(word_type.clone())
            .property("name", "charlie")
            .build()?,
        Entity::builder()
            .id(word_delta_id.clone())
            .entity_type(word_type.clone())
            .property("name", "Delta")
            .build()?,
        Entity::builder()
            .id(word_elan_id.clone())
            .entity_type(word_type.clone())
            .property("name", "élan")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // Greater than: UCA puts charlie below Delta and Delta below elan.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&word_type)
        .q(q::attr("name").gt("c"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), contains(has_id(&word_charlie_id)));
    expect_that!(page.items(), contains(has_id(&word_delta_id)));
    expect_that!(page.items(), contains(has_id(&word_elan_id)));
    // Less than: the one-letter word b is a prefix of Bravo, so alpha is the only word below it.
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&word_type)
        .q(q::attr("name").lt("b"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&word_alpha_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&word_type)
        .q(q::attr("name").ge("charlie"))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(3)));
    expect_that!(page_3.items(), contains(has_id(&word_charlie_id)));
    expect_that!(page_3.items(), contains(has_id(&word_delta_id)));
    expect_that!(page_3.items(), contains(has_id(&word_elan_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&word_type)
        .q(q::attr("name").le("alpha"))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&word_alpha_id)));
    Ok(())
}
