//! A local context naming a term twice is bad request data.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{headers::Accept, operation::ContextKindFilter},
    model::ErrorType,
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Add @context with a local context naming one term twice is refused as `BadRequestData` without a Location
/// and stores nothing
/// Clause: 5.13.2.4, 5.5.4, RFC 8259 s4
/// Tenant: private (the case asserts what the tenant's whole @context store lists, and clause 4.14 keeps each
/// tenant's stored @contexts its own; in the shared tenant every other file's entries are listed too)
/// Recorded: IETF RFC 8259 section 4 leaves the meaning of an object with duplicate names to the receiver;
/// Andromeda refuses a local context naming a term twice as data that is not syntactically correct (clause 5.5.4)
#[gtest]
#[tokio::test]
async fn o_ctx_add_0007_a_local_context_naming_a_term_twice_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    // The body names a member twice, which the model cannot write, so it is sent as written.
    let request = request_to(&tenant, Method::POST, "jsonldContexts")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\"@context\":{{\"A1\":\"https://test.andromeda/{namespace}/A1\",\"A1\":\"https://test.andromeda/{namespace}/A2\"}}}}"
        )));
    let answer = tenant.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).without_header(HeaderName::from_static("location"))
    );
    let contexts = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::Hosted)
        .send()
        .await?;
    expect_that!(contexts, len(eq(0)));
    Ok(())
}
