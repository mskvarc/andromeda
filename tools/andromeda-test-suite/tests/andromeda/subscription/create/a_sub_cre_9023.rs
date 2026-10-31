//! An unknown member of a subscription body is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription naming a member outside Tables 5.2.12-1, 5.2.14.1-1 and 5.2.15-1, at the top level, in
/// notification or in endpoint, answers 400 `BadRequestData` naming the member
/// Clause: 5.2.12, 5.2.14.1, 5.2.15, 5.8.1.4
/// Recorded: no clause refuses an extra Subscription member, so this is stricter than the specification asks; the
/// Subscription data type has no place to keep one, and a misspelt `watchedAttributes` would otherwise become a
/// subscription watching every attribute
#[gtest]
#[tokio::test]
async fn a_sub_cre_9023_an_unknown_member_of_a_subscription_body_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_unknown_member_0_id = namespace.subscription_id("unknown-member-0")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_unknown_member_0 = Subscription::builder()
        .id(subscription_unknown_member_0_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("unknown-member-0")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (member `watchedAtributes` is not allowed here), so `watchedAtributes` is written as text.
    let watched_atributes: RawJson = r#"["name"]"#.parse()?;
    let document = UncheckedDocument::try_from(subscription_unknown_member_0.render())?.set_member("watchedAtributes", &watched_atributes);
    let request = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_unknown_member_0,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("watchedAtributes")
    );
    let endpoint = sink.endpoint("unknown-member-1")?;
    // The model refuses this subscription, so it is sent as written.
    let request_2 = request_to(&broker, Method::POST, "subscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-unknown-member-1\",\n    \"type\": \"Subscription\",\n    \"entities\": [\n        {{\n            \"type\": \"Building_{namespace}\"\n        }}\n    ],\n    \"notification\": {{\n        \"formt\": \"normalized\",\n        \"endpoint\": {{\n            \"uri\": \"{endpoint}\",\n            \"accept\": \"application/json\"\n        }}\n    }}\n}}")));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("formt")
    );
    let endpoint_2 = sink.endpoint("unknown-member-2")?;
    // The model refuses this subscription, so it is sent as written.
    let request_3 = request_to(&broker, Method::POST, "subscriptions")?.append_header(CONTENT_TYPE, HeaderValue::from_static("application/json")).body(Bytes::from(format!("{{\n    \"id\": \"urn:ngsi-ld:Subscription:{namespace}-unknown-member-2\",\n    \"type\": \"Subscription\",\n    \"entities\": [\n        {{\n            \"type\": \"Building_{namespace}\"\n        }}\n    ],\n    \"notification\": {{\n        \"endpoint\": {{\n            \"uri\": \"{endpoint_2}\",\n            \"protcol\": \"http\"\n        }}\n    }}\n}}")));
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(
        &answer_3,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("protcol")
    );
    Ok(())
}
