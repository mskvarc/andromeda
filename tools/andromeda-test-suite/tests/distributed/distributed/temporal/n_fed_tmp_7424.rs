//! A split query without a filter keeps every local evolution.

use andromeda_test_suite::{broker, context_source::ContextSource, identity_matcher::has_id, namespace::Namespace, outcome::Upserted, source_answer};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{
        OneOrMany,
        Property,
        Scope,
        evolution::{ScopeInstance, TemporalEntity},
    },
    query::{EntitySplitting, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Temporal Evolution of Entities with `splitEntities` true and no filter keeps every local Entity, in
/// the `temporalValues` representation and when its evolution holds a scope history
/// Clause: 5.7.4.4, 4.5.6, 4.5.9
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7424_a_split_query_without_a_filter_keeps_every_local_evolution() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(60).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_2_evolution = TemporalEntity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(60).observed_at("2020-08-01T12:00:00Z".parse()?))
        .scope(ScopeInstance::new(OneOrMany::one("/Madrid".parse::<Scope>()?)).with_observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_2_evolution).send().await?.created()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let source_evolutions = [];
    source
        .answer(
            Method::GET,
            "/a/ngsi-ld/v1/temporal/entities",
            source_answer::temporal_entities(&source_evolutions),
        )
        .await?;
    let page = broker
        .with_accept(Accept::any())
        .temporal()
        .query_values(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&vehicle_id)));
    expect_that!(page.items(), contains(has_id(&vehicle_2_id)));
    let page_2 = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    let scoped = page_2.items().iter().find(|item| item.id == vehicle_2_id).or_fail()?;
    expect_that!(&scoped.scope, len(gt(0)));
    Ok(())
}
