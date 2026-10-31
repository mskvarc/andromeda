//! An untyped string in a time value form is served back as written.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    simplified_matcher::has_value,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Retrieve Entity answers an untyped string in the Time, `DateTime` or Date form, an offset-bearing one and
/// free text byte for byte as written, normalized and `keyValues`
/// Clause: 4.6.3, 4.6.4, 5.7.1, C.6
#[gtest]
#[tokio::test]
async fn n_ent_ret_9032_an_untyped_string_in_a_time_value_form_is_served_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let observation_stored_spelling_id = namespace.urn("Observation", "stored-spelling")?;
    let observation_type = namespace.entity_type("Observation")?;
    let observation_stored_spelling = Entity::builder()
        .id(observation_stored_spelling_id.clone())
        .entity_type(observation_type)
        .property("readingTime", "12:00:00.500000Z")
        .property("readingInstant", "2024-01-01T00:00:00.250000Z")
        .property("readingDate", "2024-06-15")
        .property("readingLabel", "2024-01-01T01:00:00+01:00")
        .property("readingNote", "12:00:00 in the morning")
        .build()?;
    broker.entities().create(&observation_stored_spelling).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&observation_stored_spelling_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("readingTime", any_attribute().value("12:00:00.500000Z")));
    expect_that!(entity, has_attribute("readingInstant", any_attribute().value("2024-01-01T00:00:00.250000Z")));
    expect_that!(entity, has_attribute("readingDate", any_attribute().value("2024-06-15")));
    expect_that!(entity, has_attribute("readingLabel", any_attribute().value("2024-01-01T01:00:00+01:00")));
    expect_that!(entity, has_attribute("readingNote", any_attribute().value("12:00:00 in the morning")));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&observation_stored_spelling_id)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_value("readingTime", "12:00:00.500000Z"));
    expect_that!(entity_2, has_value("readingInstant", "2024-01-01T00:00:00.250000Z"));
    expect_that!(entity_2, has_value("readingDate", "2024-06-15"));
    Ok(())
}
