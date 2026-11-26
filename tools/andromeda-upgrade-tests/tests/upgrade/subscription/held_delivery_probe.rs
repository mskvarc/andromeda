//! A held Subscription's probe is its queued delivery of the earliest change,
//! whatever order its failures are recorded in: the claim offers that delivery
//! alone, an exhausted probe passes the hold to the delivery of the next
//! change one retry step later, and the upgrade brings every hold v1.0.0 left
//! to that rule.

use crate::fixtures::{
    deliveries::{insert_delivery, insert_event, insert_subscription},
    rehearsal::at_v1_0_0,
    statements::{insert, instant, name, read_column, uuid},
    test_error::UpgradeTestError,
};
use andromeda_configuration::schema::database::name_cache::NameCacheConfig;
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_notification::{
    delivery_backoff::{BackoffStep, DeliveryBackoff},
    delivery_claim::DeliveryClaim,
    delivery_claim_order::{DeliveryClaimOrder, InFlightCeiling},
    delivery_entry::DeliveryKey,
    delivery_lease::LeaseTerm,
    delivery_round::ClaimRequest,
    delivery_settlement::{DeliverySettlement, DeliveryVerdict},
    delivery_store::DeliveryStore,
    held_events::HeldEvents,
};
use andromeda_postgresql_context_store::notification_delivery::queue::NotificationDeliveryQueue;
use andromeda_postgresql_name_dictionary::name_cache::NameCache;
use andromeda_postgresql_seaquery::{
    cast::cast,
    clock::span_after_transaction_start,
    interval::make_interval_secs,
    pg_type::PostgresType,
    scalar_subquery::scalar_subquery,
    statement::Statement,
};
use andromeda_retry::schedule::BackoffSchedule;
use andromeda_upgrade_tests::upgrade_database::UpgradeDatabase;
use pretty_assertions::assert_eq;
use sea_query::{Expr, ExprTrait, Order, Query, SelectStatement, SimpleExpr};
use sqlx::PgPool;
use std::{num::NonZeroU32, sync::Arc, time::Duration};
use uuid::Uuid;

/// The worker every round of a test runs as.
const WORKER: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_00a1);

/// A worker other than the test's, holding a delivery in flight.
const OTHER_WORKER: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_0000_00a2);

/// The commit instants of the changes a test queues, earliest first.
const CHANGES: [&str; 3] = ["2025-01-27T21:00:01Z", "2025-01-27T21:00:02Z", "2025-01-27T21:00:03Z"];

/// The write each of [`CHANGES`] was committed by.
const WRITES: [&str; 3] = [
    "00000000-0000-4000-8000-0000000000e1",
    "00000000-0000-4000-8000-0000000000e2",
    "00000000-0000-4000-8000-0000000000e3",
];

/// The instant a delivery seeded set aside was set aside at.
const SET_ASIDE_AT: &str = "2025-01-27T21:00:05Z";

/// How long after an exhaustion the next probe of its Subscription is due.
const PROBE_AFTER: Duration = Duration::from_secs(5);

/// A tenant upgraded from v1.0.0 holding the Subscription and the events of
/// the first `changes` of [`CHANGES`], each the creation that its own write
/// committed; returns the database, the Subscription's key and the events'.
async fn with_changes(changes: usize) -> Result<(UpgradeDatabase, i64, Vec<i64>), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    let subscription = insert_subscription(database.pool()).await?;
    let events = insert_events(database.pool(), changes).await?;
    database.migrate_to_current().await?;
    Ok((database, subscription, events))
}

/// Records the events of the first `changes` of [`CHANGES`] and returns their
/// keys, earliest change first.
async fn insert_events(pool: &PgPool, changes: usize) -> Result<Vec<i64>, UpgradeTestError> {
    let mut events = Vec::new();
    for (write_id, committed_at) in WRITES.iter().zip(CHANGES).take(changes) {
        events.push(insert_event(pool, write_id, 0, committed_at).await?);
    }
    Ok(events)
}

