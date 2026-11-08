//! With a configured api root prefix every pointer of a listing starts with its path.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, SuiteConfig},
    hosted_context::HostedContext,
    link_header::link_target,
    namespace::Namespace,
    problem::answered,
    raw_call::target,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::{Accept, RelationType},
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: On a broker whose configured apiRoot carries the path prefix /broker, Query Subscriptions, Query Context
/// Source Registration Subscriptions, Query Context Source Registrations and List @contexts with limit 1 over at
/// least two members each answer a next pointer starting with /broker/ngsi-ld/v1/ and the collection's path
/// Clause: 5.5.9, 6.2, 6.3.10
/// Recorded: Andromeda extension, not v1.9.1: `server.public_url` is the apiRoot, and a pointer starts with its path
/// (clause 6.2 allows a prefix string), so it resolves through the proxy that strips the prefix, as a Location does
#[gtest]
#[tokio::test]
async fn a_lim_pgp_9007_with_a_configured_api_root_prefix_every_pointer_of_a_listing_starts_with_its_path() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker_prefixed = broker::client(BrokerRole::Prefixed)?;
    let config = SuiteConfig::get()?;
    let unreachable_notify = config.unreachable_origin().join("notify")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let bus_type = namespace.entity_type("Bus")?;
    let json = broker_prefixed.with_heads().with_accept(Accept::new(NgsiLdMediaType::Json));
    let root = "/broker/ngsi-ld/v1";

    let subscription_ids = [namespace.subscription_id(1)?, namespace.subscription_id(2)?];
    for subscription_id in &subscription_ids {
        let subscription = Subscription::builder()
            .id(subscription_id.clone())
            .entities([vehicle_type.as_str()])
            .notification(NotificationParams::builder(Endpoint::builder(unreachable_notify.clone()).build()?).build()?)
            .build()?;
        broker_prefixed.subscriptions().create(&subscription).send().await?;
    }
    let subscriptions = json.subscriptions().query().limit(1).send().await?;
    expect_that!(
        link_target(subscriptions.head.headers(), &RelationType::Next)?,
        some(starts_with(format!("{root}/subscriptions?")))
    );

    let registration_subscription_ids = [namespace.subscription_id(3)?, namespace.subscription_id(4)?];
    for subscription_id in &registration_subscription_ids {
        let subscription = Subscription::registration_builder()
            .id(subscription_id.clone())
            .entities([bus_type.as_str()])
            .notification(NotificationParams::builder(Endpoint::builder(unreachable_notify.clone()).build()?).build()?)
            .build()?;
        broker_prefixed.registration_subscriptions().create(&subscription).send().await?;
    }
    let registration_subscriptions = json.registration_subscriptions().query().limit(1).send().await?;
    expect_that!(
        link_target(registration_subscriptions.head.headers(), &RelationType::Next)?,
        some(starts_with(format!("{root}/csourceSubscriptions?")))
    );

    let registration_ids = [namespace.registration_id(1)?, namespace.registration_id(2)?];
    for registration_id in &registration_ids {
        let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
            .id(registration_id.clone())
            .information([RegistrationInfo::builder()
                .entities([EntityInfo::new(bus_type.clone()).with_id_pattern(namespace.id_pattern("Bus")?)])
                .property_names(["name"])
                .build()?])
            .build()?;
        broker_prefixed.registrations().register(&registration).send().await?;
    }
    let registrations = json.registrations().query().entity_type(&bus_type).limit(1).send().await?;
    expect_that!(
        link_target(registrations.head.headers(), &RelationType::Next)?,
        some(starts_with(format!("{root}/csourceRegistrations?")))
    );

    let mut hosted = Vec::with_capacity(2);
    for term in ["first", "second"] {
        let context = Context::Single(ContextEntry::Inline(
            format!(r#"{{"{term}_{namespace}": "https://example.org/{namespace}/{term}"}}"#).parse::<InlineContext>()?,
        ));
        hosted.push(HostedContext::add(&broker_prefixed, &context).await?);
    }
    // The typed List @contexts takes no limit, so the request is sent as written.
    let request = request_to(&broker_prefixed, Method::GET, &target("jsonldContexts", &[("limit", "1")]))?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = broker_prefixed.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let contexts = answer?;
    expect_that!(
        link_target(contexts.headers(), &RelationType::Next)?,
        some(starts_with(format!("{root}/jsonldContexts?")))
    );

    for context in &hosted {
        broker_prefixed.contexts().delete(&context.id).send().await?;
    }
    for registration_id in &registration_ids {
        broker_prefixed.registrations().delete(registration_id).send().await?;
    }
    for subscription_id in &registration_subscription_ids {
        broker_prefixed.registration_subscriptions().delete(subscription_id).send().await?;
    }
    for subscription_id in &subscription_ids {
        broker_prefixed.subscriptions().delete(subscription_id).send().await?;
    }
    Ok(())
}
