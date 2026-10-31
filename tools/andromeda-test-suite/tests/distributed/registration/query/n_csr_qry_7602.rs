//! A scope query selects registrations by their scope.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::PeerRole, identity_matcher::has_id, namespace::Namespace, nth::nth, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::RawJson,
    query::scope_query::ScopeQuery,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations by type with `scopeQ` /Madrid/# returns the registration scoped
/// /Madrid/Gardens and not the one scoped /Berlin/Parks
/// Clause: 5.10.2.4, 5.12, 4.19
#[gtest]
#[tokio::test]
async fn n_csr_qry_7602_a_scope_query_selects_registrations_by_their_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern_ending("Vehicle", "1-.*")?)])
            .build()?])
        .build()?;
    // The model refuses it (a registration scope), so `scope` is written as text.
    let scope: RawJson = r#"["/Madrid/Gardens"]"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("scope", &scope);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern_ending("Vehicle", "2-.*")?)])
            .build()?])
        .build()?;
    // The model refuses it (a registration scope), so `scope` is written as text.
    let scope_2: RawJson = r#"["/Berlin/Parks"]"#.parse()?;
    let document_2 = UncheckedDocument::try_from(registration_2.render())?.set_member("scope", &scope_2);
    let request_2 = broker
        .unchecked(&RegisterContextSource { registration: &registration_2 })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let page = broker
        .with_accept(Accept::any())
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .scope_q("/Madrid/#".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&registration_id)));
    let page_2 = broker
        .with_accept(Accept::any())
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .scope_q("/Madrid/#".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&registration_id)));
    Ok(())
}
