//! A cursor beside an offset is bad request.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::answered, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Query Context Source Registration Subscriptions with a cursor and an offset answers 400, two pointers
/// governing one page
/// Clause: 5.5.9.1, 6.3.10
/// Tenant: private (Query Context Source Registration Subscriptions pages through every one of the tenant, so in
/// the shared tenant the subscriptions of other files fill the pages; clause 4.14 keeps each tenant's subscriptions
/// its own)
#[gtest]
#[tokio::test]
async fn a_css_qry_0010_a_cursor_beside_an_offset_is_bad_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let subscription_0000_id = namespace.subscription_id("0000")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_0000 = Subscription::registration_builder()
        .id(subscription_0000_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.registration_subscriptions().create(&subscription_0000).send().await?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &tenant.with_accept(Accept::new(NgsiLdMediaType::JsonLd)),
        Method::GET,
        "csourceSubscriptions?limit=2&cursor=zzzz&offset=0",
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
    let answer = tenant.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::BAD_REQUEST));
    Ok(())
}
