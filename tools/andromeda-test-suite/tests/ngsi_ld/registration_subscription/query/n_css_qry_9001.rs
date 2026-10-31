//! A written json ld context reads back on retrieval and in a query.

use andromeda_test_suite::{
    broker,
    core_context::core_context_list,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Context Source Registration Subscription with `jsonldContext` naming a reachable @context answers
/// 201, and both Retrieve and Query Context Source Registration Subscriptions answer that `jsonldContext`
/// Clause: 5.11.2.4, 5.11.4.5, 5.11.5.4, Table 5.2.12-1
/// Tenant: private (Query Context Source Registration Subscriptions answers every subscription of the tenant, so in
/// the shared tenant the subscriptions of other files fill the answer)
#[gtest]
#[tokio::test]
async fn n_css_qry_9001_a_written_json_ld_context_reads_back_on_retrieval_and_in_a_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let contexts = RemoteContexts::start().await?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let fleet = contexts
        .serve(
            "ctx/fleet.jsonld",
            &format!(r#"{{"@context": {{"fleet": "https://example.org/{namespace}/fleet#"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .jsonld_context(fleet.clone())
        .build()?;
    let core_in_body = tenant.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    core_in_body.registration_subscriptions().create(&subscription).send().await?;
    let retrieved = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(retrieved.jsonld_context(), some(eq(&fleet)));
    let page = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .query()
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&subscription_id)));
    expect_that!(page.items().first().and_then(Subscription::jsonld_context), some(eq(&fleet)));
    tenant.registration_subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
