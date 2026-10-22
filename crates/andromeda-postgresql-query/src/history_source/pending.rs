//! The queued entries of the live branch the application would write, as rows
//! of the history sources.
//!
//! An entry is read from `temporal_append_entries` and judged by the rules the
//! application applies when it records the entry's intent:
//!
//! - An entry is written only for an Entity with a Temporal Evolution, which
//!   either is recorded or is created by a queued intent stating types. The
//!   end of an instance needs that intent to come no later than its own, as
//!   applying the intents one after the other leaves it; an observation of an
//!   Entity with neither stays queued until one exists, and is not visible
//!   until then.
//! - An attribute entry carrying a client `instanceId` replaces the recorded
//!   row of that instance on any dataset of the attribute (clause 4.5.7), and
//!   a later queued entry naming the same instance replaces it in turn.
//! - An attribute entry without one whose write identity is already recorded
//!   lands on nothing, the history insert ignoring the conflict.
//! - A Scope entry whose write identity is already recorded lands on
//!   nothing, as an attribute entry without a client `instanceId` does.
//! - A Scope assertion is written only where the Scope in force at its write
//!   differs, judged against the recorded Scope events and the earlier queued
//!   ones by the judgement the application applies
//!   ([`crate::scope_assertion`]).
//!
//! The visible types of an Entity with a queued intent stating types are
//! those its recorded Temporal Evolution holds, then every other type its
//! queued intents state: the application adds a type to the list and never
//! removes one (clauses 5.6.11.4 and 5.6.12.4).
//!
//! The queue lives beside the live branch's relations. The recorded relations
//! an entry is judged against are named in the schema the caller gives, which
//! is `None` unless the statement's unqualified names resolve elsewhere.