/// Queues the delivery of the event of change `change` of [`CHANGES`] to
/// `subscription`, with the change order its event records and the further
/// columns in `named`, and returns its key.
async fn queue_delivery(pool: &PgPool, subscription: i64, events: &[i64], change: usize, named: Vec<(&str, SimpleExpr)>) -> Result<i64, UpgradeTestError> {
    let event = *events.get(change).ok_or(UpgradeTestError::Missing { what: "the event of a change" })?;
    let committed_at = *CHANGES.get(change).ok_or(UpgradeTestError::Missing { what: "a change" })?;
    let order = vec![("committed_at", instant(committed_at)), ("event_ordinal", Expr::val(0))];
    insert_delivery(pool, event, subscription, order.into_iter().chain(named).collect()).await
}

/// The columns of a delivery `worker` holds under a lease that runs an hour
/// past the seeding.
fn leased_to(worker: Uuid) -> Vec<(&'static str, SimpleExpr)> {
    vec![
        ("claimed_by", uuid(&worker.to_string())),
        ("lease_until", span_after_transaction_start(Duration::from_secs(3600))),
    ]
}

/// The column of a delivery set aside behind its Subscription's probe.
fn set_aside() -> Vec<(&'static str, SimpleExpr)> {
    vec![("held_at", instant(SET_ASIDE_AT))]
}

/// Holds `subscription`, as a failure of its endpoint does.
async fn hold(pool: &PgPool, subscription: i64) -> Result<(), UpgradeTestError> {
    insert(pool, "notification_delivery_holds", vec![("subscription_key", Expr::val(subscription))]).await
}

/// The tenant's delivery queue, reading names through dictionaries of its own.
fn queue(pool: &PgPool) -> NotificationDeliveryQueue {
    NotificationDeliveryQueue::new(
        pool,
        Arc::new(NameCache::<AttributeName>::new(pool, &NameCacheConfig::default())),
        Arc::new(NameCache::<EntityTypeName>::new(pool, &NameCacheConfig::default())),
    )
}

/// The wait of a schedule retrying every `delay`.
fn step(delay: Duration) -> Result<BackoffStep, UpgradeTestError> {
    Ok(DeliveryBackoff::new(BackoffSchedule::constant(delay))?.first_step())
}

/// A failed attempt, due again at once.
fn failed() -> Result<DeliveryVerdict, UpgradeTestError> {
    Ok(DeliveryVerdict::Failed {
        detail: "the endpoint answered 503".to_owned(),
        retry_after: step(Duration::ZERO)?,
    })
}

/// A final attempt, whose Subscription's next probe is due [`PROBE_AFTER`]
/// later.
fn exhausted() -> Result<DeliveryVerdict, UpgradeTestError> {
    Ok(DeliveryVerdict::Exhausted {
        detail: "the endpoint answered 400".to_owned(),
        probe_after: step(PROBE_AFTER)?,
    })
}

/// Records `verdict` of the live branch's delivery `delivery` in a round of
/// its own and returns the answer.
async fn settle(queue: &NotificationDeliveryQueue, delivery: i64, verdict: DeliveryVerdict) -> Result<Vec<LeaseAcknowledgement>, UpgradeTestError> {
    settle_in_one_round(queue, vec![(delivery, verdict)]).await
}

/// Records the verdict of each of the live branch's deliveries in `verdicts`
/// in one round, in that order, and returns the answers.
async fn settle_in_one_round(queue: &NotificationDeliveryQueue, verdicts: Vec<(i64, DeliveryVerdict)>) -> Result<Vec<LeaseAcknowledgement>, UpgradeTestError> {
    let settlements: Vec<DeliverySettlement> = verdicts
        .into_iter()
        .map(|(delivery, verdict)| DeliverySettlement {
            target: ContextTarget::Live,
            key: DeliveryKey::from(delivery),
            verdict,
        })
        .collect();
    Ok(queue.settle_and_claim(&LeaseHolder::from(WORKER), &settlements, None).await?.answers)
}

