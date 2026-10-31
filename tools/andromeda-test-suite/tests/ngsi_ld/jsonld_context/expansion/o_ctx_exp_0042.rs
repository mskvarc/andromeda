//! A subscription naming a reference cycle is refused.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace, problem::problem, remote_context::RemoteContexts};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ErrorType, NgsiLdMediaType},
    registration::{CSourceRegistration, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription as application/ld+json naming a reference cycle is refused as `BadRequestData`, and
/// the tenant holds no subscription
/// Clause: 5.5.6, 5.8.1, JSON-LD 1.1 API s4.1.2
/// Tenant: private (the case asserts the tenant holds no subscription, and clause 4.14 keeps subscriptions the
/// tenant's own)
/// Recorded: clause 5.5.6 makes a remote @context that is invalid `BadRequestData`; Andromeda reads a @context
/// whose references never end (the JSON-LD 1.1 API context overflow) as invalid
#[gtest]
#[tokio::test]
async fn o_ctx_exp_0042_a_subscription_naming_a_reference_cycle_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let tenant = broker.with_tenant(namespace.tenant()?);
    let registration_provision_id = namespace.registration_id("provision")?;
    let sentinel_type = namespace.entity_type("Sentinel")?;
    let served = contexts.url("")?;
    let tenant_body_context = tenant
        .with_context(Context::Multiple(vec![contexts.reference("jc/cycle/a.jsonld")?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let device_type = namespace.entity_type("Device")?;
    let registration_provision = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_provision_id.clone())
        .information([RegistrationInfo::builder().entities([sentinel_type.as_str()]).build()?])
        .build()?;
    tenant.registrations().register(&registration_provision).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_provision_cleanup = Cleanup::of(&tenant).registration(&registration_provision_id);
    tenant.registrations().delete(&registration_provision_id).send().await?;
    contexts
        .serve(
            "jc/user/terms.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "Device_{namespace}": "https://test.andromeda/{namespace}/Device",
        "speed": "https://test.andromeda/{namespace}/speed"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "jc/cycle/b.jsonld",
            &format!(
                r#"{{
      "@context": "{served}jc/cycle/a.jsonld"
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "jc/cycle/a.jsonld",
            &format!(
                r#"{{
      "@context": [
        "{served}jc/user/terms.jsonld",
        "{served}jc/cycle/b.jsonld"
      ]
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let subscription = Subscription::builder()
        .entities([device_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let answer = tenant_body_context.with_responses().subscriptions().create(&subscription).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page = tenant.with_accept(Accept::any()).subscriptions().query().send().await?;
    expect_that!(page.items(), len(eq(0)));
    Ok(())
}
