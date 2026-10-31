//! A split query forwards the type and the temporal query only.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, forwarded_query::query_values, namespace::Namespace, source_answer};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::geometry::{Position, Shape},
    query::{EntitySplitting, geoquery, q, scope_query::ScopeQuery, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Temporal Evolution of Entities with `splitEntities` true, q, georel, geometry, coordinates and
/// `scopeQ` is forwarded to the source with the type and the temporal query only
/// Clause: 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7421_a_split_query_forwards_the_type_and_the_temporal_query_only() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
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
        .query(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .geo_q(geoquery::near_max(1000, Shape::Point(Position::try_new(8.0, 44.0)?)))
        .entity_type(&vehicle_type)
        .q(q::attr("speed").gt(50))
        .scope_q("/Madrid".parse::<ScopeQuery>()?)
        .split_entities(EntitySplitting::Split)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/temporal/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(query_values(forwarded_one, "type"), not(is_empty()));
    expect_that!(query_values(forwarded_one, "timerel"), not(is_empty()));
    expect_that!(query_values(forwarded_one, "timeAt"), not(is_empty()));
    expect_that!(query_values(forwarded_one, "q"), is_empty());
    expect_that!(query_values(forwarded_one, "georel"), is_empty());
    expect_that!(query_values(forwarded_one, "geometry"), is_empty());
    expect_that!(query_values(forwarded_one, "coordinates"), is_empty());
    expect_that!(query_values(forwarded_one, "scopeQ"), is_empty());
    Ok(())
}
