//! With split entities a temporal q is applied after the merge and not forwarded.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    forwarded_query::query_values,
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
    query::{EntitySplitting, q, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Temporal Evolution of Entities by type with `splitEntities` true and q speed>60 over a local Entity
/// of speed 50 and a source answering Entities of speed 80 and 40 answers only the one of speed 80, and the
/// forwarded query carries no q
/// Clause: 5.7.4.4, 4.9, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_fed_tmp_0001_with_split_entities_a_temporal_q_is_applied_after_the_merge_and_not_forwarded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).observed_at("2024-01-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let source_evolutions = [
        TemporalEntity::builder()
            .id(vehicle_2_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(80).observed_at("2024-01-01T12:00:00Z".parse()?))
            .build()?,
        TemporalEntity::builder()
            .id(vehicle_3_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(40).observed_at("2024-01-01T12:00:00Z".parse()?))
            .build()?,
    ];
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
        .q(q::attr("speed").gt(60))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&vehicle_2_id)));
    expect_that!(page.items(), not(contains(has_id(&vehicle_id))));
    expect_that!(page.items(), not(contains(has_id(&vehicle_3_id))));
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/temporal/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(query_values(forwarded_one, "q"), is_empty());
    Ok(())
}