use crate::{
    history_source::{
        HistoryOf,
        HistoryStream,
        entry_values::{attribute_entry_values, scope_entry_values},
        grain::HistoryGrain,
        instance_replacement::{ReplacingEntry, naming_instance},
    },
    scope_assertion::{EVENT_COLUMNS, held_in_force, path_set},
    tombstone::entry_carries_a_value,
    type_list_union::{TypeListColumn, type_list_union},
};
use andromeda_postgresql_codes::temporal_entry_kind::TemporalEntryKind;
use andromeda_postgresql_schema::{
    queue::{TemporalAppendEntries, TemporalAppendIntents},
    temporal::{AttributeHistory, ScopeHistory, TemporalAttributeStreams, TemporalEntities},
    temporal_intent::TemporalEntry,
};
use andromeda_postgresql_seaquery::{
    branch::qualified,
    cast::typed_null,
    literal::int_literal,
    pg_array::has_elements,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use sea_query::{Condition, DynIden, Expr, ExprTrait, Iden, IntoIden, JoinType, Query, SelectStatement, SimpleExpr, TableRef, UnionType};

/// Alias `pe`: the queued entry a source row is projected from.
#[derive(Iden, Clone, Copy)]
#[iden = "pe"]
struct PendingEntry;

/// Alias `pt`: a queued intent stating types.
#[derive(Iden, Clone, Copy)]
#[iden = "pt"]
struct TypedIntent;

/// Alias `pv`: a recorded Temporal Evolution.
#[derive(Iden, Clone, Copy)]
#[iden = "pv"]
struct RecordedEvolution;

/// Alias `ps`: a recorded stream.
#[derive(Iden, Clone, Copy)]
#[iden = "ps"]
struct RecordedStream;

/// Alias `pd`: a recorded row under a queued entry's write identity.
#[derive(Iden, Clone, Copy)]
#[iden = "pd"]
struct RecordedRow;

/// Alias `pj`: a recorded Scope event a queued assertion is judged against.
#[derive(Iden, Clone, Copy)]
#[iden = "pj"]
struct JudgingEvent;

/// Alias `pjv`: the recorded Temporal Evolution of a judging event.
#[derive(Iden, Clone, Copy)]
#[iden = "pjv"]
struct JudgingEvolution;

/// Alias `pq`: an earlier queued Scope entry a queued assertion is judged
/// against.
#[derive(Iden, Clone, Copy)]
#[iden = "pq"]
struct EarlierEntry;

/// Alias `pa`: one path of the queued assertion being judged.
#[derive(Iden, Clone, Copy)]
#[iden = "pa"]
struct AssertedPath;

/// One column of the entry under `pe`.
fn entry(column: impl IntoIden) -> SimpleExpr {
    Expr::col((PendingEntry, column.into_iden()))
}

/// Whether the entry under `pe` is one of `kinds`.
pub(crate) fn entry_kind_in(alias: impl IntoIden, kinds: &[TemporalEntryKind]) -> SimpleExpr {
    Expr::col((alias.into_iden(), TemporalEntry::EntryKind)).is_in(kinds.iter().map(|kind| SimpleExpr::Constant(kind.code().into())))
}

/// The two attribute entry kinds.
pub(crate) const ATTRIBUTE_KINDS: [TemporalEntryKind; 2] = [TemporalEntryKind::Observation, TemporalEntryKind::Tombstone];

/// The two Scope entry kinds: an event, and an assertion of the Scope in force.
const SCOPE_KINDS: [TemporalEntryKind; 2] = [TemporalEntryKind::Scope, TemporalEntryKind::ScopeAssertion];

/// Whether the Entity `entity_id` names has a queued intent stating types.
pub(crate) fn types_queued_for(entity_id: SimpleExpr) -> SimpleExpr {
    let mut typed = Query::select();
    typed
        .expr(int_literal(1))
        .from_as(TemporalAppendIntents::Table, TypedIntent)
        .and_where(Expr::col((TypedIntent, TemporalAppendIntents::EntityId)).eq(entity_id))
        .and_where(has_elements(Expr::col((TypedIntent, TemporalAppendIntents::EntityTypeNameKeys))));
    Expr::exists(typed)
}

/// Whether the entry under `pe` is written when its intent is applied: its
/// Entity has a Temporal Evolution recorded in `schema` or a queued intent
/// stating types, no later than the entry's own for the end of an instance.
fn applied(schema: Option<&DynIden>) -> Condition {
    let mut recorded = Query::select();
    recorded
        .expr(int_literal(1))
        .from(qualified(schema, TemporalEntities::Table).alias(RecordedEvolution))
        .and_where(Expr::col((RecordedEvolution, TemporalEntities::Id)).equals((PendingEntry, TemporalAppendEntries::EntityId)));
    let mut typed = Query::select();
    typed
        .expr(int_literal(1))
        .from_as(TemporalAppendIntents::Table, TypedIntent)
        .and_where(Expr::col((TypedIntent, TemporalAppendIntents::EntityId)).equals((PendingEntry, TemporalAppendEntries::EntityId)))
        .and_where(has_elements(Expr::col((TypedIntent, TemporalAppendIntents::EntityTypeNameKeys))))
        .cond_where(
            Condition::any()
                .add(entry_carries_a_value(PendingEntry))
                .add(Expr::col((TypedIntent, TemporalAppendIntents::IntentKey)).lte(entry(TemporalAppendEntries::IntentKey))),
        );
    Condition::any().add(Expr::exists(recorded)).add(Expr::exists(typed))
}

/// Whether `history` holds a row under the write identity of the entry under
/// `pe`: the history insert ignores the entry then, so it lands on nothing.
fn recorded_under_entry_identity(history: TableRef, [recorded_at, write_id, write_ordinal]: [DynIden; 3]) -> SimpleExpr {
    let mut recorded = Query::select();
    recorded
        .expr(int_literal(1))
        .from(history)
        .and_where(Expr::col((RecordedRow, recorded_at)).equals((PendingEntry, TemporalAppendEntries::RecordedAt)))
        .and_where(Expr::col((RecordedRow, write_id)).equals((PendingEntry, TemporalAppendEntries::WriteId)))
        .and_where(Expr::col((RecordedRow, write_ordinal)).equals((PendingEntry, TemporalAppendEntries::WriteOrdinal)));
    Expr::exists(recorded)
}

/// Whether the attribute entry under `pe` is the row its instance ends up
/// as: a client `instanceId` no later queued entry names again, or a write
/// identity no row recorded in `schema` holds.
fn lands(schema: Option<&DynIden>) -> Condition {
    let mut replaced = naming_instance(
        entry(TemporalAppendEntries::EntityId),
        entry(TemporalEntry::AttributeNameKey),
        entry(TemporalEntry::VerbatimInstanceId),
    );
    replaced.and_where(Expr::col((ReplacingEntry, TemporalAppendEntries::IntentKey)).gt(entry(TemporalAppendEntries::IntentKey)));
    let recorded = recorded_under_entry_identity(
        qualified(schema, AttributeHistory::Table).alias(RecordedRow),
        [
            AttributeHistory::RecordedAt.into_iden(),
            AttributeHistory::WriteId.into_iden(),
            AttributeHistory::WriteOrdinal.into_iden(),
        ],
    );
    Condition::any()
        .add(
            Condition::all()
                .add(entry(TemporalEntry::VerbatimInstanceId).is_not_null())
                .add(Expr::exists(replaced).not()),
        )
        .add(Condition::all().add(entry(TemporalEntry::VerbatimInstanceId).is_null()).add(recorded.not()))
}

/// The queued attribute entries the application would write, as rows of the
/// attribute source: the grain, the write identity, the value columns.
pub(crate) fn attribute_rows(schema: Option<&DynIden>, of: &HistoryOf) -> SelectStatement {
    let mut select = Query::select();
    select
        .expr_as(entry(TemporalAppendEntries::EntityId), HistoryGrain::EntityId)
        .expr_as(entry(TemporalEntry::AttributeNameKey), HistoryGrain::AttributeNameKey)
        .expr_as(entry(TemporalEntry::DatasetId), HistoryGrain::DatasetId)
        .expr_as(entry(TemporalAppendEntries::RecordedAt), AttributeHistory::RecordedAt)
        .expr_as(entry(TemporalAppendEntries::WriteId), AttributeHistory::WriteId)
        .expr_as(entry(TemporalAppendEntries::WriteOrdinal), AttributeHistory::WriteOrdinal);
    for (column, value) in attribute_entry_values(entry) {
        select.expr_as(value, column);
    }
    select
        .from_as(TemporalAppendEntries::Table, PendingEntry)
        .and_where(entry_kind_in(PendingEntry, &ATTRIBUTE_KINDS))
        .cond_where(applied(schema))
        .cond_where(lands(schema));
    of.narrow(&mut select, entry(TemporalAppendEntries::EntityId));
    select
}

/// The queued attribute entries of the one grain `stream` names that the
/// application would write, as rows of the attribute source.
pub(crate) fn stream_attribute_rows(schema: Option<&DynIden>, stream: &HistoryStream) -> SelectStatement {
    let mut select = attribute_rows(schema, &HistoryOf::Entity(stream.column(HistoryGrain::EntityId)));
    select
        .and_where(entry(TemporalEntry::AttributeNameKey).eq(stream.column(HistoryGrain::AttributeNameKey)))
        .and_where(entry(TemporalEntry::DatasetId).not_distinct_from(stream.column(HistoryGrain::DatasetId)));
    select
}

/// The grains queued attribute entries extend that no stream recorded in
/// `schema` holds, as rows of the stream source, their stream key NULL.
pub(crate) fn stream_rows(schema: Option<&DynIden>, of: &HistoryOf) -> SelectStatement {
    let mut recorded = Query::select();
    recorded
        .expr(int_literal(1))
        .from(qualified(schema, TemporalAttributeStreams::Table).alias(RecordedStream))
        .join_as(
            JoinType::Join,
            qualified(schema, TemporalEntities::Table),
            RecordedEvolution,
            Expr::col((RecordedEvolution, TemporalEntities::TemporalEntityKey)).equals((RecordedStream, TemporalAttributeStreams::TemporalEntityKey)),
        )
        .and_where(Expr::col((RecordedEvolution, TemporalEntities::Id)).equals((PendingEntry, TemporalAppendEntries::EntityId)))
        .and_where(Expr::col((RecordedStream, TemporalAttributeStreams::AttributeNameKey)).equals((PendingEntry, TemporalEntry::AttributeNameKey)))
        .and_where(SimpleExpr::from(Expr::col((RecordedStream, TemporalAttributeStreams::DatasetId))).not_distinct_from(entry(TemporalEntry::DatasetId)));
    let mut select = Query::select();
    select
        .distinct()
        .expr_as(entry(TemporalAppendEntries::EntityId), HistoryGrain::EntityId)
        .expr_as(entry(TemporalEntry::AttributeNameKey), HistoryGrain::AttributeNameKey)
        .expr_as(entry(TemporalEntry::DatasetId), HistoryGrain::DatasetId)
        .expr_as(typed_null(PostgresType::Bigint), HistoryGrain::StreamKey)
        .from_as(TemporalAppendEntries::Table, PendingEntry)
        .and_where(entry_kind_in(PendingEntry, &ATTRIBUTE_KINDS))
        .cond_where(applied(schema))
        .and_where(Expr::exists(recorded).not());
    of.narrow(&mut select, entry(TemporalAppendEntries::EntityId));
    select
}

/// The queued Scope entries the application would write, as rows of the
/// scope source: the Entity, the write identity, the value columns.
pub(crate) fn scope_rows(schema: Option<&DynIden>, of: &HistoryOf) -> SelectStatement {
    let recorded = recorded_under_entry_identity(
        qualified(schema, ScopeHistory::Table).alias(RecordedRow),
        [
            ScopeHistory::RecordedAt.into_iden(),
            ScopeHistory::WriteId.into_iden(),
            ScopeHistory::WriteOrdinal.into_iden(),
        ],
    );
    let mut select = Query::select();
    select
        .expr_as(entry(TemporalAppendEntries::EntityId), HistoryGrain::EntityId)
        .expr_as(entry(TemporalAppendEntries::RecordedAt), ScopeHistory::RecordedAt)
        .expr_as(entry(TemporalAppendEntries::WriteId), ScopeHistory::WriteId)
        .expr_as(entry(TemporalAppendEntries::WriteOrdinal), ScopeHistory::WriteOrdinal);
    for (column, value) in scope_entry_values(entry) {
        select.expr_as(value, column);
    }
    select
        .from_as(TemporalAppendEntries::Table, PendingEntry)
        .and_where(entry_kind_in(PendingEntry, &SCOPE_KINDS))
        .cond_where(applied(schema))
        .and_where(recorded.not())
        .cond_where(
            Condition::any()
                .add(entry(TemporalEntry::EntryKind).ne(SimpleExpr::Constant(TemporalEntryKind::ScopeAssertion.code().into())))
                .add(held_in_force(&judging_events(schema), &queued_assertion(), &entry(TemporalAppendEntries::RecordedAt)).not()),
        );
    of.narrow(&mut select, entry(TemporalAppendEntries::EntityId));
    select
}

/// The Scope events a queued assertion under `pe` is judged against: the
/// Scope events of its Entity recorded in `schema`, and its Entity's Scope
/// entries queued before it, which the drain applies first.
fn judging_events(schema: Option<&DynIden>) -> SelectStatement {
    let mut recorded = Query::select();
    recorded
        .exprs(EVENT_COLUMNS.map(|column| Expr::col((JudgingEvent, column))))
        .from(qualified(schema, ScopeHistory::Table).alias(JudgingEvent))
        .join_as(
            JoinType::Join,
            qualified(schema, TemporalEntities::Table),
            JudgingEvolution,
            Expr::col((JudgingEvolution, TemporalEntities::TemporalEntityKey)).equals((JudgingEvent, ScopeHistory::TemporalEntityKey)),
        )
        .and_where(Expr::col((JudgingEvolution, TemporalEntities::Id)).equals((PendingEntry, TemporalAppendEntries::EntityId)));
    let mut earlier = Query::select();
    earlier
        .expr_as(Expr::col((EarlierEntry, TemporalAppendEntries::RecordedAt)), ScopeHistory::RecordedAt)
        .expr_as(Expr::col((EarlierEntry, TemporalEntry::ObservedAt)), ScopeHistory::ObservedAt)
        .expr_as(Expr::col((EarlierEntry, TemporalEntry::VerbatimScopePath)), ScopeHistory::VerbatimScopePath)
        .from_as(TemporalAppendEntries::Table, EarlierEntry)
        .and_where(Expr::col((EarlierEntry, TemporalAppendEntries::EntityId)).equals((PendingEntry, TemporalAppendEntries::EntityId)))
        .and_where(Expr::col((EarlierEntry, TemporalAppendEntries::IntentKey)).lt(entry(TemporalAppendEntries::IntentKey)))
        .and_where(entry_kind_in(EarlierEntry, &SCOPE_KINDS));
    recorded.union(UnionType::All, earlier);
    recorded
}

/// The paths the queued assertion under `pe` states: the assertion entries of
/// its intent.
fn queued_assertion() -> SimpleExpr {
    let mut paths = Query::select();
    paths
        .from_as(TemporalAppendEntries::Table, AssertedPath)
        .and_where(Expr::col((AssertedPath, TemporalAppendEntries::IntentKey)).equals((PendingEntry, TemporalAppendEntries::IntentKey)))
        .and_where(entry_kind_in(AssertedPath, &[TemporalEntryKind::ScopeAssertion]));
    path_set(paths, Expr::col((AssertedPath, TemporalEntry::VerbatimScopePath)))
}

/// The Entities a queued intent states types for, each with the types the
/// application leaves its Temporal Evolution, as rows of the entity source:
/// those its recorded Evolution in `schema` holds, then every other type its
/// queued intents state, in the order of the first intent stating it.
pub(crate) fn typed_entity_rows(schema: Option<&DynIden>) -> SelectStatement {
    let mut recorded = Query::select();
    recorded
        .expr_as(Expr::col((RecordedEvolution, TemporalEntities::Id)), TypeListColumn::Id)
        .expr_as(
            Expr::col((RecordedEvolution, TemporalEntities::EntityTypeNameKeys)),
            TypeListColumn::EntityTypeNameKeys,
        )
        .expr_as(int_literal(0), TypeListColumn::ListRank)
        .from(qualified(schema, TemporalEntities::Table).alias(RecordedEvolution))
        .and_where(types_queued_for(Expr::col((RecordedEvolution, TemporalEntities::Id))));
    let mut queued = Query::select();
    queued
        .expr_as(Expr::col((TypedIntent, TemporalAppendIntents::EntityId)), TypeListColumn::Id)
        .expr_as(
            Expr::col((TypedIntent, TemporalAppendIntents::EntityTypeNameKeys)),
            TypeListColumn::EntityTypeNameKeys,
        )
        .expr_as(Expr::col((TypedIntent, TemporalAppendIntents::IntentKey)), TypeListColumn::ListRank)
        .from_as(TemporalAppendIntents::Table, TypedIntent)
        .and_where(has_elements(Expr::col((TypedIntent, TemporalAppendIntents::EntityTypeNameKeys))));
    recorded.union(UnionType::All, queued);
    type_list_union(recorded)
}

#[cfg(test)]
mod tests {
    use crate::history_source::{HistoryOf, pending::attribute_rows};
    use sea_query::PostgresQueryBuilder;

    /// The view the entries are read from.
    const MIGRATION: &str = include_str!("../../../../migrations/tenant/0026_temporal_append_intents.sql");

    fn rendered() -> String {
        attribute_rows(None, &HistoryOf::EveryEntity).to_string(PostgresQueryBuilder)
    }

    /// Clause 5.6.16 deletes a Temporal Evolution and records no later end of
    /// an instance of it: the application drops a tombstone of an Entity with
    /// no evolution, so a queued one is visible only beside a recorded
    /// evolution or a typed intent no later than its own.
    #[test]
    fn a_pending_tombstone_of_an_entity_without_evolution_is_not_visible() {
        let sql = rendered();
        assert!(
            sql.contains(r#"EXISTS(SELECT 1 FROM "temporal_entities" AS "pv" WHERE "pv"."id" = "pe"."entity_id") OR EXISTS(SELECT 1 FROM "temporal_append_intents" AS "pt""#),
            "{sql}"
        );
        assert!(sql.contains(r#"("pe"."entry_kind" <> (2) OR "pt"."intent_key" <= "pe"."intent_key")"#), "{sql}");
    }

    /// Clause 4.5.1 makes a type part of every Entity: an observation of an
    /// Entity with neither an evolution nor a queued intent stating types is
    /// refused by the application and stays queued, so it is not visible.
    #[test]
    fn an_untyped_observation_of_an_entity_without_evolution_is_not_visible() {
        let sql = rendered();
        assert!(
            sql.contains(r#"WHERE "pe"."entry_kind" IN (1, 2) AND (EXISTS(SELECT 1 FROM "temporal_entities" AS "pv""#),
            "{sql}"
        );
        assert!(sql.contains(r#"cardinality("pt"."entity_type_name_keys") > 0"#), "{sql}");
    }

    /// A queued entry carries the identity its history row will: the write's
    /// instant and identity, and its intent's base ordinal plus its position
    /// counted from 0, so ties order as they will once it is recorded.
    #[test]
    fn pending_entries_carry_their_write_identity_for_ordering() {
        let sql = rendered();
        for fragment in [
            r#""pe"."recorded_at" AS "recorded_at""#,
            r#""pe"."write_id" AS "write_id""#,
            r#""pe"."write_ordinal" AS "write_ordinal""#,
        ] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
        assert!(
            MIGRATION.contains("(i.base_ordinal + e.ordinality - 1)::integer as write_ordinal"),
            "{MIGRATION}"
        );
        assert!(
            MIGRATION.contains("cross join lateral unnest(i.entries)\n    with ordinality as e"),
            "{MIGRATION}"
        );
    }

    /// Clause 4.5.7: a queued entry naming a client `instanceId` replaces the
    /// recorded row of that instance and is replaced by a later one, and an
    /// entry without one whose write identity is recorded lands on nothing.
    #[test]
    fn a_queued_entry_is_visible_only_as_the_row_its_instance_ends_up_as() {
        let sql = rendered();
        assert!(sql.contains(r#""pr"."intent_key" > "pe"."intent_key""#), "{sql}");
        assert!(
            sql.contains(r#"FROM "attribute_history" AS "pd" WHERE "pd"."recorded_at" = "pe"."recorded_at""#),
            "{sql}"
        );
    }
}
