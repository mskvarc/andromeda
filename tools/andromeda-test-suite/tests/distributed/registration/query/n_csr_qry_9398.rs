//! Registration discovery by scope query excludes an unscoped registration.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, OneOrMany, Scope},
    query::scope_query::ScopeQuery,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations by type with `scopeQ` /Madrid over a /Madrid and an unscoped
/// registration of the type answers the /Madrid one only
/// Clause: 5.10.2.4, 4.19
#[gtest]
#[tokio::test]
async fn n_csr_qry_9398_registration_discovery_by_scope_query_excludes_an_unscoped_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let origin = SuiteConfig::get()?.unreachable_origin();
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let madrid_id = namespace.registration_id("madrid")?;
    let anywhere_id = namespace.registration_id("anywhere")?;
    // A registration of one type and no identifier is asked by every retrieval by identifier of the tenant, so one
    // left behind answers other tests' retrievals with a cooldown warning.
    let _registration_cleanup = Cleanup::of(&broker).registration(&madrid_id).registration(&anywhere_id);
    let information = || -> Result<RegistrationInfo> {
        Ok(RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?)
    };
    let madrid = CSourceRegistration::builder(origin.as_str())
        .id(madrid_id.clone())
        .information([information()?])
        .scope(OneOrMany::one("/Madrid".parse::<Scope>()?))
        .build()?;
    broker.registrations().register(&madrid).send().await?;
    let anywhere = CSourceRegistration::builder(origin.as_str())
        .id(anywhere_id.clone())
        .information([information()?])
        .build()?;
    broker.registrations().register(&anywhere).send().await?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .scope_q("/Madrid".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&madrid_id)));
    Ok(())
}
