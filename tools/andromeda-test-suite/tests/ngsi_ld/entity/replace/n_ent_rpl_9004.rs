//! A replacement holding only the id is bad request data.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::ReplaceEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType},
};

/// Pins: Replace Entity with a body holding only the target id, and so no type, answers 400 `BadRequestData` and
/// the target keeps its value
/// Clause: 5.2.4, 5.5.4, 5.6.18.4
#[gtest]
#[tokio::test]
async fn n_ent_rpl_9004_a_replacement_holding_only_the_id_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let replaced_identity_id_only_id = namespace.urn("ReplacedIdentity", "id-only")?;
    let replaced_identity_type = namespace.entity_type("ReplacedIdentity")?;
    let placeholder_type = namespace.entity_type("Placeholder")?;
    let replaced_identity_id_only = Entity::builder()
        .id(replaced_identity_id_only_id.clone())
        .entity_type(replaced_identity_type)
        .property("speed", 1)
        .build()?;
    broker.entities().create(&replaced_identity_id_only).send().await?.done()?;
    let replacement = Entity::builder()
        .id(replaced_identity_id_only_id.clone())
        .entity_type(placeholder_type)
        .build()?;
    // The model requires `type`, which the request leaves out, so it is written for a placeholder and removed.
    let document = UncheckedDocument::from_entity(&replacement, Lossless::Normalized).remove_member("type");
    let request = broker.unchecked(&ReplaceEntity::builder().entity(&replacement).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&replaced_identity_id_only_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(1)));
    Ok(())
}
