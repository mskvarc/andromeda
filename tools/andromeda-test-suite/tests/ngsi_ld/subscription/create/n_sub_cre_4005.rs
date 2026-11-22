//! A scope query holding the bare any-scope wildcard inside a group is bad request data and stores nothing.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription with the `scopeQ` `(/#;/CompanyA)` is refused with `BadRequestData` and stores nothing
/// Clause: 4.19, 5.8.1.4, Table 5.2.12-1
/// Recorded: clause 4.19 writes the bare `/#` only as the whole expression, `ScopesQ =/ %x2F %23`
#[gtest]
#[tokio::test]
async fn n_sub_cre_4005_a_scope_query_holding_the_bare_any_scope_wildcard_inside_a_group_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_id = namespace.subscription_id("any-scope-in-group")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid query language text at scopeQ), so `scopeQ` is written as text.
    let scope_q: RawJson = r#""(/#;/CompanyA)""#.parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("scopeQ", &scope_q);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
