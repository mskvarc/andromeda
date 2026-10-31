//! A default instance delete under an inclusive registration deletes locally and forwards.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, Property},
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Delete Entity Attribute speed without `datasetId` of an Entity holding a default and a `datasetId` speed
/// under an inclusive registration answers 204, forwards one delete, and only the `datasetId` instance remains
/// Clause: 5.6.5.4, 4.3.6.2
#[gtest]
#[tokio::test]
async fn n_fed_wrt_0016_a_default_instance_delete_under_an_inclusive_registration_deletes_locally_and_forwards() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let dataset_1 = namespace.dataset_id("Dataset", 1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .property("speed", Property::builder(50).dataset_id(namespace.dataset_id("Dataset", 1)?))
        .property("speed", 60)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([
            DistributedOperation::CreateEntity,
            DistributedOperation::UpdateEntity,
            DistributedOperation::DeleteEntity,
            DistributedOperation::MergeEntity,
            DistributedOperation::ReplaceEntity,
            DistributedOperation::AppendAttrs,
            DistributedOperation::UpdateAttrs,
            DistributedOperation::DeleteAttrs,
        ])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer_when(
            SourceRequest::matching(Method::DELETE, "/a/ngsi-ld/v1/entities/[^/]+/attrs/speed"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    broker
        .entities()
        .delete_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    let forwarded_request = SourceRequest::matching(Method::DELETE, "/a/ngsi-ld/v1/entities/[^/]+/attrs/speed");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(50).with_dataset_id(dataset_1)));
    Ok(())
}
