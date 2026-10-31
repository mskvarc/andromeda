//! A non split query forwards its filters.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, forwarded_query::query_values, namespace::Namespace, source_answer};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::geometry::{Position, Shape},
    query::{EntitySplitting, geoquery, q, scope_query::ScopeQuery},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Entities with `splitEntities` false and q, georel, `scopeQ` and attrs is forwarded to the source
/// with the q, georel, geometry, coordinates, `scopeQ` and attrs it was given
/// Clause: 5.7.2.4
/// Recorded: clause 5.7.2.4 names the removal of the filters before forwarding only for split entities and does not
/// say what a non-split query forwards; Andromeda forwards the filters unchanged
#[gtest]
#[tokio::test]
async fn o_fed_qry_7415_a_non_split_query_forwards_its_filters() -> Result<()> {
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
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_entities = [];
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let page = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .geo_q(geoquery::near_max(1000, Shape::Point(Position::try_new(8.0, 44.0)?)))
        .entity_type(&vehicle_type)
        .q(q::attr("speed").gt(50))
        .scope_q("/Madrid".parse::<ScopeQuery>()?)
        .attrs(vec!["speed".parse()?])
        .split_entities(EntitySplitting::Whole)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(query_values(forwarded_one, "q"), not(is_empty()));
    expect_that!(query_values(forwarded_one, "georel"), not(is_empty()));
    expect_that!(query_values(forwarded_one, "geometry"), not(is_empty()));
    expect_that!(query_values(forwarded_one, "coordinates"), not(is_empty()));
    expect_that!(query_values(forwarded_one, "scopeQ"), not(is_empty()));
    expect_that!(query_values(forwarded_one, "attrs"), not(is_empty()));
    Ok(())
}
