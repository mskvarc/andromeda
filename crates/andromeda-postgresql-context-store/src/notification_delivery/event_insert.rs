//! Writing one write's notification events and their deliveries.

use crate::{
    error::ContextStoreError,
    notification_delivery::{rows::NewNotificationEvent, trigger_code::stored_trigger},
};
use andromeda_context_commit::write_record::WriteRecord;
use serde::Serialize;
use serde_json::Value;
use sqlx::{Postgres, Transaction};

/// One event as the insert statement reads it back out of its JSONB argument.
///
/// The events of one write are handed to PostgreSQL as a single JSONB document
/// rather than as parallel arrays, because one column of an event is itself an
/// array — the dictionary keys of the attributes it changed — and `UNNEST`
/// cannot carry an array per row.
#[derive(Debug, Serialize)]
struct EventArgument<'a> {
    /// Position of the event within its write.
    event_ordinal: i32,
    /// Migration-owned number of the trigger condition.
    trigger: i16,
    /// Entity whose change the event records.
    entity_id: &'a str,
    /// Dictionary keys of the attributes the change touched.
    changed_attribute_name_ids: Vec<i32>,
    /// Entity state after the change.
    entity_snapshot: Option<&'a Value>,
    /// Entity state before the change.
    previous_snapshot: Option<&'a Value>,
    /// External identifiers of the subscriptions the event is due to.
    subscription_ids: Vec<&'a str>,
}

/// Persist the notification events of one write and the deliveries they owe.
///
/// Runs in the caller's transaction, which is the transaction that writes the
/// entities themselves: a subscriber is never told about a change that was
/// rolled back, and a change that landed never loses its notification.
///
/// Set-based — one statement regardless of how many entities the write touched
/// or how many subscriptions each of them matched. An event with no matching
/// subscription is not the caller's to pass: the caller drops it, so no payload
/// is written that no delivery references.
///
/// A subscription that disappears between matching and committing resolves to
/// no key and therefore to no delivery row. That is the same outcome its
/// deletion would have produced a moment later through the cascade, and the
/// event it leaves behind is reaped as an orphan.
///
/// # Errors
///
/// Returns [`ContextStoreError::Internal`] when an event ordinal is outside the
/// range storage represents or its argument cannot be serialised, and
/// [`ContextStoreError::Database`] when the statement fails.
pub async fn insert_events_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    record: &WriteRecord,
    events: &[NewNotificationEvent<'_>],
) -> Result<(), ContextStoreError> {
    if events.is_empty() {
        return Ok(());
    }

    let arguments = events.iter().map(argument).collect::<Result<Vec<EventArgument<'_>>, ContextStoreError>>()?;
    let document =
        serde_json::to_value(&arguments).map_err(|error| ContextStoreError::Internal(format!("notification events are not serialisable: {error}")))?;

    sqlx::query(INSERT_EVENTS_SQL)
        .bind(document)
        .bind(record.recorded_at())
        .bind(record.write_id().as_uuid())
        .execute(&mut **tx)
        .await?;

    Ok(())
}

/// Project one event into the shape its JSONB argument carries.
fn argument<'a>(event: &'a NewNotificationEvent<'a>) -> Result<EventArgument<'a>, ContextStoreError> {
    Ok(EventArgument {
        event_ordinal: event
            .ordinal
            .as_i32()
            .map_err(|error| ContextStoreError::Internal(format!("notification event ordinal: {error}")))?,
        trigger: stored_trigger(event.trigger).code(),
        entity_id: event.entity_id.as_str(),
        changed_attribute_name_ids: event.changed_attribute_name_ids.iter().map(|id| id.get()).collect(),
        entity_snapshot: event.entity_snapshot,
        previous_snapshot: event.previous_snapshot,
        subscription_ids: event.subscription_ids.iter().map(|id| id.as_str()).collect(),
    })
}

/// Statement writing one write's events and every delivery they owe.
///
/// The deliveries are inserted from the events the first branch returned, so
/// the child rows can never name an event the statement did not write. The
/// `event_ordinal` is what joins the two back together: it is unique within the
/// write by the table's own constraint, which makes it a key here as well as a
/// replay identity.
///
/// `ON CONFLICT ON CONSTRAINT uq_notification_events_write_identity DO NOTHING`
/// is what makes a commit replayed under its original write identity insert no
/// second copy of an event it already wrote. Because the deliveries insert is
/// built solely from the rows this statement's own `RETURNING` clause names, a
/// conflicted event contributes no delivery either: a replay that finds every
/// one of its events already present enqueues nothing at all.
///
/// A delivery starts due immediately, at the write's own ingestion instant, so
/// that the queue's ordering by `next_attempt_at` reflects the order the
/// changes were accepted in rather than the order a worker happened to insert
/// them.
const INSERT_EVENTS_SQL: &str = r#"
WITH argument AS (
    SELECT *
    FROM jsonb_to_recordset($1::jsonb) AS e(
        event_ordinal integer,
        trigger smallint,
        entity_id text,
        changed_attribute_name_ids integer[],
        entity_snapshot jsonb,
        previous_snapshot jsonb,
        subscription_ids text[]
    )
), inserted AS (
    INSERT INTO notification_events (
        created_at, write_id, event_ordinal, trigger, entity_id,
        changed_attribute_name_ids, entity_snapshot, previous_snapshot
    )
    SELECT $2, $3, a.event_ordinal, a.trigger, a.entity_id,
           COALESCE(a.changed_attribute_name_ids, '{}'::integer[]),
           a.entity_snapshot, a.previous_snapshot
    FROM argument a
    ON CONFLICT ON CONSTRAINT uq_notification_events_write_identity DO NOTHING
    RETURNING id, event_ordinal
)
INSERT INTO notification_deliveries (event_id, subscription_key, next_attempt_at)
SELECT i.id, s.subscription_key, $2
FROM inserted i
JOIN argument a ON a.event_ordinal = i.event_ordinal
JOIN UNNEST(a.subscription_ids) AS matched(subscription_id) ON TRUE
JOIN subscriptions s ON s.id = matched.subscription_id
"#;

