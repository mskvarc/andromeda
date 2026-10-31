//! A no overwrite append under an inclusive registration keeps the existing attribute.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    partial_outcome::PartialSuccess,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::Overwrite},
    model::{Entity, EntityFragment, Relationship},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Append Entity Attributes with `noOverwrite` of speed and `isParked` to an Entity held locally under an
/// inclusive registration whose source answers 207 answers 207, forwards one append, and the Entity gains speed and
/// keeps its `isParked`
/// Clause: 5.6.3.4, 6.3.17
#[gtest]
#[tokio::test]
async fn n_fed_wrt_0013_a_no_overwrite_append_under_an_inclusive_registration_keeps_the_existing_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let registration_id = namespace.registration_id(1)?;
    let off_street_parking_downtown2_id = namespace.urn("OffStreetParking", "Downtown2")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .relationship(
            "isParked",
            Relationship::builder(off_street_parking_downtown1_id.clone()).observed_at("2017-07-29T12:00:04Z".parse()?),
        )
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
            SourceRequest::matching(Method::POST, "/a/ngsi-ld/v1/entities/[^/]+/attrs"),
            source_answer::status(StatusCode::MULTI_STATUS),
        )
        .await?;
    let fragment = EntityFragment::builder()
        .property("speed", 120)
        .relationship(
            "isParked",
            Relationship::builder(off_street_parking_downtown2_id).observed_at("2018-01-01T12:00:00Z".parse()?),
        )
        .build()?;
    broker
        .entities()
        .append_attributes(&vehicle_id, &fragment)
        .overwrite(Overwrite::Keep)
        .send()
        .await?
        .partial()?;
    let forwarded_request = SourceRequest::matching(Method::POST, "/a/ngsi-ld/v1/entities/[^/]+/attrs");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let retrieved = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(120)));
    expect_that!(
        entity,
        has_attribute("isParked", any_attribute().object(off_street_parking_downtown1_id.clone()))
    );
    Ok(())
}
