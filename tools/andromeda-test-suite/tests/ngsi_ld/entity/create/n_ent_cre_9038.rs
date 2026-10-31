//! A refused create leaves its new attribute name to the next create.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Create Entity of an existing id carrying a never-used attribute IRI answers 409 `AlreadyExists` and leaves
/// the existing Entity without it; Create Entity of a new id with the same IRI then answers 201 and reads it back
/// Clause: 5.6.1, 5.5.4, 4.5.2
#[gtest]
#[tokio::test]
async fn n_ent_cre_9038_a_refused_create_leaves_its_new_attribute_name_to_the_next_create() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let widget_abort_existing_id = namespace.urn("Widget", "abort-existing")?;
    let widget_type = namespace.entity_type("Widget")?;
    let widget_abort_fresh_id = namespace.urn("Widget", "abort-fresh")?;
    let widget_abort_existing = Entity::builder()
        .id(widget_abort_existing_id.clone())
        .entity_type(widget_type.clone())
        .property(format!("https://example.test/cold/{namespace}-seed").as_str(), 1)
        .build()?;
    broker.entities().create(&widget_abort_existing).send().await?.done()?;
    let widget_abort_existing_2 = Entity::builder()
        .id(widget_abort_existing_id.clone())
        .entity_type(widget_type.clone())
        .property(format!("https://example.test/cold/{namespace}-contended").as_str(), 1)
        .build()?;
    let answer = broker.with_responses().entities().create(&widget_abort_existing_2).send().await;
    expect_that!(&answer, problem(StatusCode::CONFLICT, ErrorType::AlreadyExists));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&widget_abort_existing_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, lacks_attribute(&format!("https://example.test/cold/{namespace}-contended")));
    expect_that!(
        entity,
        has_attribute(&format!("https://example.test/cold/{namespace}-seed"), any_attribute().value(1))
    );
    let widget_abort_fresh = Entity::builder()
        .id(widget_abort_fresh_id.clone())
        .entity_type(widget_type.clone())
        .property(format!("https://example.test/cold/{namespace}-contended").as_str(), 1)
        .build()?;
    broker.entities().create(&widget_abort_fresh).send().await?.done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&widget_abort_fresh_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(
        entity_2,
        has_attribute(&format!("https://example.test/cold/{namespace}-contended"), any_attribute().value(1))
    );
    Ok(())
}
