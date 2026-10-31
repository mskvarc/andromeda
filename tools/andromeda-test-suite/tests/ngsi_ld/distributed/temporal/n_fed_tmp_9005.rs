//! A failing source page keeps the gathered entities with a warning.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    header_matcher::present,
    namespace::Namespace,
    outcome::Upserted,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Input, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{IdPattern, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Temporal Evolution of Entities at limit 1,000 and count true over an inclusive registration whose
/// source holds 2,500 Temporal Evolutions answered in pages of 1,000 linked by next, beside 14 the broker holds
/// itself (4 interleaving the source's identifiers, 10 shared with it), whose second page fails with 500, answers
/// 200 with the 1,000 Entities of its first page and the broker's 14, counting 1,014, and an NGSILD-Warning
/// Clause: 5.7.4.4, 6.3.17, 6.3.13
#[gtest]
#[tokio::test]
async fn n_fed_tmp_9005_a_failing_source_page_keeps_the_gathered_entities_with_a_warning() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let paged_temporal_type = namespace.entity_type("PagedTemporal")?;
    let registration_id = namespace.registration_id(1)?;
    let paged_temporal_00000_l_id = namespace.urn("PagedTemporal", "00000L")?;
    let paged_temporal_00999_l_id = namespace.urn("PagedTemporal", "00999L")?;
    let paged_temporal_01000_l_id = namespace.urn("PagedTemporal", "01000L")?;
    let paged_temporal_01500_l_id = namespace.urn("PagedTemporal", "01500L")?;
    // The source holds 2,500 Entities and answers them in pages of 1,000 linked by next; the mock server generates each
    // page, numbering the Entities from 00000.
    let source_evolutions = (0..=999)
        .map(|n: i64| -> Result<TemporalEntity<Input>> {
            let numbered_id = namespace.urn("PagedTemporal", format!("{n:05}"))?;
            Ok(TemporalEntity::builder()
                .id(numbered_id)
                .entity_type(paged_temporal_type.clone())
                .property("remote", Property::builder(n).observed_at("2020-01-01T00:00:00Z".parse()?))
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities").priority(5),
            source_answer::linked_to_next(
                source_answer::temporal_entities(&source_evolutions),
                format!("/a/ngsi-ld/v1/temporal/entities?type=PagedTemporal_{namespace}&limit=1000&offset=1000").parse()?,
            )?,
        )
        .await?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities")
                .query("offset", "1000")
                .priority(1),
            source_answer::status(StatusCode::INTERNAL_SERVER_ERROR),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(paged_temporal_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let paged_temporal_00000_l_evolution = TemporalEntity::builder()
        .id(paged_temporal_00000_l_id)
        .entity_type(paged_temporal_type.clone())
        .property("local", Property::builder(true).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&paged_temporal_00000_l_evolution).send().await?.created()?;
    let paged_temporal_00999_l_evolution = TemporalEntity::builder()
        .id(paged_temporal_00999_l_id)
        .entity_type(paged_temporal_type.clone())
        .property("local", Property::builder(true).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&paged_temporal_00999_l_evolution).send().await?.created()?;
    let paged_temporal_01000_l_evolution = TemporalEntity::builder()
        .id(paged_temporal_01000_l_id)
        .entity_type(paged_temporal_type.clone())
        .property("local", Property::builder(true).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&paged_temporal_01000_l_evolution).send().await?.created()?;
    let paged_temporal_01500_l_evolution = TemporalEntity::builder()
        .id(paged_temporal_01500_l_id)
        .entity_type(paged_temporal_type.clone())
        .property("local", Property::builder(true).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&paged_temporal_01500_l_evolution).send().await?.created()?;
    let evolutions = (2490..=2499)
        .map(|n| -> Result<TemporalEntity<Input>> {
            Ok(TemporalEntity::builder()
                .id(namespace.urn("PagedTemporal", format!("{n:05}"))?)
                .entity_type(paged_temporal_type.clone())
                .property("local", Property::builder(true).observed_at("2020-01-01T00:00:00Z".parse()?))
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    for evolution in &evolutions {
        broker.temporal().create_or_update(evolution).send().await?.created()?;
    }
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&paged_temporal_type)
        .limit(1000)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.warnings(), present());
    expect_that!(page.total(), some(eq(1014)));
    expect_that!(page.items(), len(eq(1000)));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
