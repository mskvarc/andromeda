//! A PATCH of an entity attribute is accepted with `application/merge-patch+json` as its Content-Type (clause 6.3.4).

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::{content_type, send_to},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::ContentType,
        operation::{CreateEntity, PartialAttributeUpdate},
    },
    model::{Attribute, AttributeName, Entity, Property, Relationship},
};

/// The client of `broker` sending `vocabulary` in the body, as the create test sends the test vocabulary.
fn in_body(broker: &BrokerClient, vocabulary: &Vocabulary) -> Result<BrokerClient> {
    Ok(broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody))
}

/// Pins: Verify that PATCH HTTP requests can be done with "application/merge-patch+json" as Content-Type
/// Clause: 6.3.4
#[gtest]
#[tokio::test]
async fn e044_01_01_patch_entity_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy"]).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let parking = namespace.urn("OffStreetParking", "Downtown1")?;
    let person = namespace.urn("Person", "Bob")?;
    let entity = Entity::builder()
        .id(id.clone())
        .entity_type("Vehicle")
        .property("brandName", "Mercedes")
        .relationship(
            "isParked",
            Relationship::builder(parking)
                .observed_at("2017-07-29T12:00:04Z".parse()?)
                .relationship("providedBy", person),
        )
        .build()?;
    let created = send_to(&in_body(&broker, &vocabulary)?, &CreateEntity::builder().entity(&entity).build(), "entities/").await;
    verify_that!(&created, answered(StatusCode::CREATED))?;

    let client = broker.with_context(vocabulary.context())?;
    let brand: AttributeName = "brandName".parse()?;
    let patch = Attribute::from(Property::builder("BMW").build()?).into_patch().into();
    let update = PartialAttributeUpdate::builder().id(&id).name(&brand).attribute(&patch).build();
    let answer = client
        .send_unchecked(content_type(client.unchecked(&update)?, &ContentType::MergePatch.media_type())?)
        .await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));

    broker.entities().delete(&id).send().await.any_answer();
    broker.temporal().delete(&id).send().await.any_answer();
    Ok(())
}
