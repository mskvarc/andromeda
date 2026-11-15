//! A query term matches the value a source held inside the window, not one it held before.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
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
    model::{Property, evolution::TemporalEntity},
    query::{EntitySplitting, q, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: under `splitEntities` a between 12:00-13:00 query q=speed>50 answers the Entity holding local data in the
/// window whose speed 60 a source observed at 12:20, and not the Entity whose speed 60 a source observed at 11:00
/// and never since, and q=speed>70 answers neither
/// Clause: 5.7.4.4, 4.11, 4.9
#[gtest]
#[tokio::test]
async fn o_fed_tmp_7433_a_query_term_matches_the_value_a_source_held_inside_the_window_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    for id in [&vehicle_id, &vehicle_2_id] {
        let vehicle_evolution = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property("brand", Property::builder("Mercedes").observed_at("2020-08-01T12:10:00Z".parse()?))
            .build()?;
        broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    }
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_before = [TemporalEntity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(60).observed_at("2020-08-01T11:00:00Z".parse()?))
        .build()?];
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities")
                .query("timerel", "before")
                .query("timeproperty", "observedAt")
                .priority(1),
            source_answer::temporal_entities(&source_before),
        )
        .await?;
    let source_window = [TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(60).observed_at("2020-08-01T12:20:00Z".parse()?))
        .build()?];
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities").priority(10),
            source_answer::temporal_entities(&source_window),
        )
        .await?;
    let page = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::between("2020-08-01T12:00:00Z".parse()?, "2020-08-01T13:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .q(q::attr("speed").gt(50))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_id)));
    let page_2 = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::between("2020-08-01T12:00:00Z".parse()?, "2020-08-01T13:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .q(q::attr("speed").gt(70))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    let before_the_window = SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities").query("timerel", "before");
    expect_that!(source.received_matching(&before_the_window).await?, len(eq(0)));
    Ok(())
}
