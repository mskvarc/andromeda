//! A local part that fails q is no part of the aggregate of every instance the sources answered.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker,
    config::BrokerRole,
    context_source::ContextSource,
    held_series::{HeldSeries, answer_of},
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::{EntitySplitting, IdPattern, q, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Temporal Evolution of Entities with splitEntities false as aggregatedValues totalCount and sum, on a
/// broker whose `federated_aggregation` is `instances`, over one Vehicle the broker holds with the speed
/// instances 200, 201 and 202 and two sources each answer with 30 instances of their own: with q speed<150 the
/// broker's part fails and the Vehicle counts the sources' 60 instances summing to 3870, with q speed<300 it matches
/// and the Vehicle counts 63 summing to 4473, and with q speed>1000, which no source matches either, the Vehicle is
/// not in the result
/// Clause: 5.7.4.4, 4.5.19
/// Recorded: the broker asks the sources for every instance of the window in one round, and its own instances join
/// the aggregate it computes over them only when its local read kept the entity
#[gtest]
#[tokio::test]
async fn a_fed_tmp_9030_a_local_part_failing_q_is_no_part_of_an_aggregate_of_every_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::client(BrokerRole::Edge)?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    // The instants of a third holder's series, which neither source's series shares.
    let held = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(200).observed_at("2020-08-01T10:00:02Z".parse()?))
        .property("speed", Property::builder(201).observed_at("2020-08-01T10:00:05Z".parse()?))
        .property("speed", Property::builder(202).observed_at("2020-08-01T10:00:08Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&held).send().await?.created()?;
    let holders = ["a", "b"];
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
        let series = HeldSeries::new(&vehicle_id, &vehicle_type, holder, 30);
        let request_path = format!("/{prefix}/ngsi-ld/v1/temporal/entities");
        for bound in ["150", "300"] {
            source
                .answer_when(
                    SourceRequest::new(Method::GET, &request_path)
                        .query_containing("q", bound)
                        .without_query("format"),
                    source_answer::text(StatusCode::OK, json.clone(), &answer_of(&[series.instances()])),
                )
                .await?;
        }
        source
            .answer_when(
                SourceRequest::new(Method::GET, &request_path).query_containing("q", "1000"),
                source_answer::text(StatusCode::OK, json.clone(), &answer_of(&[])),
            )
            .await?;
    }
    let queries = [
        (q::attr("speed").lt(150), Some((60, 3870))),
        (q::attr("speed").lt(300), Some((63, 4473))),
        (q::attr("speed").gt(1000), None),
    ];
    for (query, aggregated) in queries {
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .temporal()
            .query_aggregated(
                temporal_query::after("2020-01-01T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::TotalCount, AggregationMethod::Sum]),
            )
            .entity_type(&vehicle_type)
            .split_entities(EntitySplitting::Whole)
            .q(query)
            .send()
            .await?;
        let Some((count, sum)) = aggregated else {
            expect_that!(page.items(), is_empty());
            continue;
        };
        expect_that!(page.items(), len(eq(1)));
        let vehicle = page.items().iter().find(|item| item.id == vehicle_id).or_fail()?;
        expect_that!(
            vehicle,
            has_aggregate("speed", AggregationMethod::TotalCount, all!(len(eq(1)), nth(0, period().value(count))))
        );
        expect_that!(
            vehicle,
            has_aggregate("speed", AggregationMethod::Sum, all!(len(eq(1)), nth(0, period().value(sum))))
        );
    }
    for registration_id in &registrations {
        broker.registrations().delete(registration_id).send().await?;
    }
    Ok(())
}
