use crate::event::{
    snapshot_notification_sink::{SnapshotStatusDeliveryDrain, SnapshotStatusPassOutcome, SnapshotStatusTransport},
    snapshot_status_queue::SnapshotStatusQueue,
};
use andromeda_configuration::{
    human_duration::HumanDuration,
    schema::notifications::delivery::{DeliveryConfig, DeliveryMode},
};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder};
use andromeda_delivery::dispatch::DeliveryResult;
use andromeda_postgresql_context_store::snapshot::status_outbox::{SnapshotDeliveryFailure, SnapshotStatusDelivery};
use andromeda_snapshot::error::SnapshotError;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::executor::block_on;
use pretty_assertions::assert_eq;
use serde_json::value::RawValue;
use std::{
    collections::BTreeMap,
    mem::take,
    num::NonZeroU32,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};
use url::Url;
use uuid::Uuid;

/// One thing the drain did, in the order it did it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Call {
    Renew(Uuid),
    Send(Uuid),
    Acknowledge(Uuid),
    Fail(Uuid),
}

/// How the receiver answers a notification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Answer {
    Accepts,
    FailsTransiently,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A queue that hands out its deliveries once and answers every statement on a
/// delivery as a worker that another worker took the delivery over from would be
/// answered.
#[derive(Default)]
struct RecordingQueue {
    due: Mutex<Vec<SnapshotStatusDelivery>>,
    taken_over: Mutex<Vec<Uuid>>,
    calls: Mutex<Vec<Call>>,
}

impl RecordingQueue {
    fn holding(deliveries: impl IntoIterator<Item = SnapshotStatusDelivery>) -> RecordingQueue {
        RecordingQueue {
            due: Mutex::new(deliveries.into_iter().collect()),
            ..RecordingQueue::default()
        }
    }

    /// Another worker reclaims the delivery once its lease has lapsed.
    fn take_over(&self, id: Uuid) {
        locked(&self.taken_over).push(id);
    }

    fn record(&self, call: Call) {
        locked(&self.calls).push(call);
    }

    fn calls(&self) -> Vec<Call> {
        locked(&self.calls).clone()
    }

    fn guarded(&self, call: Call, id: Uuid) -> LeaseAcknowledgement {
        self.record(call);
        if locked(&self.taken_over).contains(&id) {
            LeaseAcknowledgement::Lost
        } else {
            LeaseAcknowledgement::Applied
        }
    }
}

#[async_trait]
impl SnapshotStatusQueue for RecordingQueue {
    async fn claim(&self, _worker: LeaseHolder, _lease_until: DateTime<Utc>, _limit: NonZeroU32) -> Result<Vec<SnapshotStatusDelivery>, SnapshotError> {
        Ok(take(&mut *locked(&self.due)))
    }

    async fn renew(&self, delivery: &SnapshotStatusDelivery, _worker: LeaseHolder, _lease_until: DateTime<Utc>) -> Result<LeaseAcknowledgement, SnapshotError> {
        Ok(self.guarded(Call::Renew(delivery.id), delivery.id))
    }

    async fn acknowledge(&self, delivery: &SnapshotStatusDelivery, _worker: LeaseHolder) -> Result<LeaseAcknowledgement, SnapshotError> {
        Ok(self.guarded(Call::Acknowledge(delivery.id), delivery.id))
    }

    async fn fail(
        &self,
        delivery: &SnapshotStatusDelivery,
        _worker: LeaseHolder,
        _failure: SnapshotDeliveryFailure,
        _max_attempts: i32,
        _retry_at: DateTime<Utc>,
        _detail: &str,
    ) -> Result<LeaseAcknowledgement, SnapshotError> {
        Ok(self.guarded(Call::Fail(delivery.id), delivery.id))
    }

    async fn cleanup(&self, _delivered_before: DateTime<Utc>, _exhausted_before: DateTime<Utc>) -> Result<u64, SnapshotError> {
        Ok(0)
    }
}

/// A receiver that answers every notification the same way, while another
/// worker takes over the deliveries named for the one being sent: the sending
/// outlasts their leases.
struct SlowReceiver {
    queue: Arc<RecordingQueue>,
    answer: Answer,
    takes_over_while_sending: BTreeMap<Uuid, Vec<Uuid>>,
}

#[async_trait]
impl SnapshotStatusTransport for SlowReceiver {
    async fn deliver_status(&self, delivery: &SnapshotStatusDelivery) -> DeliveryResult {
        self.queue.record(Call::Send(delivery.id));
        for id in self.takes_over_while_sending.get(&delivery.id).into_iter().flatten() {
            self.queue.take_over(*id);
        }
        match self.answer {
            Answer::Accepts => DeliveryResult::Delivered,
            Answer::FailsTransiently => DeliveryResult::RetryableFailure,
        }
    }
}

#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
fn delivery(id: u128) -> SnapshotStatusDelivery {
    SnapshotStatusDelivery {
        id: Uuid::from_u128(id),
        snapshot_id: "urn:ngsi-ld:Snapshot:1".parse().expect("a valid Snapshot identifier"),
        endpoint: Url::parse("http://receiver.example/notify").expect("a valid URL"),
        receiver_info: None,
        notification: RawValue::from_string("{}".to_owned()).expect("valid JSON"),
        attempt_count: 0,
    }
}

