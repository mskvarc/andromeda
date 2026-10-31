//! A capture contacts only the registrations a context source filter admits.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    context_source_property::property_value,
    namespace::Namespace,
    snapshot_capture::await_status,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    model::Entity,
    query::q,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};

/// Pins: A Snapshot whose entity query carries a csf over tier over inclusive registrations, one with tier gold and
/// one with tier silver, ends success and the capture contacts the gold source only
/// Clause: 5.16.1.4, 5.7.2.4, 4.9, 5.10.2.4
#[gtest]
#[tokio::test]
async fn n_snp_cre_8161_a_capture_contacts_only_the_registrations_a_context_source_filter_admits() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .property("tier", property_value("gold"))
        .build()?;
    let registration_2 = CSourceRegistration::builder(source.endpoint("b")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .property("tier", property_value("silver"))
        .build()?;
    broker.registrations().register(&registration).send().await?;
    broker.registrations().register(&registration_2).send().await?;
    let vehicle = Entity::output_builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/entities", source_answer::entities(&[vehicle]))
        .await?;
    source.answer(Method::GET, "/b/ngsi-ld/v1/entities", source_answer::entities(&[])).await?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(
            QueryDocument::builder()
                .entities([vehicle_type.as_str()])
                .csf(q::attr("tier").eq("gold").build()?)
                .build()?,
        )
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    await_status(&broker, &snapshot_id, "success".parse::<SnapshotStatus>()?).await?;
    let first = source.received_matching(&SourceRequest::any_matching("/a/.*")).await?;
    expect_that!(first, not(is_empty()));
    let second = source.received_matching(&SourceRequest::any_matching("/b/.*")).await?;
    expect_that!(second, len(eq(0)));
    broker.snapshots().delete(&snapshot_id).send().await?;
    broker.registrations().delete(&registration_id).send().await?;
    broker.registrations().delete(&registration_2_id).send().await?;
    Ok(())
}
