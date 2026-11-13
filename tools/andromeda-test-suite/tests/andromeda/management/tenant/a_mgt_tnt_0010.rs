//! A suspension refuses the tenant while a request admitted before it still holds the tenant.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    raw_call::andromeda_call,
    source_answer,
    wire::header_value,
};
use googletest::prelude::*;
use http::{HeaderName, Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, RegistrationManagementInfo},
};
use std::time::{Duration, Instant};

/// How late the context source answers the held retrieval: long enough for the suspension and the refused read to
/// happen while the broker still waits for it under the suite's full parallel load, and below the broker's request
/// timeout of 30 s, so the held retrieval is answered by the source rather than cut off.
const SOURCE_DELAY: Duration = Duration::from_secs(15);

/// The registration's forwarding timeout in milliseconds, which takes precedence over the broker's 5 s default: above
/// [`SOURCE_DELAY`], so the broker keeps waiting for the late answer.
const FORWARDING_TIMEOUT_MS: u32 = 25_000;

/// Pins: a tenant suspended while a Retrieve Entity admitted before the suspension still waits for its context
/// source is refused at once with 404 `NonexistentTenant` echoing the tenant, before that retrieval is answered;
/// once the retrieval is answered the tenant stays refused, and resuming it answers 204 and serves its data again
/// Clause: 5.5.10, 6.3.14
/// Tenant: private (the subject is suspending and resuming a whole tenant)
/// Recorded: Andromeda extension, not v1.9.1: tenant management (the /tenants resources), which clause 5.5.10
/// leaves implementation-specific ("How Tenants are created, is implementation-specific")
#[gtest]
#[tokio::test]
async fn a_mgt_tnt_0010_a_suspension_refuses_the_tenant_while_a_request_admitted_before_it_still_holds_the_tenant() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let private = broker::private_tenant(&front, &namespace).await?;
    let source = ContextSource::start().await?;
    let tenant_header = HeaderName::from_static("ngsild-tenant");
    let device_type = namespace.entity_type("Device")?;
    let local_id = namespace.urn("Device", "local")?;
    let remote_id = namespace.urn("Device", "remote")?;
    let registration_id = namespace.registration_id(1)?;
    let local = Entity::builder()
        .id(local_id.clone())
        .entity_type(device_type.clone())
        .property("reading", 1)
        .build()?;
    private.entities().create(&local).send().await?.done()?;
    let remote_path = format!("/a/ngsi-ld/v1/entities/{remote_id}");
    let remote = Entity::output_builder().id(remote_id.clone()).entity_type(device_type.clone()).build()?;
    source
        .answer(Method::GET, &remote_path, source_answer::entity(&remote).set_delay(SOURCE_DELAY))
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(device_type).with_id(remote_id.clone())])
            .build()?])
        .management(RegistrationManagementInfo::builder().timeout(FORWARDING_TIMEOUT_MS).build()?)
        .build()?;
    private.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&private).registration(&registration_id);
    // The Tenant resource is an Andromeda extension no NGSI-LD operation covers, so its requests are sent as written.
    let tenant_path = format!("tenants/{namespace}");
    let reader = private.with_responses().with_accept(Accept::new(NgsiLdMediaType::Json));
    let reads = reader.entities();
    let read_local = || reads.retrieve(&local_id).send();
    let held = async {
        let answer = reads.retrieve(&remote_id).send().await;
        (answer, Instant::now())
    };
    let suspension = async {
        source.wait_for_on(Method::GET, &remote_path, 1).await?;
        let suspend = andromeda_call(
            &front,
            Method::PATCH,
            &tenant_path,
            &[("Content-Type", "application/json")],
            Some(r#"{"isActive":false}"#),
        )
        .await?;
        let refused = read_local().await;
        Result::<_>::Ok((suspend, refused, Instant::now()))
    };
    let ((held_answer, held_answered_at), suspended) = tokio::join!(held, suspension);
    let (suspend, refused, refused_at) = suspended?;
    expect_that!(suspend.status(), eq(StatusCode::NO_CONTENT));
    expect_that!(
        &refused,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(tenant_header.clone(), &namespace.to_string())
    );
    expect_that!(refused_at, lt(held_answered_at));
    expect_that!(held_answer.answered_status(), some(anything()));
    let still_refused = read_local().await;
    expect_that!(
        &still_refused,
        problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant).with_header(tenant_header.clone(), &namespace.to_string())
    );
    let resume = andromeda_call(
        &front,
        Method::PATCH,
        &tenant_path,
        &[("Content-Type", "application/json")],
        Some(r#"{"isActive":true}"#),
    )
    .await?;
    expect_that!(resume.status(), eq(StatusCode::NO_CONTENT));
    let served = read_local().await?;
    expect_that!(header_value(served.head.headers(), &tenant_header), some(eq(namespace.to_string().as_str())));
    expect_that!(served.outcome.body(), has_id(&local_id));
    Ok(())
}
