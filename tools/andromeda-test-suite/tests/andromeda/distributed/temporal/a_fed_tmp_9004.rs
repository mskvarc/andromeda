//! An entity held on both sides is merged once.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_source::ContextSource,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Upserted,
    paged_federation::{local_evolutions, serve_temporal_pages},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::NgsiLdMediaType,
    query::{IdPattern, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Temporal Evolution of Entities at offset 2,490 over an inclusive registration whose source holds 2,500
/// Temporal Evolutions answered in pages of 1,000 linked by next, beside 14 the broker holds itself (4 interleaving
/// the source's identifiers, 10 shared with it) answers each of the 10 shared Entities once, with the broker's and
/// the source's attribute, and counts 2,504
/// Clause: 5.7.4.4, 4.5.5
#[gtest]
#[tokio::test]
async fn a_fed_tmp_9004_an_entity_held_on_both_sides_is_merged_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let paged_temporal_type = namespace.entity_type("PagedTemporal")?;
    let registration_id = namespace.registration_id(1)?;
    serve_temporal_pages(&source, &namespace).await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(paged_temporal_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    for evolution in &local_evolutions(&namespace)? {
        broker.temporal().create_or_update(evolution).send().await?.created()?;
    }
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&paged_temporal_type)
        .limit(20)
        .offset(2490)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(2504)));
    expect_that!(page.items(), len(eq(14)));
    for number in 2490..=2499_i64 {
        expect_that!(
            page.items(),
            contains(all!(
                has_id(&namespace.urn("PagedTemporal", format!("{number:05}"))?),
                has_evolution("local", contains(any_attribute().value(true))),
                has_evolution("remote", contains(any_attribute().value(number)))
            ))
            .times(eq(1))
        );
    }
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
