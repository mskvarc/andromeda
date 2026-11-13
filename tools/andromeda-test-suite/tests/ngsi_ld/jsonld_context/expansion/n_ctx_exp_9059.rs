//! A deletion under a context link that does not process is bad request data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Context, Entity, EntityTypeName, ErrorType, NgsiLdMediaType},
    query::Locality,
};

/// Pins: Delete Attribute, Delete Entity by id and a local Purge Entities selecting by a type written as a full IRI,
/// under a context Link whose document loads but defines a term with a non-string @id, each answer 400
/// `BadRequestData` and delete nothing; under a context Link whose document loads and processes, each deletes
/// Clause: 5.5.6, 6.3.5, 5.6.5.4, 5.6.6.4, 5.6.21.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9059_a_deletion_under_a_context_link_that_does_not_process_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let invalid = broker.with_context(Context::Single(contexts.reference("jc/invalid.jsonld")?))?;
    let plain = broker.with_context(Context::Single(contexts.reference("jc/plain.jsonld")?))?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let building_type = format!("https://test.andromeda/{namespace}/Building").parse::<EntityTypeName>()?;
    let building_id = namespace.urn("Building", 1)?;
    let speed = "speed".parse::<AttributeName>()?;
    contexts
        .serve(
            "jc/invalid.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "gear_{namespace}": {{
          "@id": true
        }}
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "jc/plain.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "gear_{namespace}": "https://test.andromeda/{namespace}/gear"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let entities = [
        Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type)
            .property("speed", 30)
            .build()?,
        Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let answer = invalid.with_responses().entities().delete_attribute(&vehicle_id, &speed).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = invalid.with_responses().entities().delete(&vehicle_id).send().await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_3 = invalid
        .with_responses()
        .entities()
        .purge()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let vehicle = retrieved.body();
    expect_that!(vehicle, has_attribute("speed", any_attribute().value(30)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let building = retrieved_2.body();
    expect_that!(building.id, eq(&building_id));
    plain.entities().delete_attribute(&vehicle_id, &speed).send().await?.done()?;
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let vehicle_2 = retrieved_3.body();
    expect_that!(vehicle_2, lacks_attribute("speed"));
    plain.entities().delete(&vehicle_id).send().await?.done()?;
    let received = plain
        .with_responses()
        .entities()
        .purge()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .send()
        .await?;
    received.outcome.done()?;
    for id in [&vehicle_id, &building_id] {
        let answer_4 = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(id)
            .send()
            .await;
        expect_that!(&answer_4, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    }
    Ok(())
}
