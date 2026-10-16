//! Which Scope entries of an application's source are recorded.
//!
//! A Scope event (a change of Scope, a reified instance, a removal) is
//! recorded as it stands. A Scope assertion is recorded only where the Scope in
//! force at its write differs from the paths it states (clause 4.18), judged
//! against the Scope events its Entity has recorded and the Scope entries of
//! the intents of the Entity before it in the source: the statement recording
//! them cannot see the rows it inserts itself.
//!
//! Intents are claimed in key order across workers, and a retry reschedules
//! one, so an earlier intent of an Entity can still be queued, or applied by a
//! transaction not yet committed, when a later one is applied. Its entries
//! belong to the history an assertion is judged against, yet are neither
//! recorded nor in the source, so an assertion of an Entity with such an
//! intent is recorded unjudged. The queue removal runs after the application
//! in the same transaction, so an applied intent not yet committed is still
//! seen as queued. A source adds at most one redundant event per Entity that
//! way, and a redundant event changes no clause 4.19 reading of the history.

use crate::intent_application::intent_source::{I, IntentSource, Te, entry, intent, source_intents_as};
use andromeda_postgresql_codes::temporal_entry_kind::TemporalEntryKind;
use andromeda_postgresql_query::scope_assertion::{EVENT_COLUMNS, held_in_force, path_set};
use andromeda_postgresql_schema::{
    queue::TemporalAppendIntents,
    temporal::{ScopeHistory, TemporalEntities},
    temporal_intent::TemporalEntry,
};
use andromeda_postgresql_seaquery::{
    branch::qualified,
    cast::cast,
    literal::int_literal,
    pg_array::all,
    pg_type::PostgresType,
    placeholder::external_placeholder,
    unnest::unnest_of,
};
use sea_query::{Condition, DynIden, Expr, ExprTrait, Iden, Query, SelectStatement, SimpleExpr, UnionType};

/// The alias of a recorded Scope event an assertion is judged against, `sr`.
#[derive(Iden, Clone, Copy)]
#[iden = "sr"]
struct RecordedEvent;

/// The alias of an earlier intent of the source, `sj`.
#[derive(Iden, Clone, Copy)]
#[iden = "sj"]
struct EarlierIntent;

/// The alias of one entry of an earlier intent, `se`.
#[derive(Iden, Clone, Copy)]
#[iden = "se"]
struct EarlierEntry;

/// The alias of one path the judged assertion states, `sa`.
#[derive(Iden, Clone, Copy)]
#[iden = "sa"]
struct AssertedPath;

/// The alias of a queued intent outside the source, `sq`.
#[derive(Iden, Clone, Copy)]
#[iden = "sq"]
struct QueuedIntent;

/// The two Scope entry codes.
const SCOPE_KINDS: [i16; 2] = [TemporalEntryKind::Scope.code(), TemporalEntryKind::ScopeAssertion.code()];

/// Whether the Scope entry under `e`, of the intent under `i` and the
/// Temporal Evolution under `te`, is recorded in the branch `schema` names:
/// it is an event, an earlier intent of its Entity is queued outside the
/// source, or the Scope in force at its write is not the one it asserts.
pub(crate) fn recorded_scope_entry(source: &IntentSource<'_>, schema: Option<&DynIden>) -> Condition {
    let judged = held_in_force(&judging_events(source, schema), &asserted_paths(), &intent(TemporalAppendIntents::RecordedAt)).not();
    let recorded = Condition::any().add(entry(TemporalEntry::EntryKind).ne(TemporalEntryKind::ScopeAssertion.code()));
    match earlier_intent_queued(source, schema) {
        Some(queued) => recorded.add(queued).add(judged),
        None => recorded.add(judged),
    }
}

