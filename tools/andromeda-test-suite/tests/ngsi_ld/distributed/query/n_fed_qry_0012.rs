//! With split entities scope q is applied after the merge and not forwarded.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    forwarded_query::query_values,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::Entity,
    query::{EntitySplitting, scope_query::ScopeQuery},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Entities by type with `splitEntities` true and `scopeQ` /Barcelona over a local Entity scoped
/// /Madrid and a source answering Entities scoped /Barcelona and /Madrid answers only the remote one scoped
/// /Barcelona, and the forwarded query carries no `scopeQ`
/// Clause: 5.7.2.4, 4.19, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_fed_qry_0012_with_split_entities_scope_q_is_applied_after_the_merge_and_not_forwarded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .scope("/Madrid")
        .property("speed", 50)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let source_entities = [
        Entity::output_builder()
            .id(vehicle_2_id.clone())
            .entity_type(vehicle_type.clone())
            .scope("/Barcelona")
            .property("speed", 50)
            .build()?,
        Entity::output_builder()
            .id(vehicle_3_id.clone())
            .entity_type(vehicle_type.clone())
            .scope("/Madrid")
            .property("speed", 50)
            .build()?,
    ];
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities").query_containing("type", format!("Vehicle_{namespace}")),
            source_answer::entities(&source_entities),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .scope_q("/Barcelona".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&vehicle_2_id)));
    expect_that!(page.items(), not(contains(has_id(&vehicle_id))));
    expect_that!(page.items(), not(contains(has_id(&vehicle_3_id))));
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(query_values(forwarded_one, "scopeQ"), is_empty());
    Ok(())
}
