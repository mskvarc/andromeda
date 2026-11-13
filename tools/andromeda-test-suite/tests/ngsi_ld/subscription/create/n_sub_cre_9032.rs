//! A q string holding a raw control character is bad request data.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// The JSON escapes of line feed, tab, backspace, form feed, carriage return and U+0001: escaped in the request
/// body, each reaches the q text as the raw character.
const JSON_ESCAPES: [&str; 6] = [r"\n", r"\t", r"\b", r"\f", r"\r", r"\u0001"];

/// Pins: Create Subscription whose q text is name=="a", a raw line feed, tab, backspace, form feed, carriage return
/// or U+0001, then b" answers 400 `BadRequestData` and stores nothing
/// Clause: 4.9, 5.5.4, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_9032_a_q_string_holding_a_raw_control_character_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let building_type = namespace.entity_type("Building")?;
    for (index, json_escape) in JSON_ESCAPES.into_iter().enumerate() {
        let subscription_id = namespace.subscription_id(index)?;
        let subscription = Subscription::builder()
            .id(subscription_id.clone())
            .entities([building_type.as_str()])
            .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        // The model refuses a raw control character in a q, so `q` is written as text.
        let q: RawJson = format!(r#""name==\"a{json_escape}b\"""#).parse()?;
        let document = UncheckedDocument::try_from(subscription.render())?.set_member("q", &q);
        let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData), "escape {json_escape}");
        let stored = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&subscription_id)
            .send()
            .await;
        expect_that!(&stored, answered(StatusCode::NOT_FOUND), "escape {json_escape}");
    }
    Ok(())
}
