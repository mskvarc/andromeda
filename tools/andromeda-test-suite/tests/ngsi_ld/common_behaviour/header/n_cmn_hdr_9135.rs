//! The Location of a Context Source Registration Subscription names its id percent-encoded as one path segment.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    followed_location::{identifier, request},
    namespace::Namespace,
    problem::answered,
    segment_identifier::segment_identifiers,
    wire::header_value,
};
use googletest::prelude::*;
use http::{StatusCode, header::LOCATION};
use ngsi_ld::subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId};

/// Pins: Create Context Source Registration Subscription with an id holding /, ?, #, %41 or a non-ASCII character
/// answers a Location ending in /ngsi-ld/v1/csourceSubscriptions/ and the id percent-encoded as one path segment, and
/// a GET on that Location answers the subscription with that id
/// Clause: 6.12.3.1, 6.13.3.1
/// Recorded: IETF RFC 3986 section 3.3 admits only pchar in a path segment, so the relative path names the created
/// subscription only with every other octet of its id percent-encoded, % included
#[gtest]
#[tokio::test]
async fn n_cmn_hdr_9135_the_location_of_a_registration_subscription_names_its_id_percent_encoded_as_one_path_segment() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let unreachable_notify = SuiteConfig::get()?.unreachable_origin().join("notify")?;
    let bus_type = namespace.entity_type("Bus")?;
    for segment_identifier in segment_identifiers(&namespace, "Subscription") {
        let subscription_id = SubscriptionId::verbatim(&segment_identifier.id)?;
        let subscription = Subscription::registration_builder()
            .id(subscription_id.clone())
            .entities([bus_type.as_str()])
            .notification(NotificationParams::builder(Endpoint::builder(unreachable_notify.clone()).build()?).build()?)
            .build()?;
        let created = broker.with_responses().registration_subscriptions().create(&subscription).send().await?;
        expect_that!(
            header_value(created.head.headers(), &LOCATION),
            some(ends_with(format!("/ngsi-ld/v1/csourceSubscriptions/{}", segment_identifier.segment)))
        );
        let answer = broker.send_unchecked(request(&broker, created.head.headers())?).await;
        expect_that!(&answer, answered(StatusCode::OK));
        expect_that!(identifier(&answer?)?, eq(subscription_id.as_uri()));
        broker.registration_subscriptions().delete(&subscription_id).send().await?;
    }
    Ok(())
}
