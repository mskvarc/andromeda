//! The local evolution is restricted to the window before the merge.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Upserted,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Property, evolution::TemporalEntity},
    query::temporal_query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Temporal Evolution with timerel between over a local Entity under a registered source answering
/// 404 answers only the two instances inside the window
/// Clause: 5.7.3.4, 4.11
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7401_the_local_evolution_is_restricted_to_the_window_before_the_merge() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2020-08-01T12:00:00Z".parse()?))
        .property("speed", Property::builder(80).observed_at("2020-08-01T12:05:00Z".parse()?))
        .property("speed", Property::builder(100).observed_at("2020-08-01T12:10:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .operations([DistributedOperation::RetrieveTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(
            Method::GET,
            &format!("/a/ngsi-ld/v1/temporal/entities/{vehicle_id}"),
            source_answer::status(StatusCode::NOT_FOUND),
        )
        .await?;
    let retrieved = broker
        .with_accept(Accept::any())
        .temporal()
        .retrieve(&vehicle_id)
        .temporal_q(temporal_query::between("2020-08-01T12:00:00Z".parse()?, "2020-08-01T12:06:00Z".parse()?))
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "speed",
            all!(
                len(eq(2)),
                not(contains(any_attribute().value(100))),
                contains(any_attribute().value(120)),
                contains(any_attribute().value(80))
            )
        )
    );
    Ok(())
}
