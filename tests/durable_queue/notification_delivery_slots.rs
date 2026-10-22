//! A delivery pass keeps a fixed number of sending slots busy, claiming a
//! delivery only when a slot is free to send it.
//!
//! A pass opens by claiming one delivery per slot and claims one more each time
//! a delivery is acknowledged, so the deliveries it holds claimed but not yet
//! acknowledged never outnumber its slots. Each delivery's lease therefore
//! starts as its attempt does and has to outlast that one attempt only: a batch
//! claimed under one lease and sent a few at a time would still be waiting when
//! that lease ran out, a second worker would claim it, and both would send it.
//! And a slot freed by a fast delivery is refilled at once, so one slow
//! endpoint holds up its own slot and nothing else.
//!
//! A retry is scheduled one backoff after the delivery's failure is
//! acknowledged, not after the pass began, so a slow failing delivery is not
//! due again the moment its pass ends. The pass stamps it on its own clock --
//! the instant the pass was started at, advanced by the monotonic time it has
//! run since -- so that is the clock the failure is measured on here too.

use crate::{
    notification_fixture::{delivery_ids, queue_notification},
    queue_database::{delivery_of, delivery_queue, queue_database},
};
use andromeda_notification::{
    delivery_config::DeliveryConfig,
    delivery_drain::{DeliveryDisposition, NotificationDeliveryDrain, NotificationDeliveryTransport},
    delivery_entry::{DeliveryEntry, DeliveryKey},
    delivery_store::DeliveryStore,
};
use andromeda_postgresql_schema::notification::NotificationDeliveries;
use andromeda_postgresql_seaquery::{aggregate::count_all, clock::now, statement::Statement};
use chrono::{DateTime, TimeDelta, Utc};
use futures::future::BoxFuture;
use pretty_assertions::assert_eq;
use sea_query::{Alias, Expr, ExprTrait, Func, Query};
use serde_json::json;
use serial_test::serial;
use sqlx::PgPool;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicI64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::time::sleep;

/// Deliveries one pass sends at once.
const CONCURRENCY: i64 = 2;
/// The base retry backoff of the default delivery configuration.
const BASE_BACKOFF: TimeDelta = TimeDelta::seconds(5);

/// The delivery configuration under test: `batch` deliveries per pass, sent
/// [`CONCURRENCY`] at a time under a lease of `lease` seconds, which has to
/// exceed the one-second request timeout by the configured margin.
fn slot_configuration(batch: usize, lease: &str) -> DeliveryConfig {
    let mut configuration = serde_json::to_value(DeliveryConfig::default()).expect("the default configuration serialises");
    configuration["batch_size"] = json!(batch);
    configuration["concurrency"] = json!(CONCURRENCY);
    configuration["lease"] = json!(lease);
    configuration["request_timeout"] = json!("1s");
    let configuration: DeliveryConfig = serde_json::from_value(configuration).expect("the adjusted configuration deserialises");
    configuration.validate().expect("the adjusted configuration is valid");
    configuration
}

/// A drain over the suite's queue sending through `transport` under
/// `configuration`.
fn drain(pool: &PgPool, transport: Arc<dyn NotificationDeliveryTransport>, configuration: &DeliveryConfig) -> NotificationDeliveryDrain {
    NotificationDeliveryDrain::new(Arc::new(delivery_queue(pool)) as Arc<dyn DeliveryStore>, transport, configuration)
        .expect("the configuration composes a drain")
}

/// How many deliveries the two-drain verification queues and one pass may send.
const DELIVERIES: usize = 6;
/// The lease of the two-drain verification.
const SHORT_LEASE: &str = "6s";
/// How long each send is held there: under the six-second lease, while three
/// sends one after another outlast it.
const SEND_TIME: Duration = Duration::from_millis(2_500);
/// When the second drain starts: after a lease taken when the first drain
/// started has lapsed, while the first drain still has deliveries to send.
const SECOND_DRAIN_DELAY: Duration = Duration::from_millis(6_500);

/// A transport that holds every send, records which deliveries it sent, and
/// records the most deliveries any one worker held claimed but unacknowledged
/// while a send was in flight.
struct HoldingTransport {
    /// The suite database the claims are read from.
    pool: PgPool,
    /// How often each delivery was sent.
    sent: Mutex<BTreeMap<DeliveryKey, usize>>,
    /// The most live, unacknowledged claims one worker held.
    widest_claim: AtomicI64,
}

