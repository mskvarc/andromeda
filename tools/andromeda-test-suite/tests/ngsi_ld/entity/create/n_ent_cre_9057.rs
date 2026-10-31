//! A refused create and a create sent at once with the same new attribute name answer 409 and 201.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Create Entity of an existing id and Create Entity of a new id, sent at once and both carrying the same
/// never-used attribute IRI, answer 409 `AlreadyExists` and 201: the existing Entity keeps only what it held, and
/// the new one reads the attribute back
/// Clause: 5.6.1, 5.5.4, 4.5.2
#[gtest]
#[tokio::test]
async fn n_ent_cre_9057_a_refused_create_and_a_create_sent_at_once_with_the_same_new_attribute_name_answer_409_and_201() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let existing_id = namespace.urn("Widget", "abort-existing")?;
    let fresh_id = namespace.urn("Widget", "abort-fresh")?;
    let widget_type = namespace.entity_type("Widget")?;
    let seed = format!("https://example.test/cold/{namespace}-seed");
    let contended = format!("https://example.test/cold/{namespace}-contended");
    let existing = Entity::builder()
        .id(existing_id.clone()) // clone: the Entity is retrieved by its id after it is built
        .entity_type(widget_type.clone()) // clone: every Widget is built with the same type
        .property(seed.as_str(), 1)
        .build()?;
    broker.entities().create(&existing).send().await?.done()?;
    let recreated = Entity::builder()
        .id(existing_id.clone()) // clone: the Entity is retrieved by its id after it is built
        .entity_type(widget_type.clone()) // clone: every Widget is built with the same type
        .property(contended.as_str(), 1)
        .build()?;
    let fresh = Entity::builder()
        .id(fresh_id.clone()) // clone: the Entity is retrieved by its id after it is built
        .entity_type(widget_type)
        .property(contended.as_str(), 1)
        .build()?;
    let responding = broker.with_responses();
    let entities = responding.entities();
    let (refused, created) = tokio::join!(entities.create(&recreated).send(), entities.create(&fresh).send());
    expect_that!(&refused, problem(StatusCode::CONFLICT, ErrorType::AlreadyExists));
    expect_that!(&created, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&existing_id)
        .send()
        .await?;
    expect_that!(retrieved.body(), lacks_attribute(&contended));
    expect_that!(retrieved.body(), has_attribute(&seed, any_attribute().value(1)));
    let retrieved_fresh = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&fresh_id)
        .send()
        .await?;
    expect_that!(retrieved_fresh.body(), has_attribute(&contended, any_attribute().value(1)));
    Ok(())
}
