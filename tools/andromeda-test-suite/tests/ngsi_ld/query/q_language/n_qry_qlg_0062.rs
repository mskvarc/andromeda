//! A stop window pages the same whether its stop holds few stop times or many, its matches near or sparse.

use andromeda_test_suite::{broker, identity_matcher::ids_in_order, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, EntityId, Input, NgsiLdMediaType},
    query::{Locality, q},
};

/// How many stop times the test writes.
const STOP_TIMES: usize = 3000;

/// The stop a stop time calls at.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stop {
    /// The stop 30 of the stop times call at.
    Rare,
    /// The stop every other stop time calls at.
    Common,
}

impl Stop {
    /// The stop stop time `n` calls at.
    const fn of(n: usize) -> Stop {
        if n % 100 == 7 { Stop::Rare } else { Stop::Common }
    }

    /// The suffix of the stop's identifier.
    const fn suffix(self) -> &'static str {
        match self {
            Stop::Rare => "rare",
            Stop::Common => "common",
        }
    }
}

/// The departure time of stop time `n`, within the morning hour for each stop time at the rare stop and for every
/// fortieth at the common stop, and before it for the others.
const fn departure(n: usize) -> &'static str {
    match Stop::of(n) {
        Stop::Rare => "07:15:00",
        Stop::Common if n % 40 == 1 => "07:30:00",
        Stop::Common => "05:00:00",
    }
}

/// Whether `departure` lies within the morning hour the window asks for.
fn in_the_morning(departure: &str) -> bool {
    ("07:00:00".."08:00:00").contains(&departure)
}

/// The identifiers, in order, of the stop times calling at `stop` whose departure `kept` keeps.
fn calling(namespace: &Namespace, stop: Stop, kept: fn(&str) -> bool) -> Result<Vec<EntityId>> {
    Ok((1..=STOP_TIMES)
        .filter(|n| Stop::of(*n) == stop && kept(departure(*n)))
        .map(|n| namespace.urn("StopTime", format!("{n:04}")))
        .collect::<Result<Vec<_>, _>>()?)
}

/// Pins: Query Entities over 3,000 stop times with q hasStop=="<stop>";departureTime>="07:00:00";departureTime<"08:00:00"
/// answers in identifier order, with its total, the 30 stop times of a stop 30 of them call at, the first 50 and the 20
/// past offset 30 of the 75 a stop 2,970 of them call at holds one in forty apart, and the last 25 of those through the
/// next link; q hasStop=="<common stop>";departureTime<"06:00:00" answers the first 50 of the 2,895 it holds
/// Clause: 4.9, 5.5.9, 6.3.13
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0062_a_stop_window_pages_the_same_whether_its_stop_holds_few_stop_times_or_many() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let stop_time_type = namespace.entity_type("StopTime")?;
    let entities = (1..=STOP_TIMES)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("StopTime", format!("{n:04}"))?)
                .entity_type(stop_time_type.clone())
                .relationship("hasStop", namespace.urn("Stop", Stop::of(n).suffix())?)
                .property("departureTime", departure(n))
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    for batch in entities.chunks(1000) {
        broker.entities().batch_create(batch).send().await?.done()?;
    }
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let morning_at = |stop: Stop| {
        q::attr("hasStop")
            .eq(namespace.urn_text("Stop", stop.suffix()).as_str())
            .and(q::attr("departureTime").ge("07:00:00"))
            .and(q::attr("departureTime").lt("08:00:00"))
            .build()
    };

    let rare = json
        .entities()
        .query()
        .entity_type(&stop_time_type)
        .q(morning_at(Stop::Rare)?)
        .limit(50)
        .offset(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(rare.total(), some(eq(30)));
    expect_that!(rare.items(), ids_in_order(&calling(&namespace, Stop::Rare, in_the_morning)?));

    let common_ids = calling(&namespace, Stop::Common, in_the_morning)?;
    let first = json
        .entities()
        .query()
        .entity_type(&stop_time_type)
        .locality(Locality::Local)
        .q(morning_at(Stop::Common)?)
        .limit(50)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(first.total(), some(eq(75)));
    expect_that!(first.items(), ids_in_order(common_ids.get(..50).unwrap_or_default()));
    let rest = json.send(&first.next_page().or_fail()?).await?;
    expect_that!(rest.items(), ids_in_order(common_ids.get(50..).unwrap_or_default()));
    expect_that!(rest.next(), none());

    let past_offset = json
        .entities()
        .query()
        .entity_type(&stop_time_type)
        .q(morning_at(Stop::Common)?)
        .limit(20)
        .offset(30)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(past_offset.total(), some(eq(75)));
    expect_that!(past_offset.items(), ids_in_order(common_ids.get(30..50).unwrap_or_default()));

    let early = json
        .entities()
        .query()
        .entity_type(&stop_time_type)
        .q(q::attr("hasStop")
            .eq(namespace.urn_text("Stop", Stop::Common.suffix()).as_str())
            .and(q::attr("departureTime").lt("06:00:00"))
            .build()?)
        .limit(50)
        .offset(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    let early_ids = calling(&namespace, Stop::Common, |departure| departure < "06:00:00")?;
    expect_that!(early.total(), some(eq(2895)));
    expect_that!(early.items(), ids_in_order(early_ids.get(..50).unwrap_or_default()));
    Ok(())
}
