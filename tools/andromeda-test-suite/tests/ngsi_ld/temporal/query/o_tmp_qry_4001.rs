//! A temporal q string ordering follows the unicode collation algorithm.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: a temporal q name>"c" and name=="b".."d" compare the strings of the history by the Unicode Collation
/// Algorithm
/// Clause: 4.9, 5.7.4.4, 4.5.18
/// Recorded: clause 4.9 says implementations "should support the Unicode Collation Algorithm (UCA)" for comparing text
/// strings; a "should", so code point order also conforms. Under UCA the order is alpha, Bravo, charlie, Delta; under
/// code point order it is Bravo, Delta, alpha, charlie
#[gtest]
#[tokio::test]
async fn o_tmp_qry_4001_a_temporal_q_string_ordering_follows_the_unicode_collation_algorithm() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let word_alpha_id = namespace.urn("Word", "alpha")?;
    let word_type = namespace.entity_type("Word")?;
    let word_bravo_id = namespace.urn("Word", "bravo")?;
    let word_charlie_id = namespace.urn("Word", "charlie")?;
    let word_delta_id = namespace.urn("Word", "delta")?;
    let word_alpha_evolution = TemporalEntity::builder()
        .id(word_alpha_id)
        .entity_type(word_type.clone())
        .property("name", Property::builder("alpha").observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&word_alpha_evolution).send().await?.created()?;
    let word_bravo_evolution = TemporalEntity::builder()
        .id(word_bravo_id.clone())
        .entity_type(word_type.clone())
        .property("name", Property::builder("Bravo").observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&word_bravo_evolution).send().await?.created()?;
    let word_charlie_evolution = TemporalEntity::builder()
        .id(word_charlie_id.clone())
        .entity_type(word_type.clone())
        .property("name", Property::builder("charlie").observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&word_charlie_evolution).send().await?.created()?;
    let word_delta_evolution = TemporalEntity::builder()
        .id(word_delta_id.clone())
        .entity_type(word_type.clone())
        .property("name", Property::builder("Delta").observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&word_delta_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-07-01T00:00:00Z".parse()?))
        .entity_type(&word_type)
        .q(q::attr("name").gt("c"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&word_charlie_id)));
    expect_that!(page.items(), contains(has_id(&word_delta_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-07-01T00:00:00Z".parse()?))
        .entity_type(&word_type)
        .q(q::attr("name").within("b", "d"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&word_bravo_id)));
    expect_that!(page_2.items(), contains(has_id(&word_charlie_id)));
    Ok(())
}
