//! Writing one write's notification events and their deliveries.

use crate::{
    error::ContextStoreError,
    notification_delivery::{
        change_code::{stored_change_kind, stored_lifecycle},
        rows::NewNotificationEvent,
    },
};
use andromeda_context_commit::write_record::WriteRecord;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::{
    notification::{NotificationDeliveries, NotificationEvents},
    subscriptions::Subscriptions,
};
use andromeda_postgresql_seaquery::{
    pg_array::nullable_text_array,
    pg_type::PostgresType,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use chrono::{DateTime, Utc};
use sea_query::{Alias, Expr, ExprTrait, InsertStatement, JoinType, LockType, OnConflict, PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

/// Persist the notification events of one write and the deliveries they owe.
///
/// Runs in the caller's transaction, which is the transaction that writes the
/// entities themselves: a subscriber is never told about a change that was
/// rolled back, and a change that landed never loses its notification.
///
/// Two statements, however many events and subscribers the write has: the
/// events are inserted in one statement and their surrogate keys read back,
/// then every `(event, subscription)` pair the write owes is inserted in one
/// set-write from a pair of parallel arrays. sea-query cannot express the
/// original single data-modifying CTE (`WITH inserted AS (INSERT … RETURNING)
/// INSERT …`), and the two statements inside the one transaction are
/// equivalent: the second names only the event keys the first returned.
///
/// An event with no matching subscription is not the caller's to pass: the
/// caller drops it, so no payload is written that no delivery references.
///
/// A subscription that disappears between matching and committing is filtered
/// out by the join the delivery insert makes against `subscriptions`, and
/// therefore produces no delivery row. That is the same outcome its deletion
/// would have produced a moment later through the cascade, and the event it
/// leaves behind is reaped as an orphan. The join key-share locks each row it
/// keeps, so a deletion still in flight is waited out rather than raced: the
/// write never fails on a subscription an unrelated request is deleting.
///
/// # Errors
///
/// Returns [`ContextStoreError::Internal`] when an event ordinal is outside the
/// range storage represents, and [`ContextStoreError::Database`] when a
/// statement fails.
pub async fn insert_events_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    record: &WriteRecord,
    events: &[NewNotificationEvent<'_>],
) -> Result<(), ContextStoreError> {
    if events.is_empty() {
        return Ok(());
    }

    let statement = insert_events_statement(target, record.recorded_at(), record.write_id().as_uuid(), events)?;
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    // The conflict clause suppresses a replayed event, so a replay reads back
    // only the events this transaction actually inserted; every one of them is
    // then owed its deliveries.
    let inserted: Vec<(i64, i32)> = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&mut **tx).await?;

    let (event_keys, subscription_keys) = owed_deliveries(events, &inserted);
    if event_keys.is_empty() {
        return Ok(());
    }
    let statement = insert_deliveries_statement(target, event_keys, subscription_keys, record.recorded_at())?;
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let (alias, columns) = pair_source();
    let sql = apply_column_alias(sql, &alias, &columns);
    sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(&mut **tx).await?;

    Ok(())
}

/// The parallel arrays of every delivery the inserted events owe: one
/// `(event key, subscription key)` pair per matched subscription of each event
/// actually written, in event-then-matcher order.
fn owed_deliveries(events: &[NewNotificationEvent<'_>], inserted: &[(i64, i32)]) -> (Vec<i64>, Vec<i64>) {
    inserted
        .iter()
        .filter_map(|(event_key, ordinal)| events.iter().find(|event| matches_ordinal(event, *ordinal)).map(|event| (*event_key, event)))
        .flat_map(|(event_key, event)| event.subscription_keys.iter().map(move |key| (event_key, key.get())))
        .unzip()
}

/// Whether `event` occupies `ordinal` within its write.
///
/// The ordinal is unique per write by `uq_notification_events_write_identity`,
/// so the match is one-to-one with the returned row.
fn matches_ordinal(event: &NewNotificationEvent<'_>, ordinal: i32) -> bool {
    event.ordinal.as_i32().is_ok_and(|value| value == ordinal)
}

/// Insert every event of one write in a single statement, returning the
/// surrogate key and ordinal of each event actually written.
///
/// `ON CONFLICT (write_id, event_ordinal) DO NOTHING` is what makes a commit
/// replayed under its original write identity insert no second copy of an event
/// it already wrote: a conflicted event returns no row, so the caller enqueues
/// no delivery for it either. The conflict target is the columns
/// `uq_notification_events_write_identity` is declared on, which is the unique
/// index PostgreSQL matches the conflict against.
fn insert_events_statement(
    target: &ContextTarget,
    recorded_at: DateTime<Utc>,
    write_id: Uuid,
    events: &[NewNotificationEvent<'_>],
) -> Result<InsertStatement, ContextStoreError> {
    let mut statement = Query::insert();
    statement.into_table(table_of(target, NotificationEvents::Table)).columns([
        NotificationEvents::CreatedAt,
        NotificationEvents::WriteId,
        NotificationEvents::EventOrdinal,
        NotificationEvents::Lifecycle,
        NotificationEvents::EntityId,
        NotificationEvents::ChangeKinds,
        NotificationEvents::ChangeAttributeNameKeys,
        NotificationEvents::ChangeDatasetIds,
        NotificationEvents::EntitySnapshot,
        NotificationEvents::PreviousSnapshot,
    ]);
    for event in events {
        let event_ordinal = event
            .ordinal
            .as_i32()
            .map_err(|error| ContextStoreError::Internal(format!("notification event ordinal: {error}")))?;
        let change_kinds: Vec<i16> = event.change.instances.iter().map(|instance| stored_change_kind(instance.kind).code()).collect();
        let change_attribute_name_keys: Vec<i32> = event.change_attribute_name_keys.iter().map(|id| id.get()).collect();
        let change_dataset_ids: Vec<Option<&str>> = event
            .change
            .instances
            .iter()
            .map(|instance| instance.key.dataset_id.as_ref().map(Uri::as_str))
            .collect();
        statement
            .values([
                recorded_at.into(),
                Expr::val(write_id),
                event_ordinal.into(),
                stored_lifecycle(event.change.lifecycle).code().into(),
                event.entity_id.as_str().into(),
                Expr::val(change_kinds),
                Expr::val(change_attribute_name_keys),
                // A default instance's `datasetId` is a NULL element, which one
                // bound array cannot carry.
                nullable_text_array(&change_dataset_ids),
                // The snapshot is bound by value because a bound parameter owns
                // its data; the entity state cannot be borrowed into the wire
                // buffer.
                snapshot_value(event.entity_snapshot),
                snapshot_value(event.previous_snapshot),
            ])
            .map_err(|error| ContextStoreError::Internal(format!("notification event row: {error}")))?;
    }
    statement
        .on_conflict(
            OnConflict::columns([NotificationEvents::WriteId, NotificationEvents::EventOrdinal])
                .do_nothing()
                .to_owned(),
        )
        .returning(Query::returning().columns([NotificationEvents::EventKey, NotificationEvents::EventOrdinal]));
    Ok(statement.clone())
}

/// Bind one optional entity snapshot as a JSONB value or SQL `NULL`.
fn snapshot_value(snapshot: Option<&Value>) -> sea_query::SimpleExpr {
    Expr::val(snapshot.cloned())
}

/// The `UNNEST(…) AS p(event_key, subscription_key)` source the delivery
/// set-write reads its pairs from.
///
/// [`apply_column_alias`] rewrites the alias sea-query renders into this
/// column-list form; the names must match what the projection reads.
fn pair_source() -> (Alias, [UnnestColumn; 2]) {
    (
        Alias::new("p"),
        [
            UnnestColumn::new(NotificationDeliveries::EventKey, PostgresType::BigintArray),
            UnnestColumn::new(NotificationDeliveries::SubscriptionKey, PostgresType::BigintArray),
        ],
    )
}

/// Insert every delivery a write owes in one statement, from parallel arrays of
/// event keys and subscription keys.
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
/// due at the write's own ingestion instant so the queue's ordering reflects
/// the order changes were accepted in.
fn insert_deliveries_statement(
    target: &ContextTarget,
    event_keys: Vec<i64>,
    subscription_keys: Vec<i64>,
    next_attempt_at: DateTime<Utc>,
) -> Result<InsertStatement, ContextStoreError> {
    let (pairs, columns) = pair_source();
    let subscription = Alias::new("s");
    let source = Query::select()
        .column((pairs.clone(), NotificationDeliveries::EventKey))
        .column((subscription.clone(), Subscriptions::SubscriptionKey))
        .expr(Expr::val(next_attempt_at))
        .from_function(
            unnest_call([(&columns[0], Expr::val(event_keys)), (&columns[1], Expr::val(subscription_keys))]),
            pairs.clone(),
        )
        .join_as(
            JoinType::InnerJoin,
            table_of(target, Subscriptions::Table),
            subscription.clone(),
            // clone: sea-query takes each alias occurrence by value, and the
            // locking clause names the same alias again
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
        ])
        .select_from(source)
        .map_err(|error| ContextStoreError::Internal(format!("notification delivery insert: {error}")))?;
    Ok(statement.clone())
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::{
        event_insert::{insert_deliveries_statement, insert_events_statement, owed_deliveries, pair_source},
        rows::NewNotificationEvent,
    };
    use andromeda_context_commit::{subscription_key::SubscriptionKey, write_ordinal::WriteOrdinal};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        expanded::{
            attribute_change::{AttributeChange, AttributeChangeKind},
            instance_key::AttributeInstanceKey,
        },
    };
    use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
    use andromeda_postgresql_seaquery::unnest::apply_column_alias;
    use chrono::Utc;
    use sea_query::PostgresQueryBuilder;
    use serde_json::json;
    use uuid::Uuid;

    fn target() -> ContextTarget {
        ContextTarget::Live
    }

    fn entity() -> Uri {
        "urn:ngsi-ld:Vehicle:1".parse().expect("a valid entity URI")
    }

    fn subscription(key: i64) -> SubscriptionKey {
        SubscriptionKey::try_from(key).expect("a positive subscription key")
    }

    /// A modification that updated `speed` and deleted the `urn:dataset:a`
    /// instance of `name`.
    fn change() -> EntityChange {
        let instance = |attribute: &str, dataset: Option<&str>, kind: AttributeChangeKind| AttributeChange {
            key: AttributeInstanceKey {
                attr: attribute.parse().expect("a valid attribute name"),
                dataset_id: dataset.map(|raw| raw.parse().expect("a valid dataset URI")),
            },
            kind,
        };
        EntityChange::new(
            EntityLifecycle::Modified,
            vec![
                instance("https://example.org/speed", None, AttributeChangeKind::Updated),
                instance("https://example.org/name", Some("urn:dataset:a"), AttributeChangeKind::Deleted),
            ],
        )
    }

    fn event<'a>(entity: &'a Uri, change: &'a EntityChange, ordinal: u32, subscriptions: &[i64], snapshot: &'a serde_json::Value) -> NewNotificationEvent<'a> {
        NewNotificationEvent {
            ordinal: WriteOrdinal::at(ordinal),
            change,
            entity_id: entity,
            change_attribute_name_keys: vec![AttributeNameKey::from(7), AttributeNameKey::from(9)],
            entity_snapshot: Some(snapshot),
            previous_snapshot: None,
            subscription_keys: subscriptions.iter().map(|key| subscription(*key)).collect(),
        }
    }

    #[test]
    fn a_replayed_write_inserts_no_second_copy_of_an_event_it_already_wrote() {
        let entity = entity();
        let snapshot = json!({ "id": "urn:ngsi-ld:Vehicle:1" });
        let change = change();
        let events = [event(&entity, &change, 3, &[1], &snapshot)];
        let sql = insert_events_statement(&target(), Utc::now(), Uuid::nil(), &events)
            .expect("a representable event")
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"ON CONFLICT ("write_id", "event_ordinal") DO NOTHING"#), "{sql}");
        assert!(sql.contains(r#"RETURNING "event_key", "event_ordinal""#), "{sql}");
    }

    #[test]
    fn the_event_insert_carries_every_changed_instance_position_for_position() {
        let entity = entity();
        let change = change();
        let snapshot = json!({ "id": "urn:ngsi-ld:Vehicle:1" });
        let events = [event(&entity, &change, 3, &[1], &snapshot)];
        let sql = insert_events_statement(&target(), Utc::now(), Uuid::nil(), &events)
            .expect("a representable event")
            .to_string(PostgresQueryBuilder);
        // Modified is lifecycle number 2; the instances are updated (2) then
        // deleted (3), keyed 7 and 9, the first the default instance.
        assert!(sql.contains("ARRAY [2,3]") || sql.contains("ARRAY[2,3]"), "{sql}");
        assert!(sql.contains("ARRAY [7,9]") || sql.contains("ARRAY[7,9]"), "{sql}");
        assert!(
            sql.contains("array_cat(array_append('{}'::text[], NULL), array_append('{}'::text[], 'urn:dataset:a'))"),
            "{sql}"
        );
    }

    #[test]
    fn an_ordinal_beyond_the_stored_range_fails_rather_than_wrapping() {
        let entity = entity();
        let snapshot = json!({});
        let change = change();
        let event = event(&entity, &change, u32::MAX, &[1], &snapshot);
        assert!(insert_events_statement(&target(), Utc::now(), Uuid::nil(), &[event]).is_err());
    }

    #[test]
    fn every_delivery_of_every_written_event_is_inserted_from_one_pair_of_arrays() {
        let sql = insert_deliveries_statement(&target(), vec![42, 42, 43], vec![1, 2, 1], Utc::now())
            .expect("a valid insert")
            .to_string(PostgresQueryBuilder);
        let (alias, columns) = pair_source();
        let sql = apply_column_alias(sql, &alias, &columns);
        assert!(
            sql.contains(r#"INSERT INTO "notification_deliveries" ("event_key", "subscription_key", "next_attempt_at")"#),
            "{sql}"
        );
        assert!(
            sql.contains(r"FROM UNNEST(ARRAY [42,42,43]::bigint[], ARRAY [1,2,1]::bigint[]) AS p(event_key, subscription_key)"),
            "{sql}"
        );
        assert!(
            sql.contains(r#"INNER JOIN "subscriptions" AS "s" ON "s"."subscription_key" = "p"."subscription_key""#),
            "{sql}"
        );
        assert!(sql.contains(r#"SELECT "p"."event_key", "s"."subscription_key""#), "{sql}");
    }

    #[test]
    fn the_delivery_insert_key_share_locks_only_the_subscriptions_it_joins() {
        let sql = insert_deliveries_statement(&target(), vec![42], vec![1], Utc::now())
            .expect("a valid insert")
            .to_string(PostgresQueryBuilder);
        assert!(sql.ends_with(r#"FOR KEY SHARE OF "s""#), "{sql}");
    }

    #[test]
    fn the_owed_pairs_follow_event_then_matcher_order_and_skip_events_a_replay_did_not_write() {
        let entity = entity();
        let snapshot = json!({});
        let change = change();
        let events = [
            event(&entity, &change, 3, &[5, 6], &snapshot),
            event(&entity, &change, 4, &[7], &snapshot),
            event(&entity, &change, 5, &[8], &snapshot),
        ];
        let inserted = [(100_i64, 3_i32), (101, 5)];
        let (event_keys, subscription_keys) = owed_deliveries(&events, &inserted);
        pretty_assertions::assert_eq!(event_keys, vec![100, 100, 101]);
        pretty_assertions::assert_eq!(subscription_keys, vec![5, 6, 8]);
    }
}