impl HoldingTransport {
    /// The most live, unacknowledged claims any one worker holds right now.
    async fn widest_live_claim(&self) -> i64 {
        Query::select()
            .expr(Func::coalesce::<_, Expr>([Func::max(Expr::col(Alias::new("held"))).into(), Expr::val(0_i64)]))
            .from_subquery(
                Query::select()
                    .expr_as(count_all(), Alias::new("held"))
                    .from(NotificationDeliveries::Table)
                    .and_where(Expr::col(NotificationDeliveries::DeliveredAt).is_null())
                    .and_where(Expr::col(NotificationDeliveries::ExhaustedAt).is_null())
                    .and_where(Expr::col(NotificationDeliveries::LeaseUntil).gt(now()))
                    .group_by_col(NotificationDeliveries::ClaimedBy)
                    .to_owned(),
                Alias::new("leases"),
            )
            .to_query_scalar()
            .fetch_one(&self.pool)
            .await
            .expect("live leases")
    }
}

impl NotificationDeliveryTransport for HoldingTransport {
    fn deliver<'a>(&'a self, entry: &'a DeliveryEntry) -> BoxFuture<'a, DeliveryDisposition> {
        Box::pin(async move {
            let widest = self.widest_live_claim().await;
            self.widest_claim.fetch_max(widest, Ordering::Relaxed);
            *self.sent.lock().expect("sent record lock").entry(entry.key).or_default() += 1;
            sleep(SEND_TIME).await;
            DeliveryDisposition::Delivered
        })
    }
}

/// Claimed as one batch under one lease, all six deliveries would be claimed by
/// the first drain at once, three times as many as it sends at once; those
/// still unsent when that lease lapsed would be claimed by the second drain,
/// and both drains would send them.
#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_delivery_pass_holds_no_more_unacknowledged_claims_than_slots_and_sends_each_delivery_once() {
    let pool = queue_database().await;
    let queued = queue_notification(&pool, DELIVERIES).await;
    let deliveries = delivery_ids(&pool, &queued.record).await;
    assert_eq!(deliveries.len(), DELIVERIES, "the fixture queues one delivery per subscription");

    let transport = Arc::new(HoldingTransport {
        pool: pool.clone(), // clone: the transport reads claims over its own handle to the shared pool.
        sent: Mutex::new(BTreeMap::new()),
        widest_claim: AtomicI64::new(0),
    });
    let configuration = slot_configuration(DELIVERIES, SHORT_LEASE);
    let first = drain(&pool, Arc::clone(&transport) as Arc<dyn NotificationDeliveryTransport>, &configuration);
    let second = drain(&pool, Arc::clone(&transport) as Arc<dyn NotificationDeliveryTransport>, &configuration);
    let second_pass = async {
        sleep(SECOND_DRAIN_DELAY).await;
        second.execute(Utc::now()).await
    };
    let (first_outcome, second_outcome) = tokio::join!(first.execute(Utc::now()), second_pass);
    let first_outcome = first_outcome.expect("the first pass runs");
    let second_outcome = second_outcome.expect("the second pass runs");

    assert!(
        transport.widest_claim.load(Ordering::Relaxed) <= CONCURRENCY,
        "no worker ever held more deliveries claimed and unacknowledged than it has slots, saw {}",
        transport.widest_claim.load(Ordering::Relaxed)
    );
    let expected: BTreeMap<DeliveryKey, usize> = deliveries.iter().map(|delivery| (DeliveryKey::from(*delivery), 1)).collect();
    assert_eq!(
        *transport.sent.lock().expect("sent record lock"),
        expected,
        "every delivery is sent exactly once"
    );
    assert_eq!(
        first_outcome.claimed + second_outcome.claimed,
        DELIVERIES,
        "the two passes claimed every delivery once between them"
    );
    assert_eq!(
        (first_outcome.lease_lost, second_outcome.lease_lost),
        (0, 0),
        "no lease lapsed before it was acknowledged"
    );
    let delivered: i64 = Query::select()
        .expr(count_all())
        .from(NotificationDeliveries::Table)
        .and_where(Expr::col(NotificationDeliveries::DeliveredAt).is_not_null())
        .to_query_scalar()
        .fetch_one(&pool)
        .await
        .expect("delivered count");
    assert_eq!(delivered, i64::try_from(DELIVERIES).expect("the count fits"), "every delivery is acknowledged");
    pool.close().await;
}

/// How many deliveries the slow-endpoint verification queues.
const MIXED_DELIVERIES: usize = 5;
/// The lease of the slow-endpoint verification: comfortably above the slow send.
const LONG_LEASE: &str = "30s";
/// How long the slow endpoint holds its one send before it fails.
const SLOW_SEND: Duration = Duration::from_secs(3);
/// How long every other endpoint takes.
const FAST_SEND: Duration = Duration::from_millis(100);