#[cfg(test)]
mod tests {
    use crate::notification_delivery::{
        event_insert::{INSERT_EVENTS_SQL, argument},
        rows::NewNotificationEvent,
    };
    use andromeda_context_commit::write_ordinal::WriteOrdinal;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::notification::NotificationTrigger;
    use andromeda_postgresql_query::attribute_name_id::AttributeNameId;
    use serde_json::json;

    fn entity() -> Uri {
        "urn:ngsi-ld:Vehicle:1".parse().expect("a valid entity URI")
    }

    fn subscription() -> Uri {
        "urn:ngsi-ld:Subscription:1".parse().expect("a valid subscription URI")
    }

    #[test]
    fn an_event_argument_carries_its_dictionary_keys_and_subscriptions() {
        let snapshot = json!({ "id": "urn:ngsi-ld:Vehicle:1" });
        let entity = entity();
        let subscription = subscription();
        let event = NewNotificationEvent {
            ordinal: WriteOrdinal::at(3),
            trigger: NotificationTrigger::AttributeUpdated,
            entity_id: &entity,
            changed_attribute_name_ids: vec![AttributeNameId::from(7), AttributeNameId::from(9)],
            entity_snapshot: Some(&snapshot),
            previous_snapshot: None,
            subscription_ids: vec![&subscription],
        };
        let argument = argument(&event).expect("a representable ordinal");
        pretty_assertions::assert_eq!(argument.event_ordinal, 3);
        pretty_assertions::assert_eq!(argument.trigger, 5);
        pretty_assertions::assert_eq!(argument.changed_attribute_name_ids, vec![7, 9]);
        pretty_assertions::assert_eq!(argument.subscription_ids, vec!["urn:ngsi-ld:Subscription:1"]);
        assert!(argument.previous_snapshot.is_none());
    }

    #[test]
    fn an_ordinal_beyond_the_stored_range_fails_rather_than_wrapping() {
        let entity = entity();
        let event = NewNotificationEvent {
            ordinal: WriteOrdinal::at(u32::MAX),
            trigger: NotificationTrigger::EntityCreated,
            entity_id: &entity,
            changed_attribute_name_ids: Vec::new(),
            entity_snapshot: None,
            previous_snapshot: None,
            subscription_ids: Vec::new(),
        };
        assert!(argument(&event).is_err());
    }

    #[test]
    fn the_deliveries_are_inserted_from_the_events_the_statement_wrote() {
        assert!(INSERT_EVENTS_SQL.contains("FROM inserted i"));
        assert!(
            INSERT_EVENTS_SQL.contains("JOIN argument a ON a.event_ordinal = i.event_ordinal"),
            "a delivery must be matched to the event row the insert returned"
        );
    }

    /// A commit replayed under its original write identity re-inserts every
    /// event it already wrote; the row-level conflict clause is what makes
    /// that second insert a no-op instead of a duplicate or an error.
    #[test]
    fn a_replayed_write_inserts_no_second_copy_of_an_event_it_already_wrote() {
        assert!(INSERT_EVENTS_SQL.contains("ON CONFLICT ON CONSTRAINT uq_notification_events_write_identity DO NOTHING"));
    }

    /// Because the conflict clause sits between the `INSERT` and its
    /// `RETURNING`, a conflicted event returns no row from the `inserted` CTE
    /// -- and since the deliveries insert is sourced solely from that CTE, a
    /// replay that finds its event already present enqueues no delivery either.
    #[test]
    fn the_conflict_clause_precedes_the_returning_clause_the_deliveries_insert_depends_on() {
        let conflict_at = INSERT_EVENTS_SQL.find("ON CONFLICT").expect("the statement declares a conflict clause");
        let returning_at = INSERT_EVENTS_SQL
            .find("RETURNING id, event_ordinal")
            .expect("the statement returns the written events");
        assert!(conflict_at < returning_at);
    }

    #[test]
    fn the_subscription_surrogate_key_is_resolved_inside_the_statement() {
        assert!(INSERT_EVENTS_SQL.contains("JOIN subscriptions s ON s.id = matched.subscription_id"));
        assert!(INSERT_EVENTS_SQL.contains("s.subscription_key"));
    }
}
