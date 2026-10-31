//! An aggregate of an entity three sources hold counts the instances of every part.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    context_source::ContextSource,
    held_series::{HeldSeries, answer_of},
    namespace::Namespace,
    nth::nth,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, evolution::AggregationMethod},
    query::{IdPattern, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Temporal Evolution of Entities as aggregatedValues totalCount and sum over three sources each holding
/// 30 instances of one Vehicle at instants of their own, and one source alone holding a second Vehicle, under the
/// default `federated_aggregation` (`overlapping-instances`): the shared Vehicle counts 90 instances summing to 10305,
/// the second keeps its one holder's aggregate, and each source is asked again for the instances of the shared
/// Vehicle alone
/// Clause: 5.7.4.4, 4.5.19
/// Recorded: the broker asks the sources for their aggregates first and aggregates here only the entities more than
/// one part answered
#[gtest]
#[tokio::test]
async fn a_fed_tmp_9027_an_aggregate_of_an_entity_three_sources_hold_counts_every_part() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let shared_id = namespace.urn("Vehicle", 1)?;
    let alone_id = namespace.urn("Vehicle", 2)?;
    let holders = ["a", "b", "c"];
    let mut registrations = Vec::new();
    for (number, prefix) in (1..).zip(holders) {
        let registration_id = namespace.registration_id(number)?;
        let registration = CSourceRegistration::builder(source.endpoint(prefix)?)
            .id(registration_id.clone())
            .information([RegistrationInfo::builder()
                .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(format!("urn:ngsi-ld:Vehicle:{namespace}-.*").parse::<IdPattern>()?)])
                .build()?])
            .operations([DistributedOperation::QueryTemporal])
            .build()?;
        broker.registrations().register(&registration).send().await?;
        registrations.push(registration_id);
    }
    let json = HeaderValue::from_static("application/json");
    for (holder, prefix) in (0..).zip(holders) {
        let shared = HeldSeries::new(&shared_id, &vehicle_type, holder, 30);
        // The first source's aggregate of the second Vehicle counts 7 although its instances would count 30, so an
        // answer of 7 shows the aggregate kept and the instances never asked for.
        let aggregates = match holder {
            0 => answer_of(&[shared.aggregates(), HeldSeries::new(&alone_id, &vehicle_type, holder, 7).aggregates()]),
            _ => answer_of(&[shared.aggregates()]),
        };
        let request_path = format!("/{prefix}/ngsi-ld/v1/temporal/entities");
        source
            .answer_when(
                SourceRequest::new(Method::GET, &request_path).query("format", "aggregatedValues"),
                source_answer::text(StatusCode::OK, json.clone(), &aggregates),
            )
            .await?;
        source
            .answer_when(
                SourceRequest::new(Method::GET, &request_path).without_query("format"),
                source_answer::text(StatusCode::OK, json.clone(), &answer_of(&[shared.instances()])),
            )
            .await?;
    }
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query_aggregated(temporal_query::after("2020-01-01T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::TotalCount, AggregationMethod::Sum]))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    let shared = page.items().iter().find(|item| item.id == shared_id).or_fail()?;
    expect_that!(
        shared,
        has_aggregate("speed", AggregationMethod::TotalCount, all!(len(eq(1)), nth(0, period().value(90))))
    );
    expect_that!(
        shared,
        has_aggregate("speed", AggregationMethod::Sum, all!(len(eq(1)), nth(0, period().value(10_305))))
    );
    let alone = page.items().iter().find(|item| item.id == alone_id).or_fail()?;
    expect_that!(
        alone,
        has_aggregate("speed", AggregationMethod::TotalCount, all!(len(eq(1)), nth(0, period().value(7))))
    );
    for prefix in holders {
        let asked_again = source
            .received_matching(&SourceRequest::new(Method::GET, &format!("/{prefix}/ngsi-ld/v1/temporal/entities")).without_query("format"))
            .await?;
        expect_that!(asked_again, len(eq(1)));
        let asked_for: Vec<String> = asked_again
            .iter()
            .flat_map(|request| request.url.query_pairs().filter(|(name, _)| name == "id").map(|(_, value)| value.into_owned()))
            .collect();
        expect_that!(asked_for, elements_are![eq(&shared_id.to_string())]);
    }
    for registration_id in &registrations {
        broker.registrations().delete(registration_id).send().await?;
    }
    Ok(())
}
