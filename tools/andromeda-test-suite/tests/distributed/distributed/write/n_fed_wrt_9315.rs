//! A scoped purge reaches the registrations whose scope may hold a match.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, outcome::Done, source_answer, source_request::SourceRequest};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::{OneOrMany, Scope},
    query::scope_query::ScopeQuery,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Purge Entities by type with a scopeQ under redirect registrations advertising purgeEntity, one in the queried scope, one in another and one without a scope, answers 204 and forwards a purge to the first and the unscoped source, not the second
/// Clause: 5.6.21.4, 5.12, 4.3.6.7
/// Tenant: private (it counts every request its context sources receive, and a purge forwarded to every
/// registration of the tenant reaches the sources of other tests)
#[gtest]
#[tokio::test]
async fn n_fed_wrt_9315_a_scoped_purge_reaches_the_registrations_whose_scope_may_hold_a_match() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let scope_query = format!("/{namespace}/A");
    for (number, endpoint, scope) in [
        (1, "a", Some(format!("/{namespace}/A"))),
        (2, "b", Some(format!("/{namespace}/B"))),
        (3, "c", None),
    ] {
        let registration = CSourceRegistration::builder(source.endpoint(endpoint)?)
            .id(namespace.registration_id(number)?)
            .information([RegistrationInfo::builder()
                .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
                .build()?])
            .mode(RegistrationMode::Redirect)
            .operations([DistributedOperation::PurgeEntity, DistributedOperation::RetrieveEntity])
            .maybe_scope(scope.map(|scope| scope.parse::<Scope>().map(OneOrMany::one)).transpose()?)
            .build()?;
        broker.registrations().register(&registration).send().await?;
        source
            .answer(
                Method::DELETE,
                &format!("/{endpoint}/ngsi-ld/v1/entities"),
                source_answer::status(StatusCode::NO_CONTENT),
            )
            .await?;
    }
    broker
        .entities()
        .purge()
        .entity_type(&vehicle_type)
        .scope_q(scope_query.parse::<ScopeQuery>()?)
        .send()
        .await?
        .done()?;
    let asked_a = source
        .received_matching(&SourceRequest::any_matching("/a/ngsi-ld/v1/(entities|entityOperations|temporal).*"))
        .await?;
    expect_that!(asked_a, len(eq(1)));
    let asked_b = source
        .received_matching(&SourceRequest::any_matching("/b/ngsi-ld/v1/(entities|entityOperations|temporal).*"))
        .await?;
    expect_that!(asked_b, len(eq(0)));
    // Clause 4.3.6.7: a registration without scope may hold a match, and purges only what the scope query matches.
    let asked_c = source
        .received_matching(&SourceRequest::any_matching("/c/ngsi-ld/v1/(entities|entityOperations|temporal).*"))
        .await?;
    expect_that!(asked_c, len(eq(1)));
    let purged_c_with_scope = source
        .received_matching(&SourceRequest::new(Method::DELETE, "/c/ngsi-ld/v1/entities").query("scopeQ", &scope_query))
        .await?;
    expect_that!(purged_c_with_scope, len(eq(1)));
    Ok(())
}