/// Claims up to `width` deliveries in `order` in a round of its own and
/// returns their keys, ascending.
async fn claim(queue: &NotificationDeliveryQueue, width: u32, order: DeliveryClaimOrder) -> Result<Vec<i64>, UpgradeTestError> {
    let held = HeldEvents::default();
    let request = ClaimRequest {
        lease: LeaseTerm::from_seconds(60)?,
        width: NonZeroU32::new(width).ok_or(UpgradeTestError::Missing { what: "a claim width" })?,
        order,
        held: &held,
    };
    let round = queue.settle_and_claim(&LeaseHolder::from(WORKER), &[], Some(request)).await?;
    let mut keys: Vec<i64> = round
        .claims?
        .iter()
        .map(|claimed| match claimed {
            DeliveryClaim::Readable(entry) => entry.key.into_inner(),
            DeliveryClaim::Unreadable(unreadable) => unreadable.key.into_inner(),
        })
        .collect();
    keys.sort_unstable();
    Ok(keys)
}

/// Both claim orders, the queue's and the one serving the fewest in flight
/// first.
fn claim_orders() -> [DeliveryClaimOrder; 2] {
    [DeliveryClaimOrder::Queue, DeliveryClaimOrder::FewestInFlight(InFlightCeiling::Lifted)]
}

/// `SELECT <value> FROM notification_deliveries WHERE delivery_key = $1`.
fn of_delivery(delivery: i64, value: SimpleExpr) -> SelectStatement {
    Query::select()
        .expr(value)
        .from(name("notification_deliveries"))
        .and_where(Expr::col(name("delivery_key")).eq(delivery))
        .to_owned()
}

/// Whether `delivery` is set aside behind its Subscription's probe.
async fn is_set_aside(pool: &PgPool, delivery: i64) -> Result<bool, UpgradeTestError> {
    Ok(of_delivery(delivery, Expr::col(name("held_at")).is_not_null())
        .to_query_scalar::<bool>()
        .fetch_one(pool)
        .await?)
}

/// Whether `delivery` is leased to a worker.
async fn is_claimed(pool: &PgPool, delivery: i64) -> Result<bool, UpgradeTestError> {
    Ok(of_delivery(delivery, Expr::col(name("claimed_by")).is_not_null())
        .to_query_scalar::<bool>()
        .fetch_one(pool)
        .await?)
}

/// Whether `delivery` is due at `due`.
async fn is_due_at(pool: &PgPool, delivery: i64, due: &str) -> Result<bool, UpgradeTestError> {
    Ok(of_delivery(delivery, Expr::col(name("next_attempt_at")).eq(instant(due)))
        .to_query_scalar::<bool>()
        .fetch_one(pool)
        .await?)
}

/// Whether `probe` is due [`PROBE_AFTER`] after the instant `exhausted` was
/// exhausted at, the start of the transaction that recorded it.
async fn is_due_one_step_after_the_exhaustion_of(pool: &PgPool, probe: i64, exhausted: i64) -> Result<bool, UpgradeTestError> {
    let due = scalar_subquery(of_delivery(probe, Expr::col(name("next_attempt_at"))));
    let exhausted_at = scalar_subquery(of_delivery(exhausted, Expr::col(name("exhausted_at"))));
    let step = make_interval_secs(Expr::val(PROBE_AFTER.as_secs_f64()));
    Ok(Query::select()
        .expr(due.eq(exhausted_at.add(step)))
        .to_owned()
        .to_query_scalar::<bool>()
        .fetch_one(pool)
        .await?)
}

/// Moves `delivery`'s due instant to `due`.
async fn make_due_at(pool: &PgPool, delivery: i64, due: &str) -> Result<(), UpgradeTestError> {
    Query::update()
        .table(name("notification_deliveries"))
        .value(name("next_attempt_at"), instant(due))
        .and_where(Expr::col(name("delivery_key")).eq(delivery))
        .to_owned()
        .to_query()
        .execute(pool)
        .await?;
    Ok(())
}

/// The held Subscriptions, ascending.
async fn held_subscriptions(pool: &PgPool) -> Result<Vec<i64>, UpgradeTestError> {
    read_column(pool, "notification_delivery_holds", "subscription_key").await
}

