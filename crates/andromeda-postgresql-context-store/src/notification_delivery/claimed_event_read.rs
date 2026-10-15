//! Reading the events one claim's deliveries answer.
//!
//! A claim leases its deliveries first and reads their events second, inside
//! the same transaction, so the deliveries' row locks keep each event in place
//! while it is read. An event the claiming pass already holds is not read at
//! all: stored events never change, so the payload a sibling delivery carries
//! is the one the claim would read. Only after the transaction commits are the
//! states' dictionary keys named, one lookup per dictionary for the claim.

use crate::{
    error::ContextStoreError,
    notification_delivery::claimed_event::{ClaimedEvents, RawClaimedEventRow, ReadEvents},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_notification::{
    held_events::HeldEvents,
    notification_event::{NotificationEvent, NotificationEventKey},
};
use andromeda_postgresql_attribute_instance::name_resolution::names_of;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use andromeda_postgresql_schema::{name_dictionary::AttributeNames, notification::NotificationEvents};
use andromeda_postgresql_seaquery::{
    aggregate::array_agg_ordered,
    pg_array::{any, text_array},
    pg_type::PostgresType,
    unnest::{UnnestColumn, apply_with_ordinality, unnest_call},
};
use indexmap::IndexSet;
use sea_query::{Alias, Expr, ExprTrait, Func, Iden, JoinType, PostgresQueryBuilder, Query, QueryStatementBuilder, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Postgres, Transaction};
use std::sync::Arc;

/// The dictionaries a claim reads the names of its events' states back
/// through.
#[derive(Debug, Clone, Copy)]
pub struct ClaimDictionaries<'a> {
    /// The `attribute_names` dictionary.
    pub attribute_names: &'a dyn NameLookup<AttributeName>,
    /// The `entity_type_names` dictionary.
    pub entity_type_names: &'a dyn NameLookup<EntityTypeName>,
}

/// The events one claim's deliveries answer, before their states are named:
/// those the pass already held, and the rows the claim read for the rest.
#[derive(Debug)]
pub(crate) struct ClaimedEventRead {
    /// Events the claiming pass held, handed over unread.
    reused: Vec<Arc<NotificationEvent>>,
    /// Rows of the events the claim read.
    rows: Vec<RawClaimedEventRow>,
}

impl ClaimedEventRead {
    /// Read, inside the claim's transaction `tx`, each event under
    /// `event_keys` in `target`'s branch that `held` does not hold; no
    /// statement runs when `held` holds them all.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Database`] when the event read fails.
    pub(crate) async fn read(
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        event_keys: impl IntoIterator<Item = i64>,
        held: &HeldEvents,
    ) -> Result<ClaimedEventRead, ContextStoreError> {
        let (reused, unread) = split_held(distinct_event_keys(event_keys), held, target);
        let rows = if unread.is_empty() {
            Vec::new()
        } else {
            let (sql, values) = claimed_events_statement(target, &unread).build_sqlx(PostgresQueryBuilder);
            let (unnest_alias, unnest_columns, ordinality) = ordinality_alias();
            let sql = apply_with_ordinality(sql, &unnest_alias, &unnest_columns, &ordinality);
            sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&mut **tx).await?
        };
        Ok(ClaimedEventRead { reused, rows })
    }

    /// Decode the events the claim read, naming their states through
    /// `dictionaries`, hold each decoded one in `held` under `target`'s branch
    /// for the claims after this one, and join the events the pass already
    /// held. No dictionary is asked when the claim read no event.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::NameLookup`] when a dictionary cannot be
    /// read.
    pub(crate) async fn assemble(
        self,
        dictionaries: ClaimDictionaries<'_>,
        held: &HeldEvents,
        target: &ContextTarget,
    ) -> Result<ClaimedEvents, ContextStoreError> {
        let ClaimedEventRead { reused, rows } = self;
        let events = if rows.is_empty() {
            ClaimedEvents::default()
        } else {
            let read = ReadEvents::read(rows);
            let names = names_of(&read.held_keys(), dictionaries.attribute_names, dictionaries.entity_type_names)
                .await
                .map_err(ContextStoreError::NameLookup)?;
            read.assemble(&names)
        };
        events.hold_into(held, target);
        Ok(events.with_reused(reused))
    }
}

/// The events a claim's deliveries answer, each once, in the order the
/// deliveries first name them.
fn distinct_event_keys(event_keys: impl IntoIterator<Item = i64>) -> Vec<i64> {
    event_keys.into_iter().collect::<IndexSet<i64>>().into_iter().collect()
}

