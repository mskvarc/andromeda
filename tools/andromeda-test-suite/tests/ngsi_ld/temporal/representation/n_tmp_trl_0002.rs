//! A vocab property reads back normalized and simplified.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Upserted,
    simplified_pairs::vocab_pairs,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{AttributeKind, DateTime, NgsiLdMediaType, OneOrMany, VocabProperty, VocabTerm, evolution::TemporalEntity},
};

/// Pins: a Temporal Evolution created with two `VocabProperty` instances answers 201 and reads back normalized as
/// `VocabProperty` instances holding their vocab, and simplified as a `vocabs` member of `[{"vocab": ...}, observedAt]` pairs
/// Clause: 5.6.11.4, 4.5.7, 4.5.9, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_trl_0002_a_vocab_property_reads_back_normalized_and_simplified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let sedan = "sedan".parse::<VocabTerm>()?;
    let sports = "sports".parse::<VocabTerm>()?;
    let noon = "2020-08-01T12:00:00Z".parse::<DateTime>()?;
    let one = "2020-08-01T13:00:00Z".parse::<DateTime>()?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .vocab_property("category", VocabProperty::builder(sedan.clone()).observed_at(noon))
        .vocab_property("category", VocabProperty::builder(sports.clone()).observed_at(one))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "category",
            all!(
                len(eq(2)),
                contains(attribute_of(AttributeKind::VocabProperty).vocab(sedan.clone()).observed_at(noon)),
                contains(attribute_of(AttributeKind::VocabProperty).vocab(sports.clone()).observed_at(one))
            )
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_values(&vehicle_id)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    let values = retrieved_2.body();
    let pairs = vocab_pairs(values, "category").or_fail()?;
    expect_that!(pairs, len(eq(2)));
    let first = pairs.iter().find(|pair| pair.value == OneOrMany::one(sedan.clone())).or_fail()?;
    expect_that!(first.time.timestamp(), eq(noon.timestamp()));
    let second = pairs.iter().find(|pair| pair.value == OneOrMany::one(sports.clone())).or_fail()?;
    expect_that!(second.time.timestamp(), eq(one.timestamp()));
    Ok(())
}
