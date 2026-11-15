//! A declared time value is a time value wherever it sits in an Entity's value.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    q_parity::{QCase, QRequest, expect_query_and_subscription_alike},
    value::object,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{CoreDatatype, Entity, EntityId, EntityTypeName, Input, NgsiLdMediaType, Property, TypedValue, Value},
    query::{q::Query, temporal_query},
};
use std::collections::HashSet;

/// When every value of a Clock is observed, so a temporal query's window holds each one.
const OBSERVED: &str = "2020-01-01T12:00:00Z";

/// A Clock whose `since`, `list` and `held` hold the values given, each observed at [`OBSERVED`].
fn clock(id: &EntityId, clock_type: &EntityTypeName, since: Value, list: Value, held: Value) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(id.clone())
        .entity_type(clock_type.clone())
        .property("since", Property::builder(since).observed_at(OBSERVED.parse()?))
        .property("list", Property::builder(list).observed_at(OBSERVED.parse()?))
        .property("held", Property::builder(held).observed_at(OBSERVED.parse()?))
        .build()?)
}

/// A value declared as the `DateTime` `instant`.
fn date_time(instant: &str) -> Result<Value> {
    Ok(Value::Typed(TypedValue::try_new(CoreDatatype::DateTime, instant)?))
}

/// Pins: over a Clock whose `since` is the `DateTime` 2024-01-01T00:00:00Z, whose `list` holds it beside the String
/// "x" and whose structured `held` holds it as its member `m`, a Clock holding the `DateTime` 2024-02-01T00:00:00Z
/// in the same three places beside "y", and a Clock holding the undeclared text "2024-01-01T00:00:00Z" in the same
/// three places beside "z", Query Entities, a Subscription and Query Temporal Evolution of Entities with each q select
/// alike: since==2024-01-01T00:00:00Z the first, since>2024-01-15T00:00:00Z the second, since!=2024-01-01T00:00:00Z
/// the second and third, list==2024-01-01T00:00:00Z the first, list!=2024-01-01T00:00:00Z the second and third,
/// list==2024-02-01T00:00:00Z,"x" the first two, list=="2024-01-01T00:00:00Z" the third,
/// held[m]==2024-01-01T00:00:00Z the first, held[m]>2024-01-15T00:00:00Z the second, held[m]!=2024-01-01T00:00:00Z
/// the second and third and held[m]=="2024-01-01T00:00:00Z" the third; a value declared as a time value is that time
/// value as a Property's value, as an element of an array value and as a member of a structured value, and an
/// undeclared text is a String wherever it sits
/// Clause: 3.1, 4.6.3, 4.9, 5.7.2, 5.7.4, 5.8.1, C.6
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9202_a_declared_time_value_is_a_time_value_wherever_it_sits_in_an_entity_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let clock_type = namespace.entity_type("Clock")?;
    let early_id = namespace.urn("Clock", "early")?;
    let late_id = namespace.urn("Clock", "late")?;
    let text_id = namespace.urn("Clock", "text")?;
    let text = || Value::from("2024-01-01T00:00:00Z");
    let entities = [
        clock(
            &early_id,
            &clock_type,
            date_time("2024-01-01T00:00:00Z")?,
            Value::from(vec![date_time("2024-01-01T00:00:00Z")?, Value::from("x")]),
            object([("m", date_time("2024-01-01T00:00:00Z")?)]),
        )?,
        clock(
            &late_id,
            &clock_type,
            date_time("2024-02-01T00:00:00Z")?,
            Value::from(vec![date_time("2024-02-01T00:00:00Z")?, Value::from("y")]),
            object([("m", date_time("2024-02-01T00:00:00Z")?)]),
        )?,
        clock(
            &text_id,
            &clock_type,
            text(),
            Value::from(vec![text(), Value::from("z")]),
            object([("m", text())]),
        )?,
    ];
    let cases = [
        QCase {
            q: "since==2024-01-01T00:00:00Z",
            selects: &[&early_id],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "since>2024-01-15T00:00:00Z",
            selects: &[&late_id],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "since!=2024-01-01T00:00:00Z",
            selects: &[&late_id, &text_id],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "list==2024-01-01T00:00:00Z",
            selects: &[&early_id],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "list!=2024-01-01T00:00:00Z",
            selects: &[&late_id, &text_id],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "list==2024-02-01T00:00:00Z,\"x\"",
            selects: &[&early_id, &late_id],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "list==\"2024-01-01T00:00:00Z\"",
            selects: &[&text_id],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "held[m]==2024-01-01T00:00:00Z",
            selects: &[&early_id],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "held[m]>2024-01-15T00:00:00Z",
            selects: &[&late_id],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "held[m]!=2024-01-01T00:00:00Z",
            selects: &[&late_id, &text_id],
            request: QRequest::PLAIN,
        },
        QCase {
            q: "held[m]==\"2024-01-01T00:00:00Z\"",
            selects: &[&text_id],
            request: QRequest::PLAIN,
        },
    ];
    expect_query_and_subscription_alike(&broker, &namespace, &clock_type, &entities, &cases).await?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    for case in &cases {
        let page = json
            .temporal()
            .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
            .entity_type(&clock_type)
            .q(case.q.parse::<Query>()?)
            .send()
            .await?;
        let evolved: HashSet<EntityId> = page.items().iter().map(|evolution| evolution.id.clone()).collect();
        let expected: HashSet<EntityId> = case.selects.iter().map(|&id| id.clone()).collect();
        expect_that!(evolved, eq(&expected), "Query Temporal Evolution of Entities with q={}", case.q);
    }
    Ok(())
}
