//! A rooted pattern selects a registration scope without the slash.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, nth::nth, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{NgsiLdMediaType, RawJson},
    query::{IdPattern, scope_query::ScopeQuery},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations with `scopeQ=/T<ns>` answers the registration whose scope is T<ns>
/// Clause: 4.18, 4.19, 5.10.2
/// Recorded: 4.18 makes the leading slash of a Scope optional ("Madrid" is its example 2) and 4.19 writes every
/// pattern with one; Andromeda reads a Scope written without the slash as the same levels, so a rooted pattern
/// selects it
#[gtest]
#[tokio::test]
async fn o_csr_qry_9008_a_rooted_pattern_selects_a_registration_scope_without_the_slash() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_bare_id = namespace.registration_id("bare")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_bare = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_bare_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .build()?;
    // The model refuses it (a registration scope), so `scope` is written as text.
    let scope: RawJson = format!(r#"["T{namespace}"]"#).parse()?;
    let document = UncheckedDocument::try_from(registration_bare.render())?.set_member("scope", &scope);
    let request = broker
        .unchecked(&RegisterContextSource {
            registration: &registration_bare,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_bare_cleanup = Cleanup::of(&broker).registration(&registration_bare_id);
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .scope_q(format!("/T{namespace}").as_str().parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&registration_bare_id)));
    broker.registrations().delete(&registration_bare_id).send().await?;
    Ok(())
}
