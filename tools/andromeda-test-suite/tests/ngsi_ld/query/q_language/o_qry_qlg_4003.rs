//! A post query string ordering follows the unicode collation algorithm.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, NgsiLdMediaType},
    query::q,
};

/// Pins: Query Entities by POST with q name>"c" and with q name=="b".."d" compares strings by the Unicode Collation
/// Algorithm, as the GET parameter does
/// Clause: 4.9, 5.2.23, 5.7.2.4, 6.23.3.1
/// Recorded: clause 4.9 says implementations "should support the Unicode Collation Algorithm (UCA)" for comparing text
/// strings; a "should", so code point order also conforms. Under UCA the order is alpha, Bravo, charlie, Delta, elan;
/// under code point order it is Bravo, Delta, alpha, charlie, elan
#[gtest]
#[tokio::test]
async fn o_qry_qlg_4003_a_post_query_string_ordering_follows_the_unicode_collation_algorithm() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let word_alpha_id = namespace.urn("Word", "alpha")?;
    let word_type = namespace.entity_type("Word")?;
    let word_bravo_id = namespace.urn("Word", "bravo")?;
    let word_charlie_id = namespace.urn("Word", "charlie")?;
    let word_delta_id = namespace.urn("Word", "delta")?;
    let entities = [
        Entity::builder()
            .id(word_alpha_id)
            .entity_type(word_type.clone())
            .property("name", "alpha")
            .build()?,
        Entity::builder()
            .id(word_bravo_id.clone())
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
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&word_type)
        .q(q::attr("name").gt("c"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&word_charlie_id)));
    expect_that!(page.items(), contains(has_id(&word_delta_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&word_type)
        .q(q::attr("name").within("b", "d"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&word_bravo_id)));
    expect_that!(page_2.items(), contains(has_id(&word_charlie_id)));
    Ok(())
}
