//! A q nesting deeper than the nesting limit is too complex in every carrier.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, query_carriers::Carriers};
use googletest::prelude::*;

/// Pins: a q whose ; and | combinations nest 16 levels is served by a GET entity query and a temporal query and creates
/// a Subscription, and one nesting 17 is answered 403 `TooComplexQuery` naming the depth in a GET and a POST entity
/// query, a temporal query and a Subscription create, which then creates nothing
/// Clause: 4.9, 5.5.6, 6.3.2, 5.7.2.4, 5.8.1.4
/// Recorded: ETSI GS CIM 009 sets no ceiling on a q; Andromeda counts each level of nested ; and | combinations under
/// `request_limits.max_q_nesting` (16), the stack running the defaults
#[gtest]
#[tokio::test]
async fn a_qry_qlg_9071_a_q_nesting_deeper_than_the_nesting_limit_is_too_complex_in_every_carrier() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let car_type = namespace.entity_type("Car")?.to_string();
    // `c<top>>1|(c<top - 1>>1|(... c1>1|(a>1;b>1)))` nests `top + 1` levels, which the `q` model is not asked to write.
    let nested = |top: usize| (1..=top).fold("a>1;b>1".to_owned(), |inner, level| format!("c{level}>1|({inner})"));
    let at_the_limit = nested(15);
    let served = Carriers {
        broker: &broker,
        entity_type: &car_type,
        q: &at_the_limit,
    };
    let at_id = namespace.subscription_id("nest-at")?;
    served.expect_served(&at_id, &sink.endpoint("nest-at")?).await?;
    let over_the_limit = nested(16);
    let refused = Carriers {
        broker: &broker,
        entity_type: &car_type,
        q: &over_the_limit,
    };
    refused
        .expect_refused(&namespace.subscription_id("nest-over")?, &sink.endpoint("nest-over")?, "nested 17 levels deep")
        .await?;
    broker.subscriptions().delete(&at_id).send().await?;
    Ok(())
}
