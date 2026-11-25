//! Queueing the deliveries one write's notification events owe.
//!
//! Every `(event, subscription)` pair the inserted events owe is queued in one
//! set-write from parallel arrays, however many events and subscribers the
//! write has, and each delivery records where the change it notifies falls in
//! its Subscription's order.

use crate::{
    error::ContextStoreError,
    notification_delivery::{event_commit::EventCommit, rows::NewNotificationEvent},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::{
    notification::{NotificationDeliveries, NotificationDeliveryHolds},
    subscriptions::Subscriptions,
};
use andromeda_postgresql_seaquery::{
    pg_type::PostgresType,
    statement::Statement,
    unnest::{UnnestAliased, UnnestColumn, unnest_call},
};
use sea_query::{Alias, CaseStatement, Expr, ExprTrait, InsertStatement, JoinType, Keyword, LockType, OnConflict, Query, SimpleExpr};
use sqlx::{Postgres, Transaction};

/// Queue, in the caller's transaction `tx`, every delivery the events of
/// `events` that `inserted` reads back owe, each `(key, ordinal)` of
/// `inserted` naming one event actually written; nothing when none is owed.
///
/// A replayed event the conflict clause suppressed is missing from
/// `inserted`, so it is owed no second delivery.
///
/// # Errors
///
/// Returns [`ContextStoreError::StatementBuild`] when the insert cannot take
/// its rows from the arrays, and [`ContextStoreError::Database`] when the
/// statement fails.
pub(crate) async fn queue_owed_deliveries(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    events: &[NewNotificationEvent<'_>],
    inserted: &[(i64, i32)],
    commit: EventCommit,
) -> Result<(), ContextStoreError> {
    let owed = owed_deliveries(events, inserted);
    if owed.event_keys.is_empty() {
        return Ok(());
    }
    insert_deliveries_statement(target, owed, commit)?.to_query().execute(&mut **tx).await?;
    Ok(())
}

/// The deliveries a write owes, as the parallel arrays the delivery
/// set-write unnests, one element per delivery.
#[derive(Debug, Default, PartialEq, Eq)]
struct OwedDeliveries {
    /// The key of the event each delivery answers.
    event_keys: Vec<i64>,
    /// That event's position within its write, which orders the delivery
    /// after its commit instant.
    event_ordinals: Vec<i32>,
    /// The subscription each delivery is owed to.
    subscription_keys: Vec<i64>,
}

/// Every delivery the inserted events owe: one per matched subscription of
/// each event actually written, in event-then-matcher order.
fn owed_deliveries(events: &[NewNotificationEvent<'_>], inserted: &[(i64, i32)]) -> OwedDeliveries {
    let owed = inserted
        .iter()
        .filter_map(|(event_key, ordinal)| {
            events
                .iter()
                .find(|event| matches_ordinal(event, *ordinal))
                .map(|event| (*event_key, *ordinal, event))
        })
        .flat_map(|(event_key, ordinal, event)| event.subscription_keys.iter().map(move |key| (event_key, ordinal, key.get())));
    let mut deliveries = OwedDeliveries::default();
    for (event_key, ordinal, subscription_key) in owed {
        deliveries.event_keys.push(event_key);
        deliveries.event_ordinals.push(ordinal);
        deliveries.subscription_keys.push(subscription_key);
    }
    deliveries
}

/// Whether `event` occupies `ordinal` within its write.
///
/// The ordinal is unique per write by `uq_notification_events_write_identity`,
/// so the match is one-to-one with the returned row.
fn matches_ordinal(event: &NewNotificationEvent<'_>, ordinal: i32) -> bool {
    event.ordinal.as_i32().is_ok_and(|value| value == ordinal)
}

/// The `UNNEST(…) AS p(event_key, event_ordinal, subscription_key)` source
/// the delivery set-write reads its deliveries from.
///
/// The delivery statement names the source's alias after these columns, which
/// must match what the projection reads.
fn pair_source() -> (Alias, [UnnestColumn; 3]) {
    (
        Alias::new("p"),
        [
            UnnestColumn::new(NotificationDeliveries::EventKey, PostgresType::BigintArray),
            UnnestColumn::new(NotificationDeliveries::EventOrdinal, PostgresType::IntegerArray),
            UnnestColumn::new(NotificationDeliveries::SubscriptionKey, PostgresType::BigintArray),
        ],
    )
}

/// Insert every delivery a write owes in one statement, from the parallel
/// arrays of `owed`.
///
/// The pairs are joined against `subscriptions` on the surrogate key so a
/// subscription that vanished between the match and this insert contributes no
/// row (the delivery's foreign key would otherwise refuse the whole write); the
/// event it leaves behind is later reaped as an orphan.
///
/// The join alone does not close the race under READ COMMITTED: it reads the
/// subscriptions as of the statement's snapshot, so a deletion committing after
/// that snapshot but before the foreign-key check of the inserted row would
/// still refuse the write. `FOR KEY SHARE OF s` takes the row lock that check
/// takes, at the join: a deletion in flight is waited for, after which the
/// re-checked row is gone and contributes nothing, and a deletion arriving
/// later waits for this transaction and cascades onto the rows it wrote. The
/// lock is the weakest row lock there is, so it blocks only deletions and key
/// changes of the subscription, never its ordinary updates. Every delivery starts
/// due at the instant of the commit that made its change, read from the
/// store's clock, so the queue's ordering reflects the order changes were
/// committed in and a broker whose clock runs ahead of the store's does not
/// hold the delivery back from a claim. One queued for a held Subscription
/// starts set aside ([`held_at_insert`]).
///
/// Each delivery records the commit instant and the write position of the
/// change it notifies, which order its Subscription's deliveries
/// ([`NotificationDeliveries::CHANGE_ORDER`]): its key cannot, because each
/// connection draws keys from a block of its own and a replay queues the
/// change of an earlier commit after later ones.
///
/// A change owes each subscription one delivery (clause 5.8.6), which
/// `uq_notification_deliveries_event_subscription` holds:
/// `ON CONFLICT (event_key, subscription_key) DO NOTHING` drops a pair the
/// queue already holds, or one the arrays name twice, rather than queueing the
/// notification a second time.
fn insert_deliveries_statement(target: &ContextTarget, owed: OwedDeliveries, commit: EventCommit) -> Result<UnnestAliased<InsertStatement>, ContextStoreError> {
    let (pairs, columns) = pair_source();
    let aliased = pairs.clone(); // clone: the statement consumes the alias, so the column mapping takes this copy.
    let subscription = Alias::new("s");
    let source = Query::select()
        .column((pairs.clone(), NotificationDeliveries::EventKey))
        .column((subscription.clone(), Subscriptions::SubscriptionKey))
        .expr(commit.value())
        .expr(held_at_insert(target, &subscription))
        .expr(commit.value())
        .column((pairs.clone(), NotificationDeliveries::EventOrdinal))
        .from_function(
            unnest_call([
                (&columns[0], Expr::val(owed.event_keys)),
                (&columns[1], Expr::val(owed.event_ordinals)),
                (&columns[2], Expr::val(owed.subscription_keys)),
            ]),
            pairs.clone(),
        )
        .join_as(
            JoinType::InnerJoin,
            table_of(target, Subscriptions::Table),
            subscription.clone(),
            // clone: sea-query takes each alias by value, so the join copies it and the locking clause takes the original.
            Expr::col((subscription.clone(), Subscriptions::SubscriptionKey)).equals((pairs, NotificationDeliveries::SubscriptionKey)),
        )
        .lock_with_tables(LockType::KeyShare, [subscription])
        .to_owned();
    let mut statement = Query::insert();
    statement
        .into_table(table_of(target, NotificationDeliveries::Table))
        .columns([
            NotificationDeliveries::EventKey,
            NotificationDeliveries::SubscriptionKey,
            NotificationDeliveries::NextAttemptAt,
            NotificationDeliveries::HeldAt,
            NotificationDeliveries::CommittedAt,
            NotificationDeliveries::EventOrdinal,
        ])
        .select_from(source)?
        .on_conflict(
            OnConflict::columns([NotificationDeliveries::EventKey, NotificationDeliveries::SubscriptionKey])
                .do_nothing()
                .to_owned(),
        );
    Ok(UnnestAliased::new(statement).with_columns(aliased, &columns))
}

/// The `held_at` a delivery is queued with: the queueing instant when its
/// Subscription, joined as `subscription`, is held behind a probe, NULL
/// otherwise.
///
/// ```sql
/// CASE WHEN EXISTS (SELECT h.subscription_key FROM notification_delivery_holds AS h
///                   WHERE h.subscription_key = s.subscription_key FOR KEY SHARE)
///      THEN CURRENT_TIMESTAMP ELSE NULL END
/// ```
///
/// A delivery queued behind a hold is set aside from the start, so it never
/// enters the claim index for the claim to step over until the probe's next
/// failure. It is never the probe: a probe is a delivery that was attempted.
///
/// The key-share lock is what keeps such a row from outliving its hold. The
/// release deletes the hold, which waits for this transaction to commit, and
/// takes the Subscription's set-aside rows back in a later statement that sees
/// this one's rows. A hold whose deletion is in flight is waited for and then
/// read as gone, so the delivery is queued unheld. A hold still being taken is
/// not seen, so the delivery is queued unheld and the claim's own hold check
/// keeps it back until the probe's next failure sets it aside.
fn held_at_insert(target: &ContextTarget, subscription: &Alias) -> SimpleExpr {
    let hold = Alias::new("h");
    // clone: sea-query takes each alias by value, so the column list keeps a copy of the key.
    let hold_key = (hold.clone(), NotificationDeliveryHolds::SubscriptionKey);
    let hold_row = Query::select()
        .column(hold_key.clone())
        .from_as(table_of(target, NotificationDeliveryHolds::Table), hold)
        .and_where(Expr::col(hold_key).equals((subscription.clone(), Subscriptions::SubscriptionKey)))
        .lock(LockType::KeyShare)
        .to_owned();
    CaseStatement::new()
        .case(Expr::exists(hold_row), Expr::current_timestamp())
        .finally(Keyword::Null)
        .into()
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::{
        delivery_insert::{OwedDeliveries, insert_deliveries_statement, owed_deliveries},
        event_commit::EventCommit,
        rows::NewNotificationEvent,
    };
    use andromeda_context_commit::{store_instant::StoreInstant, subscription_key::SubscriptionKey, write_ordinal::WriteOrdinal};
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::entity_change::{EntityChange, EntityLifecycle};
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use chrono::{TimeZone, Utc};

    /// Three deliveries of two events of one write, the first event owed to
    /// two subscriptions.
    fn owed() -> OwedDeliveries {
        OwedDeliveries {
            event_keys: vec![42, 42, 43],
            event_ordinals: vec![0, 0, 1],
            subscription_keys: vec![1, 2, 1],
        }
    }

    /// A fresh write queues every delivery it owes in one statement from
    /// parallel arrays, joined to the subscriptions it key-share locks and
    /// nothing else, so a subscription deleted meanwhile contributes no row.
    /// Each delivery starts due at the transaction's instant, the instant its
    /// event records as `committed_at`, and records that instant and its
    /// event's write position, which order the Subscription's queue. One
    /// queued for a held Subscription is written set aside, and a pair the
    /// queue already holds is not queued again (clause 5.8.6).
    #[test]
    fn a_fresh_write_queues_every_delivery_it_owes_in_one_set_write() {
        assert_statement_snapshot!(
            "delivery-insert-a-fresh-write-queues-every-delivery-it-owes-in-one-set-write",
            insert_deliveries_statement(&ContextTarget::Live, owed(), EventCommit::ThisTransaction).expect("a valid insert")
        );
    }

    /// A replay queues the change an earlier commit made, so its deliveries
    /// start due at that commit's instant and record it, bound as read from
    /// the store's clock when the change was first committed.
    #[test]
    fn a_replayed_write_queues_its_deliveries_at_the_instant_of_the_commit_that_made_the_change() {
        let committed = Utc.with_ymd_and_hms(2025, 1, 27, 21, 0, 0).single().expect("a representable instant");
        assert_statement_snapshot!(
            "delivery-insert-a-replayed-write-queues-its-deliveries-at-the-instant-of-the-earlier-commit",
            insert_deliveries_statement(&ContextTarget::Live, owed(), EventCommit::Earlier(StoreInstant::new(committed))).expect("a valid insert")
        );
    }

    #[test]
    fn a_snapshot_branch_delivery_reads_the_holds_and_subscriptions_of_its_own_branch() {
        let key = SnapshotKey::try_from(5).expect("positive key");
        let branch = ContextTarget::Snapshot(SnapshotTarget::published(key, "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri")));
        assert_statement_snapshot!(
            "delivery-insert-a-snapshot-branch-delivery-reads-the-holds-and-subscriptions-of-its-own-branch",
            insert_deliveries_statement(&branch, owed(), EventCommit::ThisTransaction).expect("a valid insert")
        );
    }

    fn subscription(key: i64) -> SubscriptionKey {
        SubscriptionKey::try_from(key).expect("a positive subscription key")
    }

    /// The creation of `entity` at write position `ordinal`, matched by
    /// `subscriptions`.
    fn creation<'a>(entity: &'a Identifier, change: &'a EntityChange, ordinal: u32, subscriptions: &[i64]) -> NewNotificationEvent<'a> {
        NewNotificationEvent {
            ordinal: WriteOrdinal::at(ordinal),
            change,
            entity_id: entity,
            change_attribute_name_keys: Vec::new(),
            entity_snapshot: None,
            previous_snapshot: None,
            sys_attrs_snapshot: None,
            subscription_keys: subscriptions.iter().map(|key| subscription(*key)).collect(),
        }
    }

    #[test]
    fn the_owed_deliveries_follow_event_then_matcher_order_and_skip_events_a_replay_did_not_write() {
        let entity: Identifier = "urn:ngsi-ld:Vehicle:1".parse().expect("a valid entity URI");
        let change = EntityChange::new(EntityLifecycle::Created, Vec::new());
        let events = [
            creation(&entity, &change, 3, &[5, 6]),
            creation(&entity, &change, 4, &[7]),
            creation(&entity, &change, 5, &[8]),
        ];
        let inserted = [(100_i64, 3_i32), (101, 5)];
        pretty_assertions::assert_eq!(
            owed_deliveries(&events, &inserted),
            OwedDeliveries {
                event_keys: vec![100, 100, 101],
                event_ordinals: vec![3, 3, 5],
                subscription_keys: vec![5, 6, 8],
            }
        );
    }
}