/// A transport whose first send goes to a slow endpoint that fails after
/// [`SLOW_SEND`], while every other send succeeds after [`FAST_SEND`].
///
/// Every instant it records is monotonic, the clock a delivery pass advances
/// its own instant by.
#[derive(Default)]
struct OneSlowEndpoint {
    /// The delivery the slow endpoint received, and when its send began, once
    /// one did.
    slow: Mutex<Option<(DeliveryKey, Instant)>>,
    /// When each fast send completed.
    fast_completed: Mutex<Vec<(DeliveryKey, Instant)>>,
    /// When the slow send failed.
    slow_failed: Mutex<Option<Instant>>,
}

impl OneSlowEndpoint {
    /// Whether `key` is the send the slow endpoint takes: the first one made.
    fn takes_slowly(&self, key: DeliveryKey) -> bool {
        let mut slow = self.slow.lock().expect("slow endpoint lock");
        slow.get_or_insert_with(|| (key, Instant::now())).0 == key
    }
}

impl NotificationDeliveryTransport for OneSlowEndpoint {
    fn deliver<'a>(&'a self, entry: &'a DeliveryEntry) -> BoxFuture<'a, DeliveryDisposition> {
        Box::pin(async move {
            if self.takes_slowly(entry.key) {
                sleep(SLOW_SEND).await;
                *self.slow_failed.lock().expect("slow failure lock") = Some(Instant::now());
                DeliveryDisposition::RetryableFailure {
                    detail: "the slow endpoint timed out".to_owned(),
                }
            } else {
                sleep(FAST_SEND).await;
                self.fast_completed.lock().expect("fast completions lock").push((entry.key, Instant::now()));
                DeliveryDisposition::Delivered
            }
        })
    }
}

/// While one slot waits on a slow endpoint, the other keeps sending: every
/// other delivery completes before the slow one fails. A pass that claimed the
/// next deliveries only once the whole previous claim was acknowledged would
/// leave them waiting on the slow endpoint. The slow delivery's retry then
/// falls due one backoff after its failure was acknowledged, not one backoff
/// after the pass began.
#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_slow_endpoint_holds_up_only_its_own_slot_and_its_retry_is_paced_from_its_failure() {
    let pool = queue_database().await;
    let queued = queue_notification(&pool, MIXED_DELIVERIES).await;
    assert_eq!(delivery_ids(&pool, &queued.record).await.len(), MIXED_DELIVERIES);

    let transport = Arc::new(OneSlowEndpoint::default());
    let pass = drain(
        &pool,
        Arc::clone(&transport) as Arc<dyn NotificationDeliveryTransport>,
        &slot_configuration(MIXED_DELIVERIES, LONG_LEASE),
    );
    let origin = Utc::now();
    let outcome = pass.execute(origin).await.expect("the pass runs");
    assert_eq!(outcome.claimed, MIXED_DELIVERIES, "the pass claimed every delivery");

    let slow_failed_at = transport.slow_failed.lock().expect("slow failure lock").expect("the slow endpoint failed");
    let (fast_sent, fast_before_slow) = {
        let fast_completed = transport.fast_completed.lock().expect("fast completions lock");
        (fast_completed.len(), fast_completed.iter().all(|(_, completed)| *completed < slow_failed_at))
    };
    assert_eq!(fast_sent, MIXED_DELIVERIES - 1, "every other delivery was sent");
    assert!(
        fast_before_slow,
        "every other delivery completed while the slow endpoint was still holding its slot"
    );

    let (slow, slow_started_at) = transport
        .slow
        .lock()
        .expect("slow endpoint lock")
        .expect("the slow endpoint received a delivery");
    let (attempts, next_attempt_at, delivered): (i32, DateTime<Utc>, bool) = delivery_of(slow.into_inner())
        .columns([NotificationDeliveries::AttemptCount, NotificationDeliveries::NextAttemptAt])
        .expr(Expr::col(NotificationDeliveries::DeliveredAt).is_not_null())
        .to_query_as()
        .fetch_one(&pool)
        .await
        .expect("the slow delivery's state");
    assert!(!delivered, "the slow delivery failed");
    assert_eq!(attempts, 1, "its failed attempt is counted");
    // The pass's instant is `origin` advanced by the monotonic time since the
    // pass began, and the slow send began after the pass did, so `origin`
    // advanced by the time since the slow send began is no later than the
    // pass's instant at the failure, and earlier still than at its
    // acknowledgement. A retry stamped from the start of the pass instead would
    // fall due a whole slow send before this.
    let failed_on_the_pass_clock = origin + TimeDelta::from_std(slow_failed_at - slow_started_at).expect("the slow send fits a time delta");
    assert!(
        next_attempt_at >= failed_on_the_pass_clock + BASE_BACKOFF,
        "its retry falls due one backoff after its failure, not after the start of the pass: {next_attempt_at} precedes {}",
        failed_on_the_pass_clock + BASE_BACKOFF
    );
    pool.close().await;
}
