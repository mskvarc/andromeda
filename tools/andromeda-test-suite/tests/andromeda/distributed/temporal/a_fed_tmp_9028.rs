//! Gathering every instance aggregates every entity a source answered.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    config::BrokerRole,
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

/// Pins: Query Temporal Evolution of Entities as aggregatedValues totalCount and sum, on a broker whose
/// `federation.query.aggregation` is `instances`, over three sources each holding 30 instances of one Vehicle at instants of
/// their own, and one source alone holding 5 instances of a second Vehicle: the shared Vehicle counts 90 instances
/// summing to 10305, the second counts its 5, and no source is asked for an aggregate
/// Clause: 5.7.4.4, 4.5.19
/// Recorded: the broker asks the sources for every instance of the window in one round and aggregates every entity
/// they answer here
#[gtest]
#[tokio::test]
async fn a_fed_tmp_9028_gathering_every_instance_aggregates_every_answered_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::client(BrokerRole::Edge)?;
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
        let instances = match holder {
            0 => answer_of(&[shared.instances(), HeldSeries::new(&alone_id, &vehicle_type, holder, 5).instances()]),
            _ => answer_of(&[shared.instances()]),
        };
        source
            .answer_when(
                SourceRequest::new(Method::GET, &format!("/{prefix}/ngsi-ld/v1/temporal/entities")).without_query("format"),
                source_answer::text(StatusCode::OK, json.clone(), &instances),
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
        has_aggregate("speed", AggregationMethod::TotalCount, all!(len(eq(1)), nth(0, period().value(5))))
    );
    expect_that!(
        alone,
        has_aggregate("speed", AggregationMethod::Sum, all!(len(eq(1)), nth(0, period().value(10))))
    );
    let aggregates_asked = source
        .received_matching(&SourceRequest::matching(Method::GET, "/[abc]/ngsi-ld/v1/temporal/entities").query("format", "aggregatedValues"))
        .await?;
    expect_that!(aggregates_asked, len(eq(0)));
    for registration_id in &registrations {
        broker.registrations().delete(registration_id).send().await?;
    }
    Ok(())
}
