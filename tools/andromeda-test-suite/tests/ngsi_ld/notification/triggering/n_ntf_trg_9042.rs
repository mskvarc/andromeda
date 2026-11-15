//! A periodic notification carries what Query Entities answers.

use andromeda_test_suite::{broker, instant_ahead::minutes_ahead, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{
        DateTime,
        Entity,
        Lossless,
        NgsiLdMediaType,
        Property,
        geometry::{Position, Shape},
    },
    query::{
        EntitySelector,
        Join,
        Locality,
        builders::SelectorTypeBuilder,
        geoquery::{self, GeoQuery},
        q::{self, Query},
        scope_query::ScopeQuery,
    },
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// One periodic Subscription, the Query Entities request asking for the same Entities, and the Entities both answer.
struct Case {
    /// The Subscription's name, and the endpoint its notifications reach.
    name: &'static str,
    /// The Subscription's `entities`, the query's selectors.
    selectors: Vec<EntitySelector>,
    /// The `q` of both.
    q: Option<Query>,
    /// The `geoQ` of the Subscription, the geoquery of the query.
    geo_q: Option<GeoQuery>,
    /// The `scopeQ` of both.
    scope_q: Option<ScopeQuery>,
    /// The `join` the query asks for, which a `q` crossing a Relationship needs to reach the Linked Entity.
    join: Option<Join>,
    /// The identifiers both answer, sorted.
    expected: Vec<String>,
}

/// Pins: For each of a `q`, a `q` complement over an instance whose `expiresAt` passed a minute ago, a `q` through a
/// Relationship, a `geoQ` near, a `scopeQ`, no filter, two selectors and an `idPattern`, the first notification of a
/// Subscription with `timeInterval` 1 carries exactly the Entities Query Entities answers for the same selectors and
/// filters, and those are the Entities the clauses select
/// Clause: 4.9, 4.10, 4.19, 4.22, 5.2.33, 5.7.2, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9042_a_periodic_notification_carries_what_query_entities_answers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let parking_type = namespace.entity_type("Parking")?;
    let vehicle_ids = [
        namespace.urn("Vehicle", 1)?,
        namespace.urn("Vehicle", 2)?,
        namespace.urn("Vehicle", 3)?,
        namespace.urn("Vehicle", 4)?,
    ];
    let parking_ids = [namespace.urn("Parking", 1)?, namespace.urn("Parking", 2)?];
    let a_minute_ago: DateTime = minutes_ahead(-1).parse()?;
    let heidelberg = || Position::try_new(8.6724, 49.3988);
    let entities = [
        Entity::builder()
            .id(parking_ids[0].clone())
            .entity_type(parking_type.clone())
            .property("category", "commercial")
            .build()?,
        Entity::builder()
            .id(parking_ids[1].clone())
            .entity_type(parking_type.clone())
            .property("category", "residential")
            .build()?,
        Entity::builder()
            .id(vehicle_ids[0].clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 20)
            .geo_property("location", Shape::Point(heidelberg()?))
            .scope("/Spain")
            .relationship("isParkedIn", parking_ids[0].clone())
            .build()?,
        Entity::builder()
            .id(vehicle_ids[1].clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 5)
            .geo_property("location", Shape::Point(Position::try_new(13.4050, 52.5200)?))
            .scope("/France")
            .relationship("isParkedIn", parking_ids[1].clone())
            .build()?,
        Entity::builder()
            .id(vehicle_ids[2].clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(30).expires_at(a_minute_ago))
            .build()?,
        Entity::builder()
            .id(vehicle_ids[3].clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 30)
            .build()?,
    ];
    for entity in &entities {
        broker.entities().create(entity).send().await?.done()?;
    }
    let vehicles = || -> Result<Vec<EntitySelector>> { Ok(vec![EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?)]) };
    let identified = |indices: &[usize]| -> Vec<String> { indices.iter().map(|&index| vehicle_ids[index].to_string()).collect() };
    let mut every_entity: Vec<String> = vehicle_ids.iter().chain(&parking_ids).map(ToString::to_string).collect();
    every_entity.sort();
    let cases = [
        // Clause 4.22: the third Vehicle's `speed` is invalid, so it is not greater than 10.
        Case {
            name: "plain",
            selectors: vehicles()?,
            q: Some(q::attr("speed").gt(10).build()?),
            geo_q: None,
            scope_q: None,
            join: None,
            expected: identified(&[0, 3]),
        },
        // Clause 4.9: an unequal term needs the target element, and clause 4.22 leaves the third Vehicle none.
        Case {
            name: "complement",
            selectors: vehicles()?,
            q: Some(q::attr("speed").ne(20).build()?),
            geo_q: None,
            scope_q: None,
            join: None,
            expected: identified(&[1, 3]),
        },
        Case {
            name: "hop",
            selectors: vehicles()?,
            q: Some(q::attr("isParkedIn").linked("category").eq("commercial").build()?),
            geo_q: None,
            scope_q: None,
            join: Some(Join::Inline),
            expected: identified(&[0]),
        },
        Case {
            name: "near",
            selectors: vehicles()?,
            q: None,
            geo_q: Some(geoquery::near_max(1000, Shape::Point(heidelberg()?)).build()?),
            scope_q: None,
            join: None,
            expected: identified(&[0]),
        },
        Case {
            name: "scope",
            selectors: vehicles()?,
            q: None,
            geo_q: None,
            scope_q: Some("/Spain".parse::<ScopeQuery>()?),
            join: None,
            expected: identified(&[0]),
        },
        // Clause 5.8.6: with no query, geoquery or Scope query every subscribed Entity is included, the third Vehicle
        // too, since only its `speed` expired.
        Case {
            name: "unfiltered",
            selectors: vehicles()?,
            q: None,
            geo_q: None,
            scope_q: None,
            join: None,
            expected: identified(&[0, 1, 2, 3]),
        },
        Case {
            name: "two-selectors",
            selectors: vec![
                EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?),
                EntitySelector::new(SelectorTypeBuilder::from(&parking_type).build()?),
            ],
            q: None,
            geo_q: None,
            scope_q: None,
            join: None,
            expected: every_entity,
        },
        Case {
            name: "id-pattern",
            selectors: vec![
                EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_id_pattern(namespace.id_pattern_ending("Vehicle", "[12]$")?),
            ],
            q: None,
            geo_q: None,
            scope_q: None,
            join: None,
            expected: identified(&[0, 1]),
        },
    ];
    for case in &cases {
        let subscription = Subscription::builder()
            .id(namespace.subscription_id(case.name)?)
            .entities(case.selectors.clone())
            .time_interval(1)
            .maybe_q(case.q.clone())
            .maybe_geo_q(case.geo_q.clone())
            .maybe_scope_q(case.scope_q.clone())
            .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(case.name)?).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
    }
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    for case in &cases {
        let notifications = sink.wait_for_at_least(case.name, 1).await?;
        let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
        let mut notified: Vec<String> = notification.data().iter().map(|entity| entity.id.to_string()).collect();
        notified.sort();
        let page = json
            .entities()
            .query()
            .method(QueryMethod::Post)
            .selectors(case.selectors.clone())
            .maybe_q(case.q.clone())
            .maybe_geo_q(case.geo_q.clone())
            .maybe_scope_q(case.scope_q.clone())
            .maybe_join(case.join)
            .locality(Locality::Local)
            .send()
            .await?;
        let mut answered: Vec<String> = page.items().iter().map(|entity| entity.id.to_string()).collect();
        answered.sort();
        expect_that!(&notified, eq(&answered), "{}", case.name);
        expect_that!(&answered, eq(&case.expected), "{}", case.name);
        broker.subscriptions().delete(&namespace.subscription_id(case.name)?).send().await?;
    }
    Ok(())
}