/// The events among `event_keys` that `held` already holds for `target`'s
/// branch, and the keys of the rest, which the claim reads.
fn split_held(event_keys: Vec<i64>, held: &HeldEvents, target: &ContextTarget) -> (Vec<Arc<NotificationEvent>>, Vec<i64>) {
    let mut reused = Vec::new();
    let mut unread = Vec::new();
    for key in event_keys {
        match held.get(target, NotificationEventKey::from(key)) {
            Some(event) => reused.push(event),
            None => unread.push(key),
        }
    }
    (reused, unread)
}

/// The `UNNEST … WITH ORDINALITY AS k(name_key, position)` names that
/// [`apply_with_ordinality`] rewrites the projection's set-returning source to.
///
/// The one row's `name_key` is the dictionary key `UNNEST` yields and `position`
/// is the ordinality that preserves the order the event recorded its attributes
/// in; both must match the names the projection's aggregate reads.
fn ordinality_alias() -> (Alias, [UnnestColumn; 1], Alias) {
    (
        Alias::new("k"),
        [UnnestColumn::new(Alias::new("name_key"), PostgresType::IntegerArray)],
        Alias::new("position"),
    )
}

/// Read each event under `event_keys` once, in `target`'s branch.
///
/// The changed attributes are resolved back through the tenant-global
/// `attribute_names` dictionary in event-recorded order by a lateral aggregate
/// over the event's `change_attribute_name_keys`, defaulting to an empty array
/// when the event touched none. The aggregate keeps one name per key, repeats
/// included, so it stays position for position with the event's transitions
/// and `datasetId`s.
fn claimed_events_statement(target: &ContextTarget, event_keys: &[i64]) -> SelectStatement {
    let (unnest_alias, unnest_columns, position) = ordinality_alias();

    let ordered_names = Query::select()
        .expr(array_agg_ordered(
            Expr::col((ChangedName, AttributeNames::Name)),
            // clone: the aggregate and the join below name the same unnested relation.
            Expr::col((unnest_alias.clone(), position)),
        ))
        .from_function(
            unnest_call([(&unnest_columns[0], Expr::col((ClaimedEventRow, NotificationEvents::ChangeAttributeNameKeys)))]),
            // clone: the unnested relation is named by its function call and by its join.
            unnest_alias.clone(),
        )
        .join_as(
            JoinType::Join,
            AttributeNames::Table,
            ChangedName,
            Expr::col((ChangedName, AttributeNames::AttributeNameKey)).equals((unnest_alias, Alias::new("name_key"))),
        )
        .to_owned();

    let change_attributes = Func::coalesce([SimpleExpr::SubQuery(None, Box::new(ordered_names.into_sub_query_statement())), text_array(&[])]);

    Query::select()
        .column((ClaimedEventRow, NotificationEvents::EventKey))
        .column((ClaimedEventRow, NotificationEvents::WriteId))
        .column((ClaimedEventRow, NotificationEvents::EventOrdinal))
        .column((ClaimedEventRow, NotificationEvents::Lifecycle))
        .column((ClaimedEventRow, NotificationEvents::EntityId))
        .column((ClaimedEventRow, NotificationEvents::ChangeKinds))
        .expr_as(change_attributes, Alias::new("change_attributes"))
        .column((ClaimedEventRow, NotificationEvents::ChangeDatasetIds))
        .column((ClaimedEventRow, NotificationEvents::EntitySnapshot))
        .column((ClaimedEventRow, NotificationEvents::PreviousSnapshot))
        .column((ClaimedEventRow, NotificationEvents::EntityCreatedAt))
        .column((ClaimedEventRow, NotificationEvents::EntityModifiedAt))
        .column((ClaimedEventRow, NotificationEvents::EntityExpiresAt))
        .column((ClaimedEventRow, NotificationEvents::CreatedAt))
        .from(table_of(target, NotificationEvents::Table).alias(ClaimedEventRow))
        .and_where(Expr::col((ClaimedEventRow, NotificationEvents::EventKey)).eq(any(bigint_array(event_keys))))
        .to_owned()
}

/// The alias of an event a claimed delivery answers, `e`.
#[derive(Iden, Clone, Copy)]
#[iden = "e"]
struct ClaimedEventRow;

/// The alias of one changed attribute's dictionary row, `n`.
#[derive(Iden, Clone, Copy)]
#[iden = "n"]
struct ChangedName;

