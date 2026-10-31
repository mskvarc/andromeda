//! Every hub page is the window of the whole union.

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
    http::{headers::Accept, operation::ResultsCount},
    model::{Input, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{IdPattern, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Temporal Evolution of Entities over an inclusive registration whose source holds 2,500 Temporal
/// Evolutions answered in pages of 1,000 linked by next, beside 14 the broker holds itself (4 interleaving the
/// source's identifiers, 10 shared with it) answers each page, at limit 100 and 1,000 and offsets inside, across
/// and beyond the source's page boundaries, as the window of the 2,504 identifiers in identifier order, and counts
/// 2,504
/// Clause: 5.7.4.4, 5.5.9, 6.3.13, 4.3.6.7
#[gtest]
#[tokio::test]
async fn a_fed_tmp_9001_every_hub_page_is_the_window_of_the_whole_union() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let paged_temporal_type = namespace.entity_type("PagedTemporal")?;
    let registration_id = namespace.registration_id(1)?;
    let paged_temporal_00000_l_id = namespace.urn("PagedTemporal", "00000L")?;
    let paged_temporal_00999_l_id = namespace.urn("PagedTemporal", "00999L")?;
    let paged_temporal_01000_l_id = namespace.urn("PagedTemporal", "01000L")?;
    let paged_temporal_01500_l_id = namespace.urn("PagedTemporal", "01500L")?;
    let paged_temporal_00000_id = namespace.urn("PagedTemporal", "00000")?;
    let paged_temporal_00049_id = namespace.urn("PagedTemporal", "00049")?;
    let paged_temporal_00098_id = namespace.urn("PagedTemporal", "00098")?;
    let paged_temporal_00899_id = namespace.urn("PagedTemporal", "00899")?;
    let paged_temporal_00949_id = namespace.urn("PagedTemporal", "00949")?;
    let paged_temporal_00998_id = namespace.urn("PagedTemporal", "00998")?;
    let paged_temporal_00999_id = namespace.urn("PagedTemporal", "00999")?;
    let paged_temporal_01046_id = namespace.urn("PagedTemporal", "01046")?;
    let paged_temporal_01047_id = namespace.urn("PagedTemporal", "01047")?;
    let paged_temporal_01096_id = namespace.urn("PagedTemporal", "01096")?;
    let paged_temporal_01946_id = namespace.urn("PagedTemporal", "01946")?;
    let paged_temporal_01996_id = namespace.urn("PagedTemporal", "01996")?;
    let paged_temporal_02045_id = namespace.urn("PagedTemporal", "02045")?;
    let paged_temporal_02446_id = namespace.urn("PagedTemporal", "02446")?;
    let paged_temporal_02473_id = namespace.urn("PagedTemporal", "02473")?;
    let paged_temporal_02499_id = namespace.urn("PagedTemporal", "02499")?;
    let paged_temporal_02496_id = namespace.urn("PagedTemporal", "02496")?;
    let paged_temporal_02498_id = namespace.urn("PagedTemporal", "02498")?;
    let paged_temporal_00499_id = namespace.urn("PagedTemporal", "00499")?;
    let paged_temporal_01497_id = namespace.urn("PagedTemporal", "01497")?;
    let paged_temporal_01995_id = namespace.urn("PagedTemporal", "01995")?;
    let paged_temporal_02248_id = namespace.urn("PagedTemporal", "02248")?;
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
    let source_evolutions_2 = (1000..=1999)
        .map(|n: i64| -> Result<TemporalEntity<Input>> {
            let numbered_id_2 = namespace.urn("PagedTemporal", format!("{n:05}"))?;
            Ok(TemporalEntity::builder()
                .id(numbered_id_2)
                .entity_type(paged_temporal_type.clone())
                .property("remote", Property::builder(n).observed_at("2020-01-01T00:00:00Z".parse()?))
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities")
                .query("offset", "1000")
                .priority(1),
            source_answer::linked_to_next(
                source_answer::temporal_entities(&source_evolutions_2),
                format!("/a/ngsi-ld/v1/temporal/entities?type=PagedTemporal_{namespace}&limit=1000&offset=2000").parse()?,
            )?,
        )
        .await?;
    let source_evolutions_3 = (2000..=2499)
        .map(|n: i64| -> Result<TemporalEntity<Input>> {
            let numbered_id_3 = namespace.urn("PagedTemporal", format!("{n:05}"))?;
            Ok(TemporalEntity::builder()
                .id(numbered_id_3)
                .entity_type(paged_temporal_type.clone())
                .property("remote", Property::builder(n).observed_at("2020-01-01T00:00:00Z".parse()?))
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/temporal/entities")
                .query("offset", "2000")
                .priority(1),
            source_answer::temporal_entities(&source_evolutions_3),
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
        .limit(100)
        .offset(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(2504)));
    expect_that!(page.items(), len(eq(100)));
    expect_that!(page.items(), nth(0, has_id(&paged_temporal_00000_id)));
    expect_that!(page.items(), nth(50, has_id(&paged_temporal_00049_id)));
    expect_that!(page.items(), nth(99, has_id(&paged_temporal_00098_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&paged_temporal_type)
        .limit(100)
        .offset(900)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_2.total(), some(eq(2504)));
    expect_that!(page_2.items(), len(eq(100)));
    expect_that!(page_2.items(), nth(0, has_id(&paged_temporal_00899_id)));
    expect_that!(page_2.items(), nth(50, has_id(&paged_temporal_00949_id)));
    expect_that!(page_2.items(), nth(99, has_id(&paged_temporal_00998_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&paged_temporal_type)
        .limit(100)
        .offset(950)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_3.total(), some(eq(2504)));
    expect_that!(page_3.items(), len(eq(100)));
    expect_that!(page_3.items(), nth(0, has_id(&paged_temporal_00949_id)));
    expect_that!(page_3.items(), nth(50, has_id(&paged_temporal_00999_id)));
    expect_that!(page_3.items(), nth(99, has_id(&paged_temporal_01046_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&paged_temporal_type)
        .limit(100)
        .offset(1000)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_4.total(), some(eq(2504)));
    expect_that!(page_4.items(), len(eq(100)));
    expect_that!(page_4.items(), nth(0, has_id(&paged_temporal_00999_id)));
    expect_that!(page_4.items(), nth(50, has_id(&paged_temporal_01047_id)));
    expect_that!(page_4.items(), nth(99, has_id(&paged_temporal_01096_id)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&paged_temporal_type)
        .limit(100)
        .offset(1950)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_5.total(), some(eq(2504)));
    expect_that!(page_5.items(), len(eq(100)));
    expect_that!(page_5.items(), nth(0, has_id(&paged_temporal_01946_id)));
    expect_that!(page_5.items(), nth(50, has_id(&paged_temporal_01996_id)));
    expect_that!(page_5.items(), nth(99, has_id(&paged_temporal_02045_id)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&paged_temporal_type)
        .limit(100)
        .offset(2450)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_6.total(), some(eq(2504)));
    expect_that!(page_6.items(), len(eq(54)));
    expect_that!(page_6.items(), nth(0, has_id(&paged_temporal_02446_id)));
    expect_that!(page_6.items(), nth(27, has_id(&paged_temporal_02473_id)));
    expect_that!(page_6.items(), nth(53, has_id(&paged_temporal_02499_id)));
    let page_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&paged_temporal_type)
        .limit(100)
        .offset(2500)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_7.total(), some(eq(2504)));
    expect_that!(page_7.items(), len(eq(4)));
    expect_that!(page_7.items(), nth(0, has_id(&paged_temporal_02496_id)));
    expect_that!(page_7.items(), nth(2, has_id(&paged_temporal_02498_id)));
    expect_that!(page_7.items(), nth(3, has_id(&paged_temporal_02499_id)));
    let page_8 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&paged_temporal_type)
        .limit(1000)
        .offset(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_8.total(), some(eq(2504)));
    expect_that!(page_8.items(), len(eq(1000)));
    expect_that!(page_8.items(), nth(0, has_id(&paged_temporal_00000_id)));
    expect_that!(page_8.items(), nth(500, has_id(&paged_temporal_00499_id)));
    expect_that!(page_8.items(), nth(999, has_id(&paged_temporal_00998_id)));
    let page_9 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&paged_temporal_type)
        .limit(1000)
        .offset(1000)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_9.total(), some(eq(2504)));
    expect_that!(page_9.items(), len(eq(1000)));
    expect_that!(page_9.items(), nth(0, has_id(&paged_temporal_00999_id)));
    expect_that!(page_9.items(), nth(500, has_id(&paged_temporal_01497_id)));
    expect_that!(page_9.items(), nth(999, has_id(&paged_temporal_01995_id)));
    let page_10 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&paged_temporal_type)
        .limit(1000)
        .offset(2000)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page_10.total(), some(eq(2504)));
    expect_that!(page_10.items(), len(eq(504)));
    expect_that!(page_10.items(), nth(0, has_id(&paged_temporal_01996_id)));
    expect_that!(page_10.items(), nth(252, has_id(&paged_temporal_02248_id)));
    expect_that!(page_10.items(), nth(503, has_id(&paged_temporal_02499_id)));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
