//! Every batch write under a context link that does not process is bad request data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, Entity, EntityFragment, ErrorType, NgsiLdMediaType},
};

/// Pins: Batch Entity Create, Upsert, Update and Merge of application/json entities under a context Link whose
/// document loads but defines a term with a non-string @id each answer 400 `BadRequestData` problem details rather
/// than 207, and write nothing
/// Clause: 5.5.6, 6.3.5, 5.6.7.4, 5.6.8.4, 5.6.9.4, 5.6.20.4
/// Recorded: 5.5.6 raises `BadRequestData` for an invalid remote @context and the batch clauses report entity
/// failures per entity; Andromeda treats the Link context, which holds for every entity, as the request's and
/// refuses it whole
#[gtest]
#[tokio::test]
async fn o_ctx_exp_9053_every_batch_write_under_a_context_link_that_does_not_process_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let device_existing_id = namespace.urn("Device", "existing")?;
    let device_type = namespace.entity_type("Device")?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/invalid.jsonld")?))?;
    let device_created_id = namespace.urn("Device", "created")?;
    let device_upserted_id = namespace.urn("Device", "upserted")?;
    contexts
        .serve(
            "jc/invalid.jsonld",
            r#"{
      "@context": {
        "speed": {
          "@id": true
        }
      }
    }"#,
            Some("application/ld+json"),
        )
        .await?;
    let device_existing = Entity::builder()
        .id(device_existing_id.clone())
        .entity_type(device_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&device_existing).send().await?.done()?;
    let entities = [Entity::builder()
        .id(device_created_id.clone())
        .entity_type(device_type.clone())
        .property("speed", 2)
        .build()?];
    let answer = linked.with_responses().entities().batch_create(&entities).send().await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_header_containing(HeaderName::from_static("content-type"), "application/json")
    );
    let entities_2 = [Entity::builder()
        .id(device_upserted_id.clone())
        .entity_type(device_type.clone())
        .property("speed", 2)
        .build()?];
    let answer_2 = linked.with_responses().entities().batch_upsert(&entities_2).send().await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragments = [EntityFragment::for_entity(device_existing_id.clone(), device_type.clone())
        .property("speed", 3)
        .build()?];
    let answer_3 = linked.with_responses().entities().batch_update(&fragments).send().await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let fragments_2 = [EntityFragment::for_entity(device_existing_id.clone(), device_type.clone())
        .property("speed", 4)
        .build()?];
    let answer_4 = linked.with_responses().entities().batch_merge(&fragments_2).send().await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_5 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_created_id)
        .send()
        .await;
    expect_that!(&answer_5, answered(StatusCode::NOT_FOUND));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_upserted_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&device_existing_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(1)));
    Ok(())
}
