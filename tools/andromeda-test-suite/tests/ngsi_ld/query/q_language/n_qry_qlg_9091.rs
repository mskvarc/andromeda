//! A bare pattern runs to the end of its Query Term in a query and a subscription.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    problem::problem,
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityId, ErrorType, NgsiLdMediaType},
    query::q::{self, builders::QueryBuilder},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: Query Entities with a pattern written bare, as clause 4.9 writes a `RegExp`, reads the pattern to the first
/// `;` or `|` outside its groups and bracket expressions or to a `)` that closes none of them, keeps the whitespace
/// inside it and leaves the whitespace after it, and reads a quoted pattern and `["]` alike; a top-level alternation
/// left ungrouped and a quoted pattern without its closing quote are `BadRequestData`; a Subscription whose q holds a
/// bare pattern ending at the `)` of a group of terms is notified of the Entity the group selects
/// Clause: 4.9, 5.7.2.4, 5.8.1, 6.4.3.2; IEEE 1003.2
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9091_a_bare_pattern_runs_to_the_end_of_its_query_term_in_a_query_and_a_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let car_type = namespace.entity_type("Car")?;
    let subscription_id = namespace.subscription_id("bare-pattern")?;
    let mercedes_id = namespace.urn("Car", "mercedes")?;
    let merzedes_id = namespace.urn("Car", "merzedes")?;
    let mercedes_benz_id = namespace.urn("Car", "mercedes-benz")?;
    let bmw_id = namespace.urn("Car", "bmw")?;
    let semicolon_id = namespace.urn("Car", "semicolon")?;
    let quoted_id = namespace.urn("Car", "quoted")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([car_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .q(q::attr("speed")
            .lt(15)
            .or(q::attr("brandName").matches("^Mer(c|z)edes$"))
            .and(q::attr("speed").gt(50))
            .build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("bare-pattern")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let cars = [
        (&mercedes_id, "Mercedes", 40),
        (&merzedes_id, "Merzedes", 60),
        (&mercedes_benz_id, "Mercedes Benz", 70),
        (&bmw_id, "BMW", 80),
        (&semicolon_id, "a;b", 10),
        (&quoted_id, "\"Merc\"", 20),
    ];
    let mut entities = Vec::with_capacity(cars.len());
    for (id, brand_name, speed) in cars {
        entities.push(
            Entity::builder()
                .id(id.clone())
                .entity_type(car_type.clone())
                .property("brandName", brand_name)
                .property("speed", speed)
                .build()?,
        );
    }
    broker.entities().batch_create(&entities).send().await?.done()?;
    let selected = [BodyPattern::Contains(merzedes_id.to_string())];
    sink.wait_for_at_least_where("bare-pattern", &selected, 1).await?;
    let notifications = sink.received("bare-pattern").await?;
    expect_that!(notifications, len(eq(1)));
    let selections: [(QueryBuilder, Vec<&EntityId>); 11] = [
        (q::attr("brandName").matches("^Merc"), vec![&mercedes_id, &mercedes_benz_id]),
        (
            q::attr("brandName").not_matches("^Merc"),
            vec![&merzedes_id, &bmw_id, &semicolon_id, &quoted_id],
        ),
        (q::attr("brandName").matches("^Mer(c|z)edes$"), vec![&mercedes_id, &merzedes_id]),
        (q::attr("brandName").matches("(^Merc|^BMW)"), vec![&mercedes_id, &mercedes_benz_id, &bmw_id]),
        (q::attr("brandName").matches("[;]"), vec![&semicolon_id]),
        (q::attr("brandName").matches("Mercedes Benz"), vec![&mercedes_benz_id]),
        (q::attr("brandName").matches("[\"]Merc[\"]"), vec![&quoted_id]),
        (q::attr("brandName").matches("\"^Merc\""), vec![&mercedes_id, &mercedes_benz_id]),
        (q::attr("brandName").matches("^Merc ").and(q::attr("speed").gt(50)), vec![&mercedes_benz_id]),
        (q::attr("brandName").matches("^BMW").or(q::attr("speed").lt(15)), vec![&bmw_id, &semicolon_id]),
        (
            q::attr("speed")
                .lt(15)
                .or(q::attr("brandName").matches("^Mer(c|z)edes$"))
                .and(q::attr("speed").gt(50)),
            vec![&merzedes_id],
        ),
    ];
    for (filter, expected) in selections {
        let filter = filter.build()?;
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .query()
            .entity_type(&car_type)
            .q(filter.clone())
            .send()
            .await?;
        expect_that!(page.items(), len(eq(expected.len())), "{filter}");
        for id in expected {
            expect_that!(page.items(), contains(has_id(id)), "{filter}");
        }
    }
    let unterminated = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("brandName").matches("\"Merc").and(q::attr("speed").gt(5)))
        .send()
        .await;
    expect_that!(&unterminated, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    // The client refuses a pattern holding a top-level alternation, so the query is sent as written.
    let target = format!("entities?type=Car_{namespace}&q=brandName~%3D%5EMerc%7C%5EBMW");
    let ungrouped = broker.send_unchecked(request_to(&broker, Method::GET, &target)?).await;
    expect_that!(&ungrouped, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData), "{target}");
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
