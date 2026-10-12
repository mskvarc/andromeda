//! Writing one write's notification events and their deliveries.

use crate::{
    entity_state_record::state_record::{EntityStateRecord, record_of},
    error::ContextStoreError,
    notification_delivery::{
        change_code::{stored_change_kind, stored_lifecycle},
        rows::NewNotificationEvent,
    },
};
use andromeda_context_commit::write_record::WriteRecord;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity_change::EntityLifecycle;
use andromeda_postgresql_attribute_instance::name_keys::NameKeys;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::{
    notification::{NotificationDeliveries, NotificationEvents},
    subscriptions::Subscriptions,
};
use andromeda_postgresql_seaquery::{
    pg_type::PostgresType,
    placeholder::{external_placeholder, external_placeholder_as},
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use chrono::{DateTime, Utc};
use sea_query::{Alias, Expr, ExprTrait, InsertStatement, JoinType, Keyword, LockType, OnConflict, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Postgres, Transaction, postgres::PgArguments, query::QueryAs};

/// Persist the notification events of one write and the deliveries they owe.
///
/// Runs in the caller's transaction, which is the transaction that writes the
/// entities themselves: a subscriber is never told about a change that was
/// rolled back, and a change that landed never loses its notification.
///
/// Two statements, however many events and subscribers the write has: the
/// events are inserted in one statement and their surrogate keys read back,
/// then every `(event, subscription)` pair the write owes is inserted in one
/// set-write from a pair of parallel arrays. sea-query cannot express a
/// single data-modifying CTE (`WITH inserted AS (INSERT … RETURNING)
/// INSERT …`), and the two statements inside the one transaction are
/// equivalent: the second names only the event keys the first returned.
///
/// Each state an event carries is stored as one `entity_state_record`, its
/// names as the keys `keys` holds for them; the caller resolved every name the
/// states mention in the same transaction.
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
/// Returns [`ContextStoreError::OrdinalOutOfRange`] when an event ordinal is
/// outside the range storage represents, [`ContextStoreError::StateRecord`]
/// when a state has no record under `keys`, and
/// [`ContextStoreError::Database`] when a statement fails.
pub async fn insert_events_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    record: &WriteRecord,
    events: &[NewNotificationEvent<'_>],
    keys: &NameKeys,
) -> Result<(), ContextStoreError> {
    if events.is_empty() {
        return Ok(());
    }

    let rows = events.iter().map(|event| EventRow::of(event, keys)).collect::<Result<Vec<_>, _>>()?;
    let sql = insert_events_statement(target, &rows)?.to_string(PostgresQueryBuilder);
    let mut insert = sqlx::query_as::<_, (i64, i32)>(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(record.recorded_at())
        .bind(record.write_id().as_uuid());
    for row in rows {
        insert = row.bind(insert);
    }
    // The conflict clause suppresses a replayed event, so a replay reads back
    // only the events this transaction actually inserted; every one of them is
    // then owed its deliveries.
    let inserted: Vec<(i64, i32)> = insert.fetch_all(&mut **tx).await?;

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

/// The values one event row binds, after the write's instant and identity,
/// which every row shares as `$1` and `$2`.
#[derive(Debug)]
struct EventRow<'a> {
    /// `event_ordinal`.
    event_ordinal: i32,
    /// `lifecycle`.
    lifecycle: i16,
    /// `entity_id`.
    entity_id: &'a str,
    /// `change_kinds`.
    change_kinds: Vec<i16>,
    /// `change_attribute_name_keys`.
    change_attribute_name_keys: Vec<i32>,
    /// `change_dataset_ids`, a NULL element for a default instance.
    change_dataset_ids: Vec<Option<&'a str>>,
    /// `entity_snapshot`, bound only when present.
    entity_snapshot: Option<EntityStateRecord>,
    /// `previous_snapshot`, bound only when present.
    previous_snapshot: Option<EntityStateRecord>,
    /// `entity_created_at`.
    entity_created_at: Option<DateTime<Utc>>,
    /// `entity_modified_at`.
    entity_modified_at: Option<DateTime<Utc>>,
    /// `entity_expires_at`.
    entity_expires_at: Option<DateTime<Utc>>,
}

impl<'a> EventRow<'a> {
    /// The row `event` is stored as.
    ///
    /// A deletion's removed state is its previous state too, so the state is
    /// stored once, as `entity_snapshot`, and a claim reads it back as both.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the ordinal is out of the stored
    /// range or a state has no record under `keys`.
    fn of(event: &NewNotificationEvent<'a>, keys: &NameKeys) -> Result<EventRow<'a>, ContextStoreError> {
        let (entity_snapshot, previous_snapshot) = match event.change.lifecycle {
            EntityLifecycle::Deleted => (event.entity_snapshot.or(event.previous_snapshot), None),
            EntityLifecycle::Created | EntityLifecycle::Modified => (event.entity_snapshot, event.previous_snapshot),
        };
        let record = |state: Option<&EntityState>| state.map(|state| record_of(state, keys)).transpose();
        Ok(EventRow {
            event_ordinal: event.ordinal.as_i32()?,
            lifecycle: stored_lifecycle(event.change.lifecycle).code(),
            entity_id: event.entity_id.as_str(),
            change_kinds: event.change.instances.iter().map(|instance| stored_change_kind(instance.kind).code()).collect(),
            change_attribute_name_keys: event.change_attribute_name_keys.iter().map(|key| key.get()).collect(),
            change_dataset_ids: event
                .change
                .instances
                .iter()
                .map(|instance| instance.key.dataset_id.as_ref().map(Identifier::as_str))
                .collect(),
            entity_snapshot: record(entity_snapshot)?,
            previous_snapshot: record(previous_snapshot)?,
            entity_created_at: event.sys_attrs_snapshot.map(|sys_attrs| sys_attrs.created_at),
            entity_modified_at: event.sys_attrs_snapshot.map(|sys_attrs| sys_attrs.modified_at),
            entity_expires_at: event.sys_attrs_snapshot.and_then(|sys_attrs| sys_attrs.expires_at),
        })
    }

    /// The row's markers in column order, numbered on from `markers`; an
    /// absent state is the literal `NULL` and takes no marker.
    fn values(&self, markers: &mut Markers) -> [SimpleExpr; 13] {
        let state = |present: bool, markers: &mut Markers| {
            if present {
                markers.next_as(PostgresType::EntityStateRecord)
            } else {
                SimpleExpr::Keyword(Keyword::Null)
            }
        };
        [
            external_placeholder(RECORDED_AT),
            external_placeholder(WRITE_ID),
            markers.next(),
            markers.next(),
            markers.next(),
            markers.next(),
            markers.next(),
            markers.next(),
            state(self.entity_snapshot.is_some(), markers),
            state(self.previous_snapshot.is_some(), markers),
            markers.next(),
            markers.next(),
            markers.next(),
        ]
    }

    /// Bind the row's values onto `insert` in the order [`EventRow::values`]
    /// numbers them.
    fn bind<'q>(self, insert: QueryAs<'q, Postgres, (i64, i32), PgArguments>) -> QueryAs<'q, Postgres, (i64, i32), PgArguments>
    where
        'a: 'q,
    {
        let insert = insert
            .bind(self.event_ordinal)
            .bind(self.lifecycle)
            .bind(self.entity_id)
            .bind(self.change_kinds)
            .bind(self.change_attribute_name_keys)
            .bind(self.change_dataset_ids);
        let insert = match self.entity_snapshot {
            Some(state) => insert.bind(state),
            None => insert,
        };
        let insert = match self.previous_snapshot {
            Some(state) => insert.bind(state),
            None => insert,
        };
        insert.bind(self.entity_created_at).bind(self.entity_modified_at).bind(self.entity_expires_at)
    }
}

/// The marker of the write's instant, which every event row records as its
/// `created_at`.
const RECORDED_AT: usize = 1;

/// The marker of the write's identity, which every event row records.
const WRITE_ID: usize = 2;

/// The next free positional marker of a statement whose values are all bound
/// outside sea-query.
#[derive(Debug)]
struct Markers(usize);

impl Markers {
    /// Markers numbered on after the two every row shares.
    const fn after_shared() -> Markers {
        Markers(WRITE_ID + 1)
    }

    /// The next marker.
    fn next(&mut self) -> SimpleExpr {
        let marker = external_placeholder(self.0);
        self.0 += 1;
        marker
    }

    /// The next marker, cast to `ty`.
    fn next_as(&mut self, ty: PostgresType) -> SimpleExpr {
        let marker = external_placeholder_as(self.0, ty);
        self.0 += 1;
        marker
    }
}

/// Insert every event of one write in a single statement, returning the
/// surrogate key and ordinal of each event actually written.
///
/// Every value is a bare marker bound by the caller: a state is one
/// `entity_state_record`, a composite sea-query cannot carry, and a change's
/// `datasetId`s an array with NULL elements, which it cannot carry either.
///
/// `ON CONFLICT (write_id, event_ordinal) DO NOTHING` is what makes a commit
/// replayed under its original write identity insert no second copy of an event
/// it already wrote: a conflicted event returns no row, so the caller enqueues
/// no delivery for it either. The conflict target is the columns
/// `uq_notification_events_write_identity` is declared on, which is the unique
/// index PostgreSQL matches the conflict against.
fn insert_events_statement(target: &ContextTarget, rows: &[EventRow<'_>]) -> Result<InsertStatement, ContextStoreError> {
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
        NotificationEvents::EntityCreatedAt,
        NotificationEvents::EntityModifiedAt,
        NotificationEvents::EntityExpiresAt,
    ]);
    let mut markers = Markers::after_shared();
    for row in rows {
        statement.values(row.values(&mut markers))?;
    }
    statement
        .on_conflict(
            OnConflict::columns([NotificationEvents::WriteId, NotificationEvents::EventOrdinal])
                .do_nothing()
                .to_owned(),
        )
        .returning(Query::returning().columns([NotificationEvents::EventKey, NotificationEvents::EventOrdinal]));
    Ok(statement)
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
        .select_from(source)?;
    Ok(statement.clone())
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::{
        event_insert::{EventRow, insert_deliveries_statement, insert_events_statement, owed_deliveries, pair_source},
        rows::NewNotificationEvent,
    };
    use andromeda_context_commit::{subscription_key::SubscriptionKey, write_ordinal::WriteOrdinal};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_entity::{
        entity_state::EntityState,
        entity_types::EntityTypes,
        reads::{
            attribute_change::{AttributeChange, AttributeChangeKind},
            attribute_instance_key::AttributeInstanceKey,
        },
    };
    use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        notification::SysAttrsSnapshot,
    };
    use andromeda_postgresql_attribute_instance::name_keys::NameKeys;
    use andromeda_postgresql_query::{
        attribute_name_key::AttributeNameKey,
        dictionary::resolved_names::ResolvedNames,
        entity_type_name_key::EntityTypeNameKey,
    };
    use andromeda_postgresql_seaquery::unnest::apply_column_alias;
    use chrono::{TimeZone, Utc};
    use sea_query::PostgresQueryBuilder;

    const VEHICLE: &str = "https://example.org/Vehicle";

    fn target() -> ContextTarget {
        ContextTarget::Live
    }

    fn entity() -> Identifier {
        "urn:ngsi-ld:Vehicle:1".parse().expect("a valid entity URI")
    }

    /// The `Vehicle` entity the events record a change of.
    fn vehicle() -> EntityState {
        EntityState::new(entity(), EntityTypes::one(VEHICLE.parse().expect("a valid type name")))
    }

    /// The keys the commit resolved for the `Vehicle` state.
    fn keys() -> NameKeys {
        let vehicle: EntityTypeName = VEHICLE.parse().expect("a valid type name");
        NameKeys::new(ResolvedNames::default(), ResolvedNames::from_pairs([(vehicle, EntityTypeNameKey::from(4))]))
    }

    fn subscription(key: i64) -> SubscriptionKey {
        SubscriptionKey::try_from(key).expect("a positive subscription key")
    }

    /// A change through `lifecycle` that updated `speed` and deleted the
    /// `urn:dataset:a` instance of `name`.
    fn change(lifecycle: EntityLifecycle) -> EntityChange {
        let instance = |attribute: &str, dataset: Option<&str>, kind: AttributeChangeKind| AttributeChange {
            key: AttributeInstanceKey {
                attr: attribute.parse().expect("a valid attribute name"),
                dataset_id: dataset.map(|raw| raw.parse().expect("a valid dataset URI")),
            },
            kind,
        };
        EntityChange::new(
            lifecycle,
            vec![
                instance("https://example.org/speed", None, AttributeChangeKind::Updated),
                instance("https://example.org/name", Some("urn:dataset:a"), AttributeChangeKind::Deleted),
            ],
        )
    }

    fn event<'a>(entity: &'a Identifier, change: &'a EntityChange, ordinal: u32, subscriptions: &[i64], snapshot: &'a EntityState) -> NewNotificationEvent<'a> {
        NewNotificationEvent {
            ordinal: WriteOrdinal::at(ordinal),
            change,
            entity_id: entity,
            change_attribute_name_keys: vec![AttributeNameKey::from(7), AttributeNameKey::from(9)],
            entity_snapshot: Some(snapshot),
            previous_snapshot: None,
            sys_attrs_snapshot: None,
            subscription_keys: subscriptions.iter().map(|key| subscription(*key)).collect(),
        }
    }

    fn statement(events: &[NewNotificationEvent<'_>]) -> String {
        let rows = events
            .iter()
            .map(|event| EventRow::of(event, &keys()).expect("a representable event"))
            .collect::<Vec<_>>();
        insert_events_statement(&target(), &rows)
            .expect("a valid insert")
            .to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_replayed_write_inserts_no_second_copy_of_an_event_it_already_wrote() {
        let entity = entity();
        let snapshot = vehicle();
        let change = change(EntityLifecycle::Modified);
        let sql = statement(&[event(&entity, &change, 3, &[1], &snapshot)]);
        assert!(sql.contains(r#"ON CONFLICT ("write_id", "event_ordinal") DO NOTHING"#), "{sql}");
        assert!(sql.contains(r#"RETURNING "event_key", "event_ordinal""#), "{sql}");
    }

    /// Every row records the write's instant and identity, bound once.
    #[test]
    fn every_event_of_a_write_shares_its_instant_and_identity() {
        let entity = entity();
        let snapshot = vehicle();
        let change = change(EntityLifecycle::Modified);
        let sql = statement(&[event(&entity, &change, 3, &[1], &snapshot), event(&entity, &change, 4, &[1], &snapshot)]);
        assert!(sql.contains("VALUES ($1, $2, $3, $4,"), "{sql}");
        assert!(sql.contains("($1, $2, $13, $14,"), "{sql}");
    }

    #[test]
    fn the_event_row_carries_every_changed_instance_position_for_position() {
        let entity = entity();
        let change = change(EntityLifecycle::Modified);
        let snapshot = vehicle();
        let row = EventRow::of(&event(&entity, &change, 3, &[1], &snapshot), &keys()).expect("a representable event");
        // Modified is lifecycle number 2; the instances are updated (2) then
        // deleted (3), keyed 7 and 9, the first the default instance.
        pretty_assertions::assert_eq!(row.lifecycle, 2);
        pretty_assertions::assert_eq!(row.change_kinds, vec![2, 3]);
        pretty_assertions::assert_eq!(row.change_attribute_name_keys, vec![7, 9]);
        pretty_assertions::assert_eq!(row.change_dataset_ids, vec![None, Some("urn:dataset:a")]);
    }

    #[test]
    fn an_ordinal_beyond_the_stored_range_fails_rather_than_wrapping() {
        let entity = entity();
        let snapshot = vehicle();
        let change = change(EntityLifecycle::Modified);
        assert!(EventRow::of(&event(&entity, &change, u32::MAX, &[1], &snapshot), &keys()).is_err());
    }

    /// A state whose type the commit did not key has no record.
    #[test]
    fn a_state_whose_type_has_no_key_is_refused() {
        let entity = entity();
        let snapshot = vehicle();
        let change = change(EntityLifecycle::Modified);
        assert!(EventRow::of(&event(&entity, &change, 1, &[1], &snapshot), &NameKeys::default()).is_err());
    }

    #[test]
    fn a_snapshot_is_bound_as_one_entity_state_record() {
        let entity = entity();
        let change = change(EntityLifecycle::Modified);
        let (current, previous) = (vehicle(), vehicle());
        let mut modification = event(&entity, &change, 1, &[1], &current);
        modification.previous_snapshot = Some(&previous);
        let sql = statement(&[modification]);
        pretty_assertions::assert_eq!(sql.matches("::entity_state_record").count(), 2, "{sql}");
        assert!(sql.contains("$9::entity_state_record, $10::entity_state_record, $11, $12, $13)"), "{sql}");
        assert!(!sql.contains("json"), "{sql}");
    }

    /// Clause 5.8.6 notifies a deletion with the removed Entity, which is
    /// also its previous state: the state is stored once.
    #[test]
    fn a_deletion_writes_its_removed_state_once() {
        let entity = entity();
        let change = change(EntityLifecycle::Deleted);
        let removed = vehicle();
        let mut deletion = event(&entity, &change, 1, &[1], &removed);
        deletion.previous_snapshot = Some(&removed);
        let row = EventRow::of(&deletion, &keys()).expect("a representable event");
        assert!(row.entity_snapshot.is_some());
        assert!(row.previous_snapshot.is_none());
        let sql = statement(&[deletion]);
        assert!(sql.contains("$9::entity_state_record, NULL, $10, $11, $12)"), "{sql}");
    }

    /// A deletion handed its removed state as the previous state alone still
    /// stores it, as `entity_snapshot`.
    #[test]
    fn a_deletion_handed_only_its_previous_state_stores_it_as_the_removed_state() {
        let entity = entity();
        let change = change(EntityLifecycle::Deleted);
        let removed = vehicle();
        let mut deletion = event(&entity, &change, 1, &[1], &removed);
        deletion.entity_snapshot = None;
        deletion.previous_snapshot = Some(&removed);
        let row = EventRow::of(&deletion, &keys()).expect("a representable event");
        assert!(row.entity_snapshot.is_some_and(|record| record.entity_type_name_keys == vec![4]));
        assert!(row.previous_snapshot.is_none());
    }

    /// A deletion's removed entity can no longer be read back at delivery, so
    /// its clause 4.8 system attributes are written beside its snapshot.
    #[test]
    fn a_deletion_writes_the_removed_entitys_system_attributes() {
        let entity = entity();
        let change = change(EntityLifecycle::Deleted);
        let removed = vehicle();
        let created_at = Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant");
        let modified_at = Utc.timestamp_opt(1_700_000_100, 0).single().expect("a representable instant");
        let sys_attrs = SysAttrsSnapshot {
            created_at,
            modified_at,
            expires_at: None,
        };
        let mut deletion = event(&entity, &change, 1, &[1], &removed);
        deletion.sys_attrs_snapshot = Some(&sys_attrs);
        let row = EventRow::of(&deletion, &keys()).expect("a representable event");
        pretty_assertions::assert_eq!(
            (row.entity_created_at, row.entity_modified_at, row.entity_expires_at),
            (Some(created_at), Some(modified_at), None)
        );
    }

    /// A change that leaves the entity in place stores no system attributes
    /// and no snapshot it was not handed.
    #[test]
    fn a_change_without_system_attributes_stores_nulls() {
        let entity = entity();
        let change = change(EntityLifecycle::Modified);
        let current = vehicle();
        let row = EventRow::of(&event(&entity, &change, 1, &[1], &current), &keys()).expect("a representable event");
        assert!(row.previous_snapshot.is_none());
        assert!(row.entity_created_at.is_none() && row.entity_modified_at.is_none() && row.entity_expires_at.is_none());
        let sql = statement(&[event(&entity, &change, 1, &[1], &current)]);
        assert!(sql.contains("$9::entity_state_record, NULL, $10, $11, $12)"), "{sql}");
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
        let snapshot = vehicle();
        let change = change(EntityLifecycle::Modified);
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
