//! An array holding only null is read as the empty array.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{attribute_members, has_attribute},
    namespace::Namespace,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity with a Property whose value is `[null]` answers 201, and the Entity reads back with that
/// Property holding the empty array, JSON-LD expansion having dropped the null
/// Clause: 5.6.1, 4.5.0
#[gtest]
#[tokio::test]
async fn n_ent_cre_9007_an_array_holding_only_null_is_read_as_the_empty_array() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let empty_probe_id = namespace.urn("EmptyProbe", 1)?;
    let empty_probe_type = namespace.entity_type("EmptyProbe")?;
    let empty_probe = Entity::builder().id(empty_probe_id.clone()).entity_type(empty_probe_type).build()?;
    // The model refuses it (invalid value at tags), so `tags` is written as text.
    let tags: RawJson = r#"{"type": "Property", "value": [null]}"#.parse()?;
    let document = UncheckedDocument::from_entity(&empty_probe, Lossless::Normalized).set_member("tags", &tags);
    let request = broker.unchecked(&CreateEntity::builder().entity(&empty_probe).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&empty_probe_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(attribute_members(entity, "tags"), some(eq(2)));
    expect_that!(entity, has_attribute("tags", property().length_at([], 0)));
    Ok(())
}
