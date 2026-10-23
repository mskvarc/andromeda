//! Which Scope entries of an application's source are recorded.
//!
//! A Scope event (a change of Scope, a reified instance, a removal) is
//! recorded as it stands. A Scope assertion is recorded only where the Scope in
//! force at its write differs from the paths it states (clause 4.18), judged
//! against the Scope events its Entity has recorded and the Scope entries of
//! the intents of the Entity before it in the source: the statement recording
//! them cannot see the rows it inserts itself.
//!
//! An application never runs while an earlier intent of one of its Entities is
//! still queued outside its source: the drain refuses such a set, and a live
//! commit records inline only the Entities with no queued history. Every
//! entry an assertion is judged against is therefore recorded or in the
//! source.

use crate::intent_application::intent_source::{I, StagedSource, entry, intent, source_intents_as};
use andromeda_postgresql_codes::temporal_entry_kind::TemporalEntryKind;
use andromeda_postgresql_query::scope_assertion::{EVENT_COLUMNS, held_in_force, path_set};
use andromeda_postgresql_schema::{current::Entities, queue::TemporalAppendIntents, temporal::ScopeHistory, temporal_intent::TemporalEntry};
use andromeda_postgresql_seaquery::{branch::qualified, unnest::unnest_of};
use andromeda_postgresql_temporal_query::alias::Te;
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

/// The two Scope entry codes.
const SCOPE_KINDS: [i16; 2] = [TemporalEntryKind::Scope.code(), TemporalEntryKind::ScopeAssertion.code()];

/// Whether the Scope entry under `e`, of the intent under `i` and the
/// Temporal Evolution under `te`, is recorded in the branch `schema` names:
/// it is an event, or the Scope in force at its write is not the one it
/// asserts.
pub(crate) fn recorded_scope_entry(source: &StagedSource<'_>, schema: Option<&DynIden>) -> Condition {
    let judged = held_in_force(&judging_events(source, schema), &asserted_paths(), &intent(TemporalAppendIntents::RecordedAt)).not();
    Condition::any()
        .add(entry(TemporalEntry::EntryKind).ne(TemporalEntryKind::ScopeAssertion.code()))
        .add(judged)
}

/// The Scope events the assertion under `e` is judged against: its Entity's
/// recorded in `schema`, and the Scope entries of the Entity's intents before
/// its own in the source.
fn judging_events(source: &StagedSource<'_>, schema: Option<&DynIden>) -> SelectStatement {
    let mut recorded = Query::select();
    recorded
        .exprs(EVENT_COLUMNS.map(|column| Expr::col((RecordedEvent, column))))
        .from(qualified(schema, ScopeHistory::Table).alias(RecordedEvent))
        .and_where(Expr::col((RecordedEvent, ScopeHistory::EntityKey)).equals((Te, Entities::EntityKey)));
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

#[cfg(test)]
mod tests {
    use crate::intent_application::{
        intent_source::{EntryReads, IntentSource, StagedSource},
        scope_judgement::recorded_scope_entry,
    };
    use andromeda_postgresql_codes::temporal_intent_reach::TemporalIntentReach;
    use sea_query::{Alias, Asterisk, DynIden, IntoIden, PostgresQueryBuilder, Query};

    fn rendered(source: &IntentSource<'_>, schema: Option<&DynIden>) -> String {
        let staged = StagedSource::new(source, EntryReads::Everything, &[]);
        Query::select()
            .column(Asterisk)
            .cond_where(recorded_scope_entry(&staged, schema))
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    fn queued() -> IntentSource<'static> {
        IntentSource::Queued {
            keys: &[3, 5],
            reach: TemporalIntentReach::ALL.to_vec(),
        }
    }

    /// Every entry an assertion is judged against is recorded or in the
    /// source, so no branch reads the queue to judge one.
    #[test]
    fn an_assertion_is_judged_without_reading_the_queue_in_either_branch() {
        let schema = Alias::new("snap_1").into_iden();
        let snapshot = rendered(&queued(), Some(&schema));
        assert!(!snapshot.contains(r#""temporal_append_intents" AS "sq""#), "{snapshot}");
        assert!(snapshot.contains(r#"FROM "snap_1"."scope_history" AS "sr""#), "{snapshot}");
        let live = rendered(&queued(), None);
        assert!(!live.contains(r#""temporal_append_intents" AS "sq""#), "{live}");
        assert!(live.contains(r#"FROM "scope_history" AS "sr""#), "{live}");
    }
}
