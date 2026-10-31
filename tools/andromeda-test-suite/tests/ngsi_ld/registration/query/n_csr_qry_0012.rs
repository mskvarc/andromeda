//! Scope q matches registration scopes by the scope query language.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{EntityTypeName, NgsiLdMediaType, OneOrMany, Output, Scope},
    query::{IdPattern, scope_query::ScopeQuery},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations by type and `scopeQ` answers 200 with the registrations whose scope
/// matches: /Madrid only the exact path, /Madrid/+ one level below, /Madrid/# the path and every descendant,
/// /Berlin its own; a registered scope reads back as written
/// Clause: 5.10.2, 4.18, 4.19, 5.2.9
#[gtest]
#[tokio::test]
async fn n_csr_qry_0012_scope_q_matches_registration_scopes_by_the_scope_query_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let device_type = format!("https://test.andromeda/{namespace}/Device").parse::<EntityTypeName>()?;
    let ids = [namespace.registration_id(1)?, namespace.registration_id(2)?, namespace.registration_id(3)?];
    for (id, scope) in ids.iter().zip(["/Madrid", "/Madrid/Gardens", "/Berlin"]) {
        let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
            .id(id.clone())
            .information([RegistrationInfo::builder()
                .entities([EntityInfo::new(device_type.clone())
                    .with_id_pattern(format!("urn:ngsi-ld://test.andromeda/{namespace}/Device:{namespace}-.*").parse::<IdPattern>()?)])
                .build()?])
            .scope(OneOrMany::try_from(vec![scope.parse::<Scope>()?])?)
            .build()?;
        broker.registrations().register(&registration).send().await?;
    }
    let [registration_1_id, registration_2_id, registration_3_id] = &ids;
    for (scope_q, expected) in [
        ("/Madrid", vec![registration_1_id]),
        ("/Madrid/+", vec![registration_2_id]),
        ("/Madrid/#", vec![registration_1_id, registration_2_id]),
        ("/Berlin", vec![registration_3_id]),
    ] {
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .registrations()
            .query()
            .entity_type(&device_type)
            .scope_q(scope_q.parse::<ScopeQuery>()?)
            .send()
            .await?;
        let found = page.items().iter().map(CSourceRegistration::<Output>::id).collect::<Vec<_>>();
        expect_that!(found, len(eq(expected.len())));
        for id in expected {
            expect_that!(found, contains(eq(&id)));
        }
    }
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(registration_2_id)
        .send()
        .await?;
    // Clause 4.18 allows one scope written bare or as a one-element array, and the test accepts either.
    let gardens = "/Madrid/Gardens".parse::<Scope>()?;
    expect_that!(retrieved.scope().map(OneOrMany::as_slice), some(elements_are![eq(&gardens)]));
    Ok(())
}
