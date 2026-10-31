//! An empty fragment moves no modified at.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, problem::answered, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Update Context Source Registration with the empty Fragment {} leaves the registration's `modifiedAt` as it
/// was
/// Clause: 4.8, 5.4, 5.9.3
/// Recorded: an empty write modifies nothing, so nothing has a new modification time (clause 4.8)
#[gtest]
#[tokio::test]
async fn n_csr_upd_2702_an_empty_fragment_moves_no_modified_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .description("before")
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let before = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let modified = before.output().modified_at;
    expect_that!(modified, some(anything()));
    // The empty fragment carries no member, which no typed fragment writes, so it is sent as written.
    let request = json_request_to(&broker, Method::PATCH, &format!("csourceRegistrations/{registration_id}"), "{}")?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let after = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    expect_that!(after.description(), some(eq("before")));
    expect_that!(after.output().modified_at, eq(modified));
    Ok(())
}
