//! A query of several selectors reaches only the registrations matching one.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    context_source::ContextSource,
    eventually::retried,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    source_answer,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, NgsiLdMediaType},
    query::{EntitySelector, IdPattern, builders::SelectorTypeBuilder},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities through POST with selectors of two types, in a tenant whose one registration covers a third
/// type, answers the two local Entities alone, forwards nothing to that registration's source and leaves its
/// `timesSent` unset
/// Clause: 5.7.2.4, 5.12, 5.2.33
/// Tenant: private (the registration under test is the only one that may answer the query; in the shared tenant the
/// registrations of other files would answer it too)
#[gtest]
#[tokio::test]
async fn n_fed_qry_9019_a_query_of_several_selectors_reaches_only_the_registrations_matching_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let alpha_id = namespace.urn("Alpha", 1)?;
    let alpha_type = namespace.entity_type("Alpha")?;
    let beta_id = namespace.urn("Beta", 1)?;
    let beta_type = namespace.entity_type("Beta")?;
    let other_id = namespace.urn("Other", 1)?;
    let other_type = namespace.entity_type("Other")?;
    let registration_id = namespace.registration_id(1)?;
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    let alpha = Entity::builder()
        .id(alpha_id.clone())
        .entity_type(alpha_type.clone())
        .property("name", "alpha")
        .build()?;
    tenant.entities().create(&alpha).send().await?.done()?;
    let beta = Entity::builder()
        .id(beta_id.clone())
        .entity_type(beta_type.clone())
        .property("name", "beta")
        .build()?;
    tenant.entities().create(&beta).send().await?.done()?;
    let source_entities = [Entity::output_builder()
        .id(other_id)
        .entity_type(other_type.clone())
        .property("name", "other")
        .build()?];
    source
        .answer(Method::GET, "/other/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("other")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(other_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    tenant.registrations().register(&registration).send().await?;
    let _registration_cleanup = Cleanup::of(&tenant).registration(&registration_id);
    let selectors = [
        EntitySelector::new(SelectorTypeBuilder::from(&alpha_type).build()?),
        EntitySelector::new(SelectorTypeBuilder::from(&beta_type).build()?),
    ];
    let page = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .selectors(selectors)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&alpha_id)));
    expect_that!(page.items(), contains(has_id(&beta_id)));
    let forwarded = source.received_on(Method::GET, "/other/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(0)));
    retried(async || {
        let registration_2 = tenant
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .registrations()
            .retrieve(&registration_id)
            .send()
            .await?;
        verify_that!(registration_2.output().times_sent, none())?;
        Ok(())
    })
    .await?;
    tenant.registrations().delete(&registration_id).send().await?;
    Ok(())
}
