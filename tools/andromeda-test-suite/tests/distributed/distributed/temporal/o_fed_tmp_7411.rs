//! Limit and offset page a source result without repeating it.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Property, evolution::TemporalEntity},
    query::temporal_query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Temporal Evolution of Entities by type with limit 2 over five Entities only a source holds answers
/// pages of two, two and one with count 5 each, in ascending identifier order
/// Clause: 5.5.9, 5.7.4.4
/// Recorded: clause 5.5.9 requires paging to cover the result without repeating it but does not fix the order of a
/// distributed result; Andromeda pages it by identifier
#[gtest]
#[tokio::test]
async fn o_fed_tmp_7411_limit_and_offset_page_a_source_result_without_repeating_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let vehicle_4_id = namespace.urn("Vehicle", 4)?;
    let vehicle_5_id = namespace.urn("Vehicle", 5)?;
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
    let source_evolutions = [
        TemporalEntity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(10).observed_at("2020-08-01T12:00:00Z".parse()?))
            .build()?,
        TemporalEntity::builder()
            .id(vehicle_2_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(10).observed_at("2020-08-01T12:00:00Z".parse()?))
            .build()?,
        TemporalEntity::builder()
            .id(vehicle_3_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(10).observed_at("2020-08-01T12:00:00Z".parse()?))
            .build()?,
        TemporalEntity::builder()
            .id(vehicle_4_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(10).observed_at("2020-08-01T12:00:00Z".parse()?))
            .build()?,
        TemporalEntity::builder()
            .id(vehicle_5_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(10).observed_at("2020-08-01T12:00:00Z".parse()?))
            .build()?,
    ];
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities")
                .without_query("offset")
                .priority(1),
            source_answer::temporal_entities(&source_evolutions),
        )
        .await?;
    let source_evolutions_2 = [];
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities").priority(2),
            source_answer::temporal_entities(&source_evolutions_2),
        )
        .await?;
    let page = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .limit(2)
        .offset(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.total(), some(eq(5)));
    expect_that!(page.items(), nth(0, has_id(&vehicle_id)));
    expect_that!(page.items(), nth(1, has_id(&vehicle_2_id)));
    let page_2 = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .limit(2)
        .offset(2)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.total(), some(eq(5)));
    expect_that!(page_2.items(), nth(0, has_id(&vehicle_3_id)));
    expect_that!(page_2.items(), nth(1, has_id(&vehicle_4_id)));
    let page_3 = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .limit(2)
        .offset(4)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.total(), some(eq(5)));
    expect_that!(page_3.items(), nth(0, has_id(&vehicle_5_id)));
    Ok(())
}
