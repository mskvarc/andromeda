//! Writing one write's notification events and their deliveries.

use crate::{
    error::ContextStoreError,
    notification_delivery::{rows::NewNotificationEvent, trigger_code::stored_trigger},
};
use andromeda_context_commit::write_record::WriteRecord;
use andromeda_postgresql_schema::{
    notification::{NotificationDeliveries, NotificationEvents},
    subscriptions::Subscriptions,
};
use andromeda_postgresql_seaquery::pg_array::{any, text_array};
use chrono::{DateTime, Utc};
use sea_query::{Expr, ExprTrait, InsertStatement, OnConflict, PostgresQueryBuilder, Query};
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
/// The events are inserted in one statement and their surrogate keys read back;
/// each written event then has its deliveries inserted from the subscriptions it
/// matched. sea-query cannot express the original single data-modifying CTE
/// (`WITH inserted AS (INSERT … RETURNING) INSERT …`), so the two inserts run as
/// separate statements inside the one transaction, which is equivalent: the
/// second insert names only the event keys the first returned.
///
/// An event with no matching subscription is not the caller's to pass: the
/// caller drops it, so no payload is written that no delivery references.
///
/// A subscription that disappears between matching and committing resolves to
/// no key and therefore to no delivery row. That is the same outcome its
/// deletion would have produced a moment later through the cascade, and the
/// event it leaves behind is reaped as an orphan.
///
/// # Errors
///
/// Returns [`ContextStoreError::Internal`] when an event ordinal is outside the
/// range storage represents, and [`ContextStoreError::Database`] when a
/// statement fails.
pub async fn insert_events_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    record: &WriteRecord,
    events: &[NewNotificationEvent<'_>],
) -> Result<(), ContextStoreError> {
    if events.is_empty() {
        return Ok(());
    }

    let statement = insert_events_statement(record.recorded_at(), record.write_id().as_uuid(), events)?;
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    // The conflict clause suppresses a replayed event, so a replay reads back
    // only the events this transaction actually inserted; every one of them is
    // then owed its deliveries.
    let inserted: Vec<(i64, i32)> = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&mut **tx).await?;

    for (event_id, event_ordinal) in inserted {
        let Some(event) = events.iter().find(|event| matches_ordinal(event, event_ordinal)) else {
            continue;
        };
        let subscription_ids: Vec<&str> = event.subscription_ids.iter().map(|id| id.as_str()).collect();
        if subscription_ids.is_empty() {
            continue;
        }
        let statement = insert_deliveries_statement(event_id, record.recorded_at(), &subscription_ids)?;
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(&mut **tx).await?;
    }

    Ok(())
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
fn insert_events_statement(recorded_at: DateTime<Utc>, write_id: Uuid, events: &[NewNotificationEvent<'_>]) -> Result<InsertStatement, ContextStoreError> {
    let mut statement = Query::insert();
    statement.into_table(NotificationEvents::Table).columns([
        NotificationEvents::CreatedAt,
        NotificationEvents::WriteId,
        NotificationEvents::EventOrdinal,
        NotificationEvents::Trigger,
        NotificationEvents::EntityId,
        NotificationEvents::ChangedAttributeNameIds,
        NotificationEvents::EntitySnapshot,
        NotificationEvents::PreviousSnapshot,
    ]);
    for event in events {
        let event_ordinal = event
            .ordinal
            .as_i32()
            .map_err(|error| ContextStoreError::Internal(format!("notification event ordinal: {error}")))?;
        let changed_attribute_name_ids: Vec<i32> = event.changed_attribute_name_ids.iter().map(|id| id.get()).collect();
        statement
            .values([
                recorded_at.into(),
                Expr::val(write_id),
                event_ordinal.into(),
                stored_trigger(event.trigger).code().into(),
                event.entity_id.as_str().into(),
                Expr::val(changed_attribute_name_ids),
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
        .returning(Query::returning().columns([NotificationEvents::Id, NotificationEvents::EventOrdinal]));
    Ok(statement.to_owned())
}

/// Bind one optional entity snapshot as a JSONB value or SQL `NULL`.
fn snapshot_value(snapshot: Option<&Value>) -> sea_query::SimpleExpr {
    Expr::val(snapshot.cloned())
}

/// Insert the deliveries one written event owes, resolving each matched
/// subscription's external identifier to the surrogate key the queue references.
///
/// A subscription that vanished between the match and this insert resolves to no
/// row, so the event keeps no delivery for it and is later reaped as an orphan.
/// A delivery starts due at the write's own ingestion instant so the queue's
/// ordering reflects the order changes were accepted in.
fn insert_deliveries_statement(event_id: i64, next_attempt_at: DateTime<Utc>, subscription_ids: &[&str]) -> Result<InsertStatement, ContextStoreError> {
    let source = Query::select()
        .expr(Expr::val(event_id))
        .expr(Expr::col((Subscriptions::Table, Subscriptions::SubscriptionKey)))
        .expr(Expr::val(next_attempt_at))
        .from(Subscriptions::Table)
        .and_where(Expr::col((Subscriptions::Table, Subscriptions::Id)).eq(any(text_array(subscription_ids))))
        .to_owned();
    let mut statement = Query::insert();
    statement
        .into_table(NotificationDeliveries::Table)
        .columns([
            NotificationDeliveries::EventId,
            NotificationDeliveries::SubscriptionKey,
            NotificationDeliveries::NextAttemptAt,
        ])
        .select_from(source)
        .map_err(|error| ContextStoreError::Internal(format!("notification delivery insert: {error}")))?;
    Ok(statement.to_owned())
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::{
        event_insert::{insert_deliveries_statement, insert_events_statement},
        rows::NewNotificationEvent,
    };
    use andromeda_context_commit::write_ordinal::WriteOrdinal;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::notification::NotificationTrigger;
    use andromeda_postgresql_query::attribute_name_id::AttributeNameId;
    use chrono::Utc;
    use sea_query::PostgresQueryBuilder;
    use serde_json::json;
    use uuid::Uuid;

    fn entity() -> Uri {
        "urn:ngsi-ld:Vehicle:1".parse().expect("a valid entity URI")
    }

    fn subscription() -> Uri {
        "urn:ngsi-ld:Subscription:1".parse().expect("a valid subscription URI")
    }

    fn event<'a>(entity: &'a Uri, subscription: &'a Uri, snapshot: &'a serde_json::Value) -> NewNotificationEvent<'a> {
        NewNotificationEvent {
            ordinal: WriteOrdinal::at(3),
            trigger: NotificationTrigger::AttributeUpdated,
            entity_id: entity,
            changed_attribute_name_ids: vec![AttributeNameId::from(7), AttributeNameId::from(9)],
            entity_snapshot: Some(snapshot),
            previous_snapshot: None,
            subscription_ids: vec![subscription],
        }
    }

    #[test]
    fn a_replayed_write_inserts_no_second_copy_of_an_event_it_already_wrote() {
        let entity = entity();
        let subscription = subscription();
        let snapshot = json!({ "id": "urn:ngsi-ld:Vehicle:1" });
        let events = [event(&entity, &subscription, &snapshot)];
        let sql = insert_events_statement(Utc::now(), Uuid::nil(), &events)
            .expect("a representable event")
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"ON CONFLICT ("write_id", "event_ordinal") DO NOTHING"#), "{sql}");
        assert!(sql.contains(r#"RETURNING "id", "event_ordinal""#), "{sql}");
    }

    #[test]
    fn the_event_insert_carries_the_dictionary_keys_and_trigger_number() {
        let entity = entity();
        let subscription = subscription();
        let snapshot = json!({ "id": "urn:ngsi-ld:Vehicle:1" });
        let events = [event(&entity, &subscription, &snapshot)];
        let sql = insert_events_statement(Utc::now(), Uuid::nil(), &events)
            .expect("a representable event")
            .to_string(PostgresQueryBuilder);
        // AttributeUpdated is trigger number 5; the changed keys 7 and 9 are the
        // integer array bound for the event.
        assert!(sql.contains('5'), "{sql}");
        assert!(sql.contains("ARRAY [7,9]") || sql.contains("ARRAY[7,9]"), "{sql}");
    }

    #[test]
    fn an_ordinal_beyond_the_stored_range_fails_rather_than_wrapping() {
        let entity = entity();
        let subscription = subscription();
        let snapshot = json!({});
        let mut event = event(&entity, &subscription, &snapshot);
        event.ordinal = WriteOrdinal::at(u32::MAX);
        assert!(insert_events_statement(Utc::now(), Uuid::nil(), &[event]).is_err());
    }

    #[test]
    fn the_delivery_insert_resolves_the_subscription_surrogate_key() {
        let sql = insert_deliveries_statement(42, Utc::now(), &["urn:ngsi-ld:Subscription:1"])
            .expect("a valid insert")
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"INTO "notification_deliveries""#), "{sql}");
        assert!(sql.contains(r#""subscriptions""#), "{sql}");
        assert!(sql.contains(r#""subscription_key""#), "{sql}");
        assert!(sql.contains("= ANY("), "{sql}");
    }
}
