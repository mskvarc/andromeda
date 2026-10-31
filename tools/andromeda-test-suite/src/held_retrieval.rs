//! The world the federated aggregated retrieval tests share: four Vehicles held by the broker and three context sources
//! in different combinations, each holder's `speed` a [`HeldSeries`] of its own instants (clauses 4.5.19 and 5.7.3.4).
//!
//! Vehicle 1 is held by the broker and source `a`, Vehicle 2 by sources `a`, `b` and `c`, Vehicle 3 by the broker
//! alone and Vehicle 4 by source `a` alone. The broker holds the series of holder 2, source `a` that of holder 0 and
//! source `b` that of holder 1, so no two parts of one Vehicle share an instant.

use crate::{
    aggregation_matcher::{has_aggregate, period},
    broker::BrokerClient,
    context_source::ContextSource,
    held_series::HeldSeries,
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
        EntityId,
        EntityTypeName,
        NgsiLdMediaType,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::{IdPattern, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationId, RegistrationInfo},
};

/// The prefixes the three sources answer under, source `n` holding the series of holder `n`.
pub const SOURCES: [&str; 3] = ["a", "b", "c"];

/// The holder whose series the broker holds.
const BROKER_HOLDER: u32 = 2;

/// How many instances every part holds.
const INSTANCES: u32 = 30;

/// How many instances source `a`'s own aggregate of Vehicle 4 counts: fewer than the instances it answers, so an
/// answer counting them shows the aggregate kept and the instances never asked for.
pub const ALONE_AGGREGATED: u32 = 7;

/// How the sources answer a retrieval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceAnswers {
    /// An aggregated retrieval with the source's own aggregate, any other with every instance.
    AggregatesAndInstances,
    /// Every instance only; an aggregated retrieval is not answered.
    Instances,
}

/// The four Vehicles and the registrations of their sources.
#[derive(Debug)]
pub struct HeldVehicles {
    /// Vehicle 1, held by the broker and source `a`.
    pub broker_and_one_source: EntityId,
    /// Vehicle 2, held by every source and not by the broker.
    pub every_source: EntityId,
    /// Vehicle 3, held by the broker alone.
    pub broker_alone: EntityId,
    /// Vehicle 4, held by source `a` alone.
    pub one_source_alone: EntityId,
    /// The registration of every source.
    registrations: Vec<RegistrationId>,
}

/// Records the broker's parts on `broker`, registers the three parts of `source` for `retrieveTemporal` and stubs each
/// source's answers as `answers` says.
///
/// # Errors
///
/// Fails when a name does not parse, a write is not answered 201 or a registration is refused.
pub async fn hold_vehicles(broker: &BrokerClient, source: &ContextSource, namespace: &Namespace, answers: SourceAnswers) -> Result<HeldVehicles> {
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let held = HeldVehicles {
        broker_and_one_source: namespace.urn("Vehicle", 1)?,
        every_source: namespace.urn("Vehicle", 2)?,
        broker_alone: namespace.urn("Vehicle", 3)?,
        one_source_alone: namespace.urn("Vehicle", 4)?,
        registrations: Vec::new(),
    };
    for vehicle_id in [&held.broker_and_one_source, &held.broker_alone] {
        record(broker, vehicle_id, &vehicle_type).await?;
    }
    let mut registrations = Vec::new();
    for (number, prefix) in (1..).zip(SOURCES) {
        let registration_id = namespace.registration_id(number)?;
        let registration = CSourceRegistration::builder(source.endpoint(prefix)?)
            .id(registration_id.clone())
            .information([RegistrationInfo::builder()
                .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(format!("urn:ngsi-ld:Vehicle:{namespace}-.*").parse::<IdPattern>()?)])
                .build()?])
            .operations([DistributedOperation::RetrieveTemporal])
            .build()?;
        broker.registrations().register(&registration).send().await?;
        registrations.push(registration_id);
    }
    // Each Vehicle a source holds, the sources holding it and how many instances each source's own aggregate counts.
    let parts = [
        (&held.broker_and_one_source, &SOURCES[..1], INSTANCES),
        (&held.every_source, &SOURCES[..], INSTANCES),
        (&held.one_source_alone, &SOURCES[..1], ALONE_AGGREGATED),
    ];
    for (vehicle_id, holders, aggregated) in parts {
        for (holder, prefix) in (0..).zip(holders) {
            let series = HeldSeries::new(vehicle_id, &vehicle_type, holder, INSTANCES);
            let aggregated = HeldSeries::new(vehicle_id, &vehicle_type, holder, aggregated);
            stub(source, &retrieval_path(prefix, vehicle_id), &series, &aggregated, answers).await?;
        }
    }
    Ok(HeldVehicles { registrations, ..held })
}

