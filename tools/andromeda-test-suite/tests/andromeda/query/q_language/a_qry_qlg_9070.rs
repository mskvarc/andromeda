//! A q making more than the comparison limit is too complex in every carrier.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, query_carriers::Carriers};
use googletest::prelude::*;

/// Pins: a q making 1000 comparisons (one Query Term comparing with a list of 1000 values) is served by a GET entity
/// query and a temporal query and creates a Subscription, and one making 1001 is answered 403 `TooComplexQuery` naming
/// the count in a GET and a POST entity query, a temporal query and a Subscription create, which then creates nothing
/// Clause: 4.9, 5.5.6, 6.3.2, 5.7.2.4, 5.8.1.4
/// Recorded: ETSI GS CIM 009 sets no ceiling on a q; Andromeda counts every Query Term as one comparison and a value
/// list as one per value, under `request_limits.max_q_comparisons` (1000), the stack running the defaults
#[gtest]
#[tokio::test]
async fn a_qry_qlg_9070_a_q_making_more_than_the_comparison_limit_is_too_complex_in_every_carrier() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let car_type = namespace.entity_type("Car")?.to_string();
    let comparisons = |count: usize| format!("speed=={}", (0..count).map(|value| value.to_string()).collect::<Vec<_>>().join(","));
    // One Query Term comparing with a list of values is one comparison per value; the list is far past what the `q` model
    // is asked to write, so every carrier is sent the text as written.
    let at_the_limit = comparisons(1000);
    let served = Carriers {
        broker: &broker,
        entity_type: &car_type,
        q: &at_the_limit,
    };
    let at_id = namespace.subscription_id("list-at")?;
    served.expect_served(&at_id, &sink.endpoint("list-at")?).await?;
    let over_the_limit = comparisons(1001);
    let refused = Carriers {
        broker: &broker,
        entity_type: &car_type,
        q: &over_the_limit,
    };
    refused
        .expect_refused(&namespace.subscription_id("list-over")?, &sink.endpoint("list-over")?, "1001 comparisons")
        .await?;
    broker.subscriptions().delete(&at_id).send().await?;
    Ok(())
}
