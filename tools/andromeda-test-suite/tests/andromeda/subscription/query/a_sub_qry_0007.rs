//! A malformed cursor is bad request.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::answered, wire::request_to};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Query Subscriptions with a cursor the broker never issued answers 400
/// Clause: 5.5.9.1, 6.3.10
/// Tenant: private (Query Subscriptions pages through every subscription of the tenant, so in the shared tenant the
/// subscriptions of other files fill the pages; clause 4.14 keeps each tenant's subscriptions its own)
#[gtest]
#[tokio::test]
async fn a_sub_qry_0007_a_malformed_cursor_is_bad_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let subscription_0000_id = namespace.subscription_id("0000")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_0000 = Subscription::builder()
        .id(subscription_0000_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    tenant.subscriptions().create(&subscription_0000).send().await?;
    // The client cannot write a parameter of this read, so it is sent as written.
    let request = request_to(
        &tenant.with_accept(Accept::new(NgsiLdMediaType::Json)),
        Method::GET,
        "subscriptions?limit=2&cursor=zzzz",
    )?
    .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let answer = tenant.with_accept(Accept::new(NgsiLdMediaType::Json)).send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::BAD_REQUEST));
    Ok(())
}
