//! A merge under an inclusive registration merges locally and forwards.

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
    model::{Entity, EntityFragment, Relationship},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Merge Entity of speed and `isParked2` into an Entity held locally under an inclusive registration answers
/// 204, forwards one merge, and the Entity keeps `brandName`, takes the new `isParked2` and gains speed
/// Clause: 5.6.17.4, 4.3.6.2
#[gtest]
#[tokio::test]
async fn n_fed_wrt_0006_a_merge_under_an_inclusive_registration_merges_locally_and_forwards() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown2_id = namespace.urn("OffStreetParking", "Downtown2")?;
    let registration_id = namespace.registration_id(1)?;
    let off_street_parking_downtown3_id = namespace.urn("OffStreetParking", "Downtown3")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "BMW")
        .relationship(
            "isParked2",
            Relationship::builder(off_street_parking_downtown2_id.clone()).observed_at("2017-07-29T12:00:04Z".parse()?),
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
        ])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let retrieved = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute("isParked2", any_attribute().object(off_street_parking_downtown2_id.clone()))
    );
    source
        .answer_when(
            SourceRequest::matching(Method::PATCH, "/a/ngsi-ld/v1/entities/[^/]+"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let fragment = EntityFragment::builder()
        .property("speed", 56)
        .relationship(
            "isParked2",
            Relationship::builder(off_street_parking_downtown3_id.clone()).observed_at("2018-07-29T12:00:04Z".parse()?),
        )
        .build()?;
    broker.entities().merge(&vehicle_id, &fragment).send().await?.done()?;
    let forwarded_request = SourceRequest::matching(Method::PATCH, "/a/ngsi-ld/v1/entities/[^/]+");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let retrieved_2 = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("brandName", any_attribute().value("BMW")));
    expect_that!(
        entity_2,
        has_attribute("isParked2", any_attribute().object(off_street_parking_downtown3_id.clone()))
    );
    expect_that!(entity_2, has_attribute("speed", any_attribute().value(56)));
    Ok(())
}
