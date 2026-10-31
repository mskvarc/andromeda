//! A query last n over a source history counts no instance without observed at.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
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
    query::temporal_query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Temporal Evolution of Entities after 2020-01-01 with `lastN` 2 over a local Entity whose speed holds
/// 120 and 80 observed at 12:03 and 12:05, under an inclusive source answering speed 100 observed at 12:07 and 999
/// with no `observedAt`, answers 80 and 100; with `lastN` 10, and without `lastN`, it answers 120, 80 and 100 and
/// not 999
/// Clause: 5.7.4.4, 4.11, 4.5.5, 4.5.7, Table 5.2.21-1
#[gtest]
#[tokio::test]
async fn n_fed_tmp_9015_a_query_last_n_over_a_source_history_counts_no_instance_without_observed_at() -> Result<()> {
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
    let source_evolutions = [TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 999)
        .property("speed", Property::builder(100).observed_at("2020-08-01T12:07:00Z".parse()?))
        .build()?];
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities").query_containing("type", format!("Vehicle_{namespace}")),
            source_answer::temporal_entities(&source_evolutions),
        )
        .await?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?).last_n(2))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(
            0,
            all!(
                has_id(&vehicle_id),
                has_evolution("speed", all!(len(eq(2)), nth(0, any_attribute().value(80)), nth(1, any_attribute().value(100))))
            )
        )
    );
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?).last_n(10))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(
        page_2.items(),
        nth(
            0,
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
        )
    );
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(
        page_3.items(),
        nth(
            0,
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
        )
    );
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/temporal/entities").await?;
    expect_that!(forwarded, len(eq(3)));
    Ok(())
}
