//! A replacement without an id keeps the target id.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::EntityIdMember},
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Replace Entity with a body naming no id answers 204, and Query Entities by the type and the target id
/// answers the Entity with the new value
/// Clause: 5.6.18.4
#[gtest]
#[tokio::test]
async fn n_ent_rpl_9002_a_replacement_without_an_id_keeps_the_target_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let replaced_identity_without_id_id = namespace.urn("ReplacedIdentity", "without-id")?;
    let replaced_identity_type = namespace.entity_type("ReplacedIdentity")?;
    let replaced_identity_without_id = Entity::builder()
        .id(replaced_identity_without_id_id.clone())
        .entity_type(replaced_identity_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&replaced_identity_without_id).send().await?.done()?;
    let replacement = Entity::builder()
        .id(replaced_identity_without_id_id.clone())
        .entity_type(replaced_identity_type.clone())
        .property("speed", 2)
        .build()?;
    let answer = broker
        .with_responses()
        .entities()
        .replace(&replacement)
        .id_member(EntityIdMember::Omitted)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&replaced_identity_type)
        .ids(vec![replaced_identity_without_id_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(
            0,
            all!(has_id(&replaced_identity_without_id_id), has_attribute("speed", any_attribute().value(2)))
        )
    );
    Ok(())
}