/// Records the broker's series of `vehicle_id` on `broker` through Create Or Update Temporal Evolution.
async fn record(broker: &BrokerClient, vehicle_id: &EntityId, vehicle_type: &EntityTypeName) -> Result<()> {
    let series = HeldSeries::new(vehicle_id, vehicle_type, BROKER_HOLDER, INSTANCES);
    let mut evolution = TemporalEntity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone());
    for (value, instant) in series.observations() {
        evolution = evolution.property("speed", Property::builder(value).observed_at(instant.parse()?));
    }
    broker.temporal().create_or_update(&evolution.build()?).send().await?.created()?;
    Ok(())
}

/// Stubs the answers of `source` to a retrieval of `request_path` as `answers` says: `aggregated`'s aggregate and
/// `series`'s instances.
async fn stub(source: &ContextSource, request_path: &str, series: &HeldSeries, aggregated: &HeldSeries, answers: SourceAnswers) -> Result<()> {
    let json = HeaderValue::from_static("application/json");
    match answers {
        SourceAnswers::AggregatesAndInstances => {
            source
                .answer_when(
                    SourceRequest::new(Method::GET, request_path).query("format", "aggregatedValues"),
                    source_answer::text(StatusCode::OK, json.clone(), &aggregated.aggregates()),
                )
                .await?;
        }
        SourceAnswers::Instances => {}
    }
    source
        .answer_when(
            SourceRequest::new(Method::GET, request_path).without_query("format"),
            source_answer::text(StatusCode::OK, json, &series.instances()),
        )
        .await?;
    Ok(())
}

/// The path source `prefix` is asked to retrieve `vehicle_id` under.
#[must_use]
pub fn retrieval_path(prefix: &str, vehicle_id: &EntityId) -> String {
    format!("/{prefix}/ngsi-ld/v1/temporal/entities/{vehicle_id}")
}

/// Expects the aggregated retrieval of `vehicle_id` from `broker` to count `count` `speed` instances summing to `sum`,
/// each over one whole-range period.
///
/// # Errors
///
/// Fails when the retrieval is not answered with an aggregated temporal entity.
pub async fn expect_aggregate(broker: &BrokerClient, vehicle_id: &EntityId, count: u32, sum: u32) -> Result<()> {
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_aggregated(vehicle_id)
        .temporal_q(temporal_query::after("2020-01-01T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::TotalCount, AggregationMethod::Sum]))
        .send()
        .await?;
    let aggregated = retrieved.body();
    expect_that!(
        aggregated,
        has_aggregate("speed", AggregationMethod::TotalCount, all!(len(eq(1)), nth(0, period().value(count))))
    );
    expect_that!(
        aggregated,
        has_aggregate("speed", AggregationMethod::Sum, all!(len(eq(1)), nth(0, period().value(sum))))
    );
    Ok(())
}

/// Deletes the registrations of `held` from `broker`.
///
/// # Errors
///
/// Fails when a deletion is refused.
pub async fn release(broker: &BrokerClient, held: &HeldVehicles) -> Result<()> {
    for registration_id in &held.registrations {
        broker.registrations().delete(registration_id).send().await?;
    }
    Ok(())
}