/// A bound `bigint[]` value from a batch of claimed identifiers.
fn bigint_array(ids: &[i64]) -> SimpleExpr {
    Expr::val(ids.to_vec())
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::claimed_event_read::{claimed_events_statement, distinct_event_keys, ordinality_alias, split_held};
    use andromeda_context_commit::{write_id::WriteId, write_ordinal::WriteOrdinal};
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use andromeda_ngsi_ld_model::entity_change::{EntityChange, EntityLifecycle};
    use andromeda_notification::{
        held_events::HeldEvents,
        notification_event::{NotificationEvent, NotificationEventKey},
    };
    use andromeda_postgresql_seaquery::unnest::apply_with_ordinality;
    use chrono::Utc;
    use sea_query::PostgresQueryBuilder;
    use std::sync::Arc;
    use uuid::Uuid;

    fn events_sql(target: &ContextTarget) -> String {
        let sql = claimed_events_statement(target, &[7, 3]).to_string(PostgresQueryBuilder);
        let (alias, columns, ordinality) = ordinality_alias();
        apply_with_ordinality(sql, &alias, &columns, &ordinality)
    }

    fn snapshot() -> ContextTarget {
        let key = SnapshotKey::try_from(5).expect("positive key");
        let id = "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri");
        ContextTarget::Snapshot(SnapshotTarget::published(key, id))
    }

    fn event(key: i64) -> Arc<NotificationEvent> {
        Arc::new(
            NotificationEvent::builder()
                .key(NotificationEventKey::from(key))
                .write_id(WriteId::from_uuid(Uuid::nil()))
                .event_ordinal(WriteOrdinal::at(1))
                .change(EntityChange::new(EntityLifecycle::Created, Vec::new()))
                .entity_id("urn:ngsi-ld:Vehicle:1".parse().expect("a valid entity URI"))
                .created_at(Utc::now())
                .build(),
        )
    }

    #[test]
    fn the_changed_attributes_keep_the_order_the_event_recorded_them_in() {
        let sql = events_sql(&ContextTarget::Live);
        assert!(sql.contains(r#"array_agg("n"."name" ORDER BY "k"."position")"#), "{sql}");
        assert!(sql.contains("WITH ORDINALITY AS k(name_key, position)"), "{sql}");
        assert!(sql.contains("COALESCE"), "{sql}");
    }

    #[test]
    fn the_event_projection_reads_each_claimed_event_once() {
        let sql = events_sql(&ContextTarget::Live);
        assert!(sql.contains(r#"FROM "notification_events" AS "e""#), "{sql}");
        assert!(sql.contains(r#""e"."event_key" = ANY("#), "{sql}");
        assert!(!sql.contains("notification_deliveries"), "{sql}");
        for column in [
            "event_key",
            "write_id",
            "event_ordinal",
            "lifecycle",
            "entity_id",
            "change_kinds",
            "change_dataset_ids",
            "entity_snapshot",
            "previous_snapshot",
            "entity_created_at",
            "entity_modified_at",
            "entity_expires_at",
            "created_at",
        ] {
            assert!(sql.contains(&format!(r#""e"."{column}""#)), "{column}: {sql}");
        }
        assert!(sql.contains(r#"AS "change_attributes""#), "{sql}");
    }

    #[test]
    fn a_snapshot_branch_event_projection_qualifies_its_relation() {
        let sql = events_sql(&snapshot());
        assert!(sql.contains(r#"FROM "snap_5"."notification_events" AS "e""#), "{sql}");
    }

    #[test]
    fn the_event_projection_does_not_recover_the_branch_from_a_control_table_join() {
        let sql = events_sql(&ContextTarget::Live);
        assert!(!sql.contains("context_spaces"), "the branch is the caller's, not a joined control row: {sql}");
        assert!(!sql.contains(r#""snap""#), "{sql}");
    }

    #[test]
    fn the_event_keys_of_a_claim_are_distinct_in_first_seen_order() {
        pretty_assertions::assert_eq!(distinct_event_keys([7, 3, 7, 9, 3]), [7, 3, 9]);
    }

    #[test]
    fn a_claim_reads_only_the_events_its_pass_does_not_hold() {
        let held = HeldEvents::default();
        let in_flight = event(3);
        held.hold(&ContextTarget::Live, &in_flight);
        let (reused, unread) = split_held(vec![7, 3, 9], &held, &ContextTarget::Live);
        pretty_assertions::assert_eq!(unread, [7, 9]);
        pretty_assertions::assert_eq!(reused.len(), 1);
        assert!(Arc::ptr_eq(&reused[0], &in_flight));
    }

    #[test]
    fn a_claim_on_another_branch_reads_an_event_the_live_branch_holds_under_the_same_key() {
        let held = HeldEvents::default();
        let live = event(3);
        held.hold(&ContextTarget::Live, &live);
        let (reused, unread) = split_held(vec![3], &held, &snapshot());
        assert!(reused.is_empty());
        pretty_assertions::assert_eq!(unread, [3]);
    }
}
