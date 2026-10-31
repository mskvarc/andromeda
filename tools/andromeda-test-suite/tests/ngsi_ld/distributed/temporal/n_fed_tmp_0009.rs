//! A local temporal retrieve is not forwarded.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    evolution_matcher::{has_evolution, lacks_evolution},
    namespace::Namespace,
    outcome::Upserted,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{Property, evolution::TemporalEntity},
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Temporal Evolution of an Entity with local true answers the local Entity and never contacts the
/// matching source
/// Clause: 5.7.3.4, 6.3.18
/// Tenant: private (it counts every request its context source receives, and the discovery and unscoped requests of
/// other tests reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_fed_tmp_0009_a_local_temporal_retrieve_is_not_forwarded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2020-08-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(80).observed_at("2020-08-01T12:05:00Z".parse()?))
        .property("speed", Property::builder(100).observed_at("2020-08-01T12:07:00Z".parse()?))
        .property("fuelLevel", Property::builder(67).observed_at("2020-08-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(53).observed_at("2020-08-01T13:05:00Z".parse()?))
        .property("fuelLevel", Property::builder(40).observed_at("2020-08-01T14:07:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .operations([
            DistributedOperation::RetrieveEntity,
            DistributedOperation::QueryEntity,
            DistributedOperation::RetrieveTemporal,
            DistributedOperation::QueryTemporal,
        ])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("altitude", Property::builder(100).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    source
        .answer_when(
            SourceRequest::matching(Method::GET, "/a/ngsi-ld/v1/temporal/entities/[^/]+"),
            source_answer::temporal_entity(&source_evolution),
        )
        .await?;
    let retrieved = broker
        .with_accept(Accept::any())
        .temporal()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("speed", len(eq(3))));
    expect_that!(evolution, lacks_evolution("altitude"));
    let forwarded_request = SourceRequest::any_matching("/a/ngsi-ld/v1/(entities|entityOperations|temporal).*");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(0)));
    Ok(())
}
