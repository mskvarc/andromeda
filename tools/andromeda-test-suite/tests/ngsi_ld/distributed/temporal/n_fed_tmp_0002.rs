//! With split entities and no filter the local and the forwarded temporal entities are answered.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    identity_matcher::has_id,
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
    query::{EntitySplitting, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Temporal Evolution of Entities by type with `splitEntities` true and no filter answers the local
/// Entity and the one the source returns
/// Clause: 5.7.4.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_fed_tmp_0002_with_split_entities_and_no_filter_the_local_and_the_forwarded_temporal_entities_are_answered() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).observed_at("2024-01-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let source_evolutions = [TemporalEntity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(80).observed_at("2024-01-01T12:00:00Z".parse()?))
        .build()?];
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities").query_containing("type", format!("Vehicle_{namespace}")),
            source_answer::temporal_entities(&source_evolutions),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&vehicle_id)));
    expect_that!(page.items(), contains(has_id(&vehicle_2_id)));
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/temporal/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    Ok(())
}