#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
fn config(concurrent: u32) -> DeliveryConfig {
    DeliveryConfig {
        mode: DeliveryMode::Durable,
        request_timeout: HumanDuration::from_secs(10),
        max_concurrent: NonZeroU32::new(concurrent).expect("a positive bound"),
        ..DeliveryConfig::default()
    }
}

/// Run one pass that claims all of `count` due deliveries, and return what the
/// drain did.
fn run_pass(count: u128, answer: Answer, takes_over_while_sending: BTreeMap<Uuid, Vec<Uuid>>) -> Result<(SnapshotStatusPassOutcome, Vec<Call>), SnapshotError> {
    let queue = Arc::new(RecordingQueue::holding((1..=count).map(delivery)));
    let receiver = SlowReceiver {
        queue: Arc::clone(&queue), // clone: the receiver records its sends and takeovers on the queue the drain reads.
        answer,
        takes_over_while_sending,
    };
    let concurrent = u32::try_from(count).unwrap_or(u32::MAX);
    let port: Arc<dyn SnapshotStatusQueue> = Arc::<RecordingQueue>::clone(&queue); // clone: the drain and the assertions share one queue.
    let drain = SnapshotStatusDeliveryDrain::new(port, Arc::new(receiver), &config(concurrent))?;
    let outcome = block_on(drain.execute(Utc::now()))?;
    Ok((outcome, queue.calls()))
}

fn id(value: u128) -> Uuid {
    Uuid::from_u128(value)
}

#[test]
fn every_delivery_renews_its_lease_right_before_it_is_sent() -> Result<(), SnapshotError> {
    let (outcome, calls) = run_pass(3, Answer::Accepts, BTreeMap::new())?;
    assert_eq!(outcome, SnapshotStatusPassOutcome { claimed: 3, lease_lost: 0 });
    assert_eq!(
        calls,
        vec![
            Call::Renew(id(1)),
            Call::Send(id(1)),
            Call::Acknowledge(id(1)),
            Call::Renew(id(2)),
            Call::Send(id(2)),
            Call::Acknowledge(id(2)),
            Call::Renew(id(3)),
            Call::Send(id(3)),
            Call::Acknowledge(id(3)),
        ]
    );
    Ok(())
}

#[test]
fn a_pass_sends_only_the_deliveries_no_other_worker_took_over_before_their_turn() -> Result<(), SnapshotError> {
    let takeovers = BTreeMap::from([(id(1), vec![id(2), id(3)])]);
    let (outcome, calls) = run_pass(3, Answer::Accepts, takeovers)?;
    assert_eq!(outcome, SnapshotStatusPassOutcome { claimed: 3, lease_lost: 2 });
    assert_eq!(
        calls,
        vec![
            Call::Renew(id(1)),
            Call::Send(id(1)),
            Call::Acknowledge(id(1)),
            Call::Renew(id(2)),
            Call::Renew(id(3)),
        ],
        "the deliveries another worker owns are neither sent nor acknowledged"
    );
    Ok(())
}

#[test]
fn an_acknowledgement_that_finds_its_delivery_taken_over_is_counted_as_a_lost_lease() -> Result<(), SnapshotError> {
    let takeovers = BTreeMap::from([(id(1), vec![id(1)])]);
    let (outcome, calls) = run_pass(1, Answer::Accepts, takeovers)?;
    assert_eq!(outcome, SnapshotStatusPassOutcome { claimed: 1, lease_lost: 1 });
    assert_eq!(calls, vec![Call::Renew(id(1)), Call::Send(id(1)), Call::Acknowledge(id(1))]);
    Ok(())
}

#[test]
fn a_failure_record_that_finds_its_delivery_taken_over_is_counted_as_a_lost_lease() -> Result<(), SnapshotError> {
    let takeovers = BTreeMap::from([(id(1), vec![id(1)])]);
    let (outcome, calls) = run_pass(1, Answer::FailsTransiently, takeovers)?;
    assert_eq!(outcome, SnapshotStatusPassOutcome { claimed: 1, lease_lost: 1 });
    assert_eq!(calls, vec![Call::Renew(id(1)), Call::Send(id(1)), Call::Fail(id(1))]);
    Ok(())
}

#[test]
fn a_delivery_pass_issues_no_cleanup() -> Result<(), &'static str> {
    // The drain is database-backed, so this guards the split structurally:
    // the delivery pass is defined before the cleanup pass, and the only
    // outbox cleanup call sits inside the cleanup pass, so a wake-driven
    // delivery pass issues no retention DELETE.
    let source = include_str!("../snapshot_notification_sink.rs");
    let execute = source.find("pub async fn execute").ok_or("delivery pass missing")?;
    let cleanup_fn = source.find("pub async fn cleanup").ok_or("cleanup pass missing")?;
    let cleanup_call = source.find("self.outbox.cleanup(").ok_or("outbox cleanup call missing")?;
    assert!(execute < cleanup_fn, "the delivery pass is defined before the cleanup pass");
    assert!(cleanup_call > cleanup_fn, "the outbox cleanup is issued only from the cleanup pass");
    Ok(())
}