/// The Scope events the assertion under `e` is judged against: its Entity's
/// recorded in `schema`, and the Scope entries of the Entity's intents before
/// its own in the source.
fn judging_events(source: &IntentSource<'_>, schema: Option<&DynIden>) -> SelectStatement {
    let mut recorded = Query::select();
    recorded
        .exprs(EVENT_COLUMNS.map(|column| Expr::col((RecordedEvent, column))))
        .from(qualified(schema, ScopeHistory::Table).alias(RecordedEvent))
        .and_where(Expr::col((RecordedEvent, ScopeHistory::TemporalEntityKey)).equals((Te, TemporalEntities::TemporalEntityKey)));
    let mut earlier = Query::select();
    source_intents_as(&mut earlier, source, EarlierIntent);
    earlier
        .from_function(unnest_of(Expr::col((EarlierIntent, TemporalAppendIntents::Entries))), EarlierEntry)
        .expr_as(Expr::col((EarlierIntent, TemporalAppendIntents::RecordedAt)), ScopeHistory::RecordedAt)
        .expr_as(Expr::col((EarlierEntry, TemporalEntry::ObservedAt)), ScopeHistory::ObservedAt)
        .expr_as(Expr::col((EarlierEntry, TemporalEntry::VerbatimScopePath)), ScopeHistory::VerbatimScopePath)
        .and_where(Expr::col((EarlierIntent, TemporalAppendIntents::EntityId)).equals((I, TemporalAppendIntents::EntityId)))
        .and_where(Expr::col((EarlierIntent, TemporalAppendIntents::IntentKey)).lt(intent(TemporalAppendIntents::IntentKey)))
        .and_where(Expr::col((EarlierEntry, TemporalEntry::EntryKind)).is_in(SCOPE_KINDS));
    recorded.union(UnionType::All, earlier);
    recorded
}

/// The paths the assertion under `e` states: the assertion entries of its
/// intent.
fn asserted_paths() -> SimpleExpr {
    let mut paths = Query::select();
    paths
        .from_function(unnest_of(intent(TemporalAppendIntents::Entries)), AssertedPath)
        .and_where(Expr::col((AssertedPath, TemporalEntry::EntryKind)).eq(TemporalEntryKind::ScopeAssertion.code()));
    path_set(paths, Expr::col((AssertedPath, TemporalEntry::VerbatimScopePath)))
}

/// Whether an intent of the Entity under `i` that comes before it is queued
/// outside the source, `None` where the branch has no queue.
///
/// Only the live branch queues intents; a Snapshot branch records its history
/// in the write's own transaction. Every intent queued when a commit records
/// its own came before it, while the drain's own claimed intents are in the
/// source.
fn earlier_intent_queued(source: &IntentSource<'_>, schema: Option<&DynIden>) -> Option<SimpleExpr> {
    if schema.is_some() {
        return None;
    }
    let mut queued = Query::select();
    queued
        .expr(int_literal(1))
        .from_as(TemporalAppendIntents::Table, QueuedIntent)
        .and_where(Expr::col((QueuedIntent, TemporalAppendIntents::EntityId)).equals((I, TemporalAppendIntents::EntityId)));
    match source {
        IntentSource::Queued { .. } => {
            queued
                .and_where(Expr::col((QueuedIntent, TemporalAppendIntents::IntentKey)).lt(intent(TemporalAppendIntents::IntentKey)))
                .and_where(Expr::col((QueuedIntent, TemporalAppendIntents::IntentKey)).ne(all(cast(external_placeholder(1), PostgresType::BigintArray))));
        }
        IntentSource::Bound { .. } => {}
    }
    Some(Expr::exists(queued))
}

#[cfg(test)]
mod tests {
    use crate::intent_application::{intent_source::IntentSource, scope_judgement::recorded_scope_entry};
    use andromeda_postgresql_codes::temporal_intent_reach::TemporalIntentReach;
    use sea_query::{Alias, Asterisk, DynIden, IntoIden, PostgresQueryBuilder, Query};

    fn rendered(source: &IntentSource<'_>, schema: Option<&DynIden>) -> String {
        Query::select()
            .column(Asterisk)
            .cond_where(recorded_scope_entry(source, schema))
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    fn queued() -> IntentSource<'static> {
        IntentSource::Queued {
            keys: &[3, 5],
            reach: TemporalIntentReach::ALL.to_vec(),
        }
    }

    #[test]
    fn a_snapshot_branch_has_no_queue_to_guard_against() {
        let schema = Alias::new("snap_1").into_iden();
        let sql = rendered(&queued(), Some(&schema));
        assert!(!sql.contains(r#""temporal_append_intents" AS "sq""#), "{sql}");
        assert!(sql.contains(r#"FROM "snap_1"."scope_history" AS "sr""#), "{sql}");
    }
}
