//! A retrieve last n over a source history counts no instance without observed at.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Temporal Evolution of a local Entity whose speed holds 120 and 80 observed at 12:03 and 12:05,
/// under an inclusive source answering speed 100 observed at 12:07 and 999 with no `observedAt`, answers `lastN` 2
/// with 80 and 100, and `lastN` 10 and a read without `lastN` each with 120, 80 and 100 and not 999
/// Clause: 5.7.3.4, 4.5.5, 4.5.7, Table 5.2.21-1
#[gtest]
#[tokio::test]
async fn n_fed_tmp_9014_a_retrieve_last_n_over_a_source_history_counts_no_instance_without_observed_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(120).observed_at("2020-08-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(80).observed_at("2020-08-01T12:05:00Z".parse()?))
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
        .property("speed", 999)
        .property("speed", Property::builder(100).observed_at("2020-08-01T12:07:00Z".parse()?))
        .build()?;
    source
        .answer_when(
            SourceRequest::matching(Method::GET, "/a/ngsi-ld/v1/temporal/entities/[^/]+"),
            source_answer::temporal_entity(&source_evolution),
        )
        .await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .last_n(2)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution("speed", all!(len(eq(2)), nth(0, any_attribute().value(80)), nth(1, any_attribute().value(100))))
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .last_n(10)
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(
        evolution_2,
        has_evolution(
            "speed",
            all!(
                len(eq(3)),
                nth(0, any_attribute().value(120)),
                nth(1, any_attribute().value(80)),
                nth(2, any_attribute().value(100)),
                not(contains(any_attribute().value(999)))
            )
        )
    );
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution_3 = retrieved_3.body();
    expect_that!(
        evolution_3,
        has_evolution(
            "speed",
            all!(
                len(eq(3)),
                nth(0, any_attribute().value(120)),
                nth(1, any_attribute().value(80)),
                nth(2, any_attribute().value(100)),
                not(contains(any_attribute().value(999)))
            )
        )
    );
    let forwarded_request = SourceRequest::matching(Method::GET, "/a/ngsi-ld/v1/temporal/entities/[^/]+");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(3)));
    Ok(())
}
