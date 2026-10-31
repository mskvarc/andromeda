//! A non-split temporal scope query reaches an unscoped registration.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        OneOrMany,
        Property,
        Scope,
        evolution::{ScopeInstance, TemporalEntity},
    },
    query::{EntitySplitting, scope_query::ScopeQuery, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, RegistrationInfo},
};

/// Pins: Query Temporal Evolution of Entities by type with `scopeQ` /Madrid and `splitEntities` false over three
/// `queryTemporal` registrations of the type, one scoped /Madrid, one scoped /Barcelona and one declaring no scope,
/// is forwarded once to the /Madrid and once to the unscoped source with `scopeQ` /Madrid, never to the /Barcelona
/// source, and answers the Entities both returned
/// Clause: 5.7.4.4, 5.12, 4.3.6.7, Table 5.2.9-1
#[gtest]
#[tokio::test]
async fn n_fed_tmp_9398_a_non_split_temporal_scope_query_reaches_an_unscoped_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let madrid: Scope = "/Madrid".parse()?;
    let barcelona: Scope = "/Barcelona".parse()?;
    let mut cleanup = Cleanup::of(&broker);
    for (endpoint, vehicle_scope, registration_scope) in [
        ("madrid", &madrid, Some(&madrid)),
        ("barcelona", &barcelona, Some(&barcelona)),
        ("anywhere", &madrid, None),
    ] {
        let evolution = TemporalEntity::builder()
            .id(namespace.urn("Vehicle", endpoint)?)
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(60).observed_at("2020-08-01T12:00:00Z".parse()?))
            .scope(ScopeInstance::new(OneOrMany::one(vehicle_scope.clone())))
            .build()?;
        source
            .answer(
                Method::GET,
                &format!("/{endpoint}/ngsi-ld/v1/temporal/entities"),
                source_answer::temporal_entities(&[evolution]),
            )
            .await?;
        let registration_id = namespace.registration_id(endpoint)?;
        let registration = CSourceRegistration::builder(source.endpoint(endpoint)?)
            .id(registration_id.clone())
            .information([RegistrationInfo::builder().entities([vehicle_type.as_str()]).build()?])
            .operations([DistributedOperation::QueryTemporal])
            .maybe_scope(registration_scope.map(|scope| OneOrMany::one(scope.clone())))
            .build()?;
        broker.registrations().register(&registration).send().await?;
        // A forward to a registration left behind fails at its endpoint, and the cooldown that opens there holds
        // back a later test's registration naming the same endpoint.
        cleanup = cleanup.registration(&registration_id);
    }
    let page = broker
        .with_accept(Accept::any())
        .temporal()
        .query(temporal_query::after("2020-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .scope_q("/Madrid".parse::<ScopeQuery>()?)
        .split_entities(EntitySplitting::Whole)
        .send()
        .await?;
    expect_that!(page.items(), contains(has_id(&namespace.urn("Vehicle", "madrid")?)));
    expect_that!(page.items(), contains(has_id(&namespace.urn("Vehicle", "anywhere")?)));
    expect_that!(page.items(), not(contains(has_id(&namespace.urn("Vehicle", "barcelona")?))));
    // Clause 4.3.6.7: without split entities the source applies the scope query itself.
    for endpoint in ["madrid", "anywhere"] {
        let forwarded = source
            .received_matching(&SourceRequest::new(Method::GET, &format!("/{endpoint}/ngsi-ld/v1/temporal/entities")).query("scopeQ", "/Madrid"))
            .await?;
        expect_that!(forwarded, len(eq(1)));
        let all_forwarded = source.received_matching(&SourceRequest::any_matching(&format!("/{endpoint}/.*"))).await?;
        expect_that!(all_forwarded, len(eq(1)));
    }
    let barcelona_forwarded = source.received_matching(&SourceRequest::any_matching("/barcelona/.*")).await?;
    expect_that!(barcelona_forwarded, len(eq(0)));
    Ok(())
}
