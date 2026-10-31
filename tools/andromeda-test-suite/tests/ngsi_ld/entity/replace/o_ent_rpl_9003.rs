//! A replacement naming another entity is bad request data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    wire::api_uri,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::ReplaceEntity},
    model::{Entity, ErrorType, NgsiLdMediaType},
};

/// Pins: Replace Entity with a body whose id names another Entity answers 400 `BadRequestData`, and the target
/// keeps its value
/// Clause: 5.6.18.4
/// Recorded: 5.6.18.4 replaces the target "with the same Entity ID" and says nothing of a body naming another;
/// Andromeda refuses it with `BadRequestData`
#[gtest]
#[tokio::test]
async fn o_ent_rpl_9003_a_replacement_naming_another_entity_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let replaced_identity_target_id = namespace.urn("ReplacedIdentity", "target")?;
    let replaced_identity_type = namespace.entity_type("ReplacedIdentity")?;
    let replaced_identity_other_id = namespace.urn("ReplacedIdentity", "other")?;
    let replaced_identity_target = Entity::builder()
        .id(replaced_identity_target_id.clone())
        .entity_type(replaced_identity_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&replaced_identity_target).send().await?.done()?;
    // The operation writes its path from the body's id; the request goes to the path the test names.
    let replacement = Entity::builder()
        .id(replaced_identity_other_id.clone())
        .entity_type(replaced_identity_type.clone())
        .property("speed", 2)
        .build()?;
    let request = broker
        .unchecked(&ReplaceEntity::builder().entity(&replacement).build())?
        .uri(api_uri(&broker, &format!("entities/{replaced_identity_target_id}"))?);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&replaced_identity_target_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(1)));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&replaced_identity_other_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
