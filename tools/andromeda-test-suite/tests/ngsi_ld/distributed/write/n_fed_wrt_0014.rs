//! A no overwrite append under two redirect registrations reaches each source.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    partial_outcome::PartialSuccess,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::operation::Overwrite,
    model::{EntityFragment, Relationship},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Append Entity Attributes with `noOverwrite` to an Entity held by two redirect sources that answer 207
/// answers 207 and forwards one append to each source
/// Clause: 5.6.3.4, 6.3.17, 4.3.6.2
#[gtest]
#[tokio::test]
async fn n_fed_wrt_0014_a_no_overwrite_append_under_two_redirect_registrations_reaches_each_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let off_street_parking_downtown2_id = namespace.urn("OffStreetParking", "Downtown2")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .mode(RegistrationMode::Redirect)
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
    let registration_2 = CSourceRegistration::builder(source.endpoint("b")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .mode(RegistrationMode::Redirect)
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
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    source
        .answer_when(
            SourceRequest::matching(Method::POST, "/a/ngsi-ld/v1/entities/[^/]+/attrs"),
            source_answer::status(StatusCode::MULTI_STATUS),
        )
        .await?;
    source
        .answer_when(
            SourceRequest::matching(Method::POST, "/b/ngsi-ld/v1/entities/[^/]+/attrs"),
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
    let forwarded_request_2 = SourceRequest::matching(Method::POST, "/b/ngsi-ld/v1/entities/[^/]+/attrs");
    let forwarded_2 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_2, len(eq(1)));
    Ok(())
}