/// The columns of `notification_delivery_holds`, by name.
async fn hold_columns(pool: &PgPool) -> Result<Vec<String>, UpgradeTestError> {
    let column = || cast(Expr::col(name("column_name")), PostgresType::Text);
    Ok(Query::select()
        .expr(column())
        .from((name("information_schema"), name("columns")))
        .and_where(Expr::col(name("table_schema")).eq("public"))
        .and_where(Expr::col(name("table_name")).eq("notification_delivery_holds"))
        .order_by_expr(column(), Order::Asc)
        .to_owned()
        .to_query_scalar::<String>()
        .fetch_all(pool)
        .await?)
}

#[tokio::test]
async fn two_failures_recorded_in_separate_rounds_leave_the_earlier_change_as_the_probe() -> Result<(), UpgradeTestError> {
    let (database, subscription, events) = with_changes(2).await?;
    let pool = database.pool();
    let first = queue_delivery(pool, subscription, &events, 0, Vec::new()).await?;
    let second = queue_delivery(pool, subscription, &events, 1, Vec::new()).await?;
    let queue = queue(pool);
    assert_eq!(claim(&queue, 2, DeliveryClaimOrder::Queue).await?, [first, second]);

    // The later change's failure is recorded first, in a round of its own, as
    // a drain records an outcome that ends with no round in flight.
    assert_eq!(settle(&queue, second, failed()?).await?, [LeaseAcknowledgement::Applied]);
    assert_eq!(settle(&queue, first, failed()?).await?, [LeaseAcknowledgement::Applied]);

    assert_eq!(held_subscriptions(pool).await?, [subscription]);
    assert_eq!([is_set_aside(pool, first).await?, is_set_aside(pool, second).await?], [false, true]);
    assert_eq!(claim(&queue, 2, DeliveryClaimOrder::Queue).await?, [first]);
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn two_failures_recorded_in_one_round_leave_the_earlier_change_as_the_probe() -> Result<(), UpgradeTestError> {
    let (database, subscription, events) = with_changes(2).await?;
    let pool = database.pool();
    // The later change is queued first, so its key is the smaller, and its
    // failure is recorded first: neither the keys nor the order of the
    // settlements points at the earlier change.
    let second = queue_delivery(pool, subscription, &events, 1, Vec::new()).await?;
    let first = queue_delivery(pool, subscription, &events, 0, Vec::new()).await?;
    let queue = queue(pool);
    assert_eq!(claim(&queue, 2, DeliveryClaimOrder::Queue).await?, [second, first]);

    assert_eq!(
        settle_in_one_round(&queue, vec![(second, failed()?), (first, failed()?)]).await?,
        [LeaseAcknowledgement::Applied, LeaseAcknowledgement::Applied]
    );

    assert_eq!(held_subscriptions(pool).await?, [subscription]);
    assert_eq!([is_set_aside(pool, first).await?, is_set_aside(pool, second).await?], [false, true]);
    assert_eq!(claim(&queue, 2, DeliveryClaimOrder::Queue).await?, [first]);
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_held_subscription_offers_its_earliest_queued_change_and_not_a_later_one() -> Result<(), UpgradeTestError> {
    for order in claim_orders() {
        let (database, subscription, events) = with_changes(2).await?;
        let pool = database.pool();
        hold(pool, subscription).await?;
        // The later change is queued first, so its key is the smaller.
        queue_delivery(pool, subscription, &events, 1, Vec::new()).await?;
        let earliest = queue_delivery(pool, subscription, &events, 0, Vec::new()).await?;
        assert_eq!(claim(&queue(pool), 2, order).await?, [earliest], "{order:?}");
        database.drop_database().await?;
    }
    Ok(())
}

#[tokio::test]
async fn a_held_subscription_whose_probe_is_in_flight_offers_nothing() -> Result<(), UpgradeTestError> {
    let (database, subscription, events) = with_changes(2).await?;
    let pool = database.pool();
    hold(pool, subscription).await?;
    queue_delivery(pool, subscription, &events, 0, leased_to(OTHER_WORKER)).await?;
    queue_delivery(pool, subscription, &events, 1, Vec::new()).await?;
    let queue = queue(pool);
    for order in claim_orders() {
        assert_eq!(claim(&queue, 2, order).await?, Vec::<i64>::new(), "{order:?}");
    }
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn an_exhausted_probe_passes_the_hold_to_the_earliest_queued_change_one_step_later() -> Result<(), UpgradeTestError> {
    let (database, subscription, events) = with_changes(3).await?;
    let pool = database.pool();
    hold(pool, subscription).await?;
    let probe = queue_delivery(pool, subscription, &events, 0, leased_to(WORKER)).await?;
    let next = queue_delivery(pool, subscription, &events, 1, set_aside()).await?;
    let last = queue_delivery(pool, subscription, &events, 2, set_aside()).await?;

    assert_eq!(settle(&queue(pool), probe, exhausted()?).await?, [LeaseAcknowledgement::Applied]);

    assert_eq!([is_set_aside(pool, next).await?, is_set_aside(pool, last).await?], [false, true]);
    assert!(is_due_one_step_after_the_exhaustion_of(pool, next, probe).await?);
    assert_eq!(held_subscriptions(pool).await?, [subscription]);
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn an_exhausted_last_delivery_releases_the_hold() -> Result<(), UpgradeTestError> {
    let (database, subscription, events) = with_changes(1).await?;
    let pool = database.pool();
    hold(pool, subscription).await?;
    let probe = queue_delivery(pool, subscription, &events, 0, leased_to(WORKER)).await?;

    assert_eq!(settle(&queue(pool), probe, exhausted()?).await?, [LeaseAcknowledgement::Applied]);

    assert_eq!(held_subscriptions(pool).await?, Vec::<i64>::new());
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn an_exhausted_delivery_that_is_not_the_probe_leaves_the_probe_as_it_is() -> Result<(), UpgradeTestError> {
    const PROBE_DUE: &str = "2999-01-01T00:00:00Z";
    let (database, subscription, events) = with_changes(2).await?;
    let pool = database.pool();
    hold(pool, subscription).await?;
    let probe = queue_delivery(pool, subscription, &events, 0, Vec::new()).await?;
    make_due_at(pool, probe, PROBE_DUE).await?;
    // In flight since before the hold was taken, so not set aside.
    let in_flight = queue_delivery(pool, subscription, &events, 1, leased_to(WORKER)).await?;

    assert_eq!(settle(&queue(pool), in_flight, exhausted()?).await?, [LeaseAcknowledgement::Applied]);

    assert!(is_due_at(pool, probe, PROBE_DUE).await?);
    assert!(!is_set_aside(pool, probe).await?);
    assert_eq!(held_subscriptions(pool).await?, [subscription]);
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn the_upgrade_takes_back_the_earliest_change_of_a_hold_whose_probe_is_a_later_one() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    let subscription = insert_subscription(database.pool()).await?;
    let events = insert_events(database.pool(), 3).await?;
    let event = |change: usize| events.get(change).copied().ok_or(UpgradeTestError::Missing { what: "the event of a change" });
    let earliest = insert_delivery(database.pool(), event(0)?, subscription, set_aside()).await?;
    let stored_probe = insert_delivery(database.pool(), event(1)?, subscription, vec![("attempt_count", Expr::val(1))]).await?;
    let in_flight = insert_delivery(database.pool(), event(2)?, subscription, leased_to(OTHER_WORKER)).await?;
    insert(
        database.pool(),
        "notification_delivery_holds",
        vec![("subscription_key", Expr::val(subscription)), ("delivery_key", Expr::val(stored_probe))],
    )
    .await?;

    database.migrate_to_current().await?;

    let pool = database.pool();
    assert_eq!(
        [
            is_set_aside(pool, earliest).await?,
            is_set_aside(pool, stored_probe).await?,
            is_set_aside(pool, in_flight).await?,
        ],
        [false, true, false]
    );
    assert!(is_claimed(pool, in_flight).await?);
    assert_eq!(held_subscriptions(pool).await?, [subscription]);
    assert_eq!(hold_columns(pool).await?, ["subscription_key"]);
    assert_eq!(claim(&queue(pool), 3, DeliveryClaimOrder::Queue).await?, [earliest]);
    database.drop_database().await?;
    Ok(())
}
