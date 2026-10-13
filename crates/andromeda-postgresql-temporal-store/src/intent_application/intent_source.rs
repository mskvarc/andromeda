//! The intents an application reads, and their entries.
//!
//! Every statement of an application reads one source relation under `i`,
//! one row per intent with the columns of `temporal_append_intents`: the
//! claimed rows of the queue, or a commit's intents bound as one
//! `temporal_intent[]` beside the commit's write identity. Each entry is
//! unnested beside its 1-based position under `e`. A statement adds the
//! catalogue relations it needs as further `FROM` items joined in its
//! `WHERE`, because the position comes from an `UNNEST` of the intent row,
//! and a `JOIN ... ON` after it could not name the intent. The two sources
//! differ in that one relation alone, so the drain and the commit record
//! history through the same statements.

use crate::temporal_entry_record::{TemporalEntryRecord, TemporalIntentRecord};
use andromeda_context_commit::write_record::WriteRecord;
use andromeda_postgresql_codes::temporal_entry_kind::TemporalEntryKind;
use andromeda_postgresql_schema::{queue::TemporalAppendIntents, temporal_intent::TemporalEntry};
use andromeda_postgresql_seaquery::{
    cast::cast,
    pg_array::any,
    pg_type::PostgresType,
    placeholder::external_placeholder,
    unnest::{Ordinality, apply_record_ordinality, unnest_of},
};
use sea_query::{Asterisk, Expr, ExprTrait, Iden, PostgresQueryBuilder, Query, QueryStatementWriter, SelectStatement, SimpleExpr};
use sqlx::{Postgres, postgres::PgArguments, query::Query as SqlxQuery};

/// The source relation's alias, `i`.
#[derive(Iden, Clone, Copy)]
#[iden = "i"]
pub(crate) struct I;

/// The alias of one unnested entry, `e`.
#[derive(Iden, Clone, Copy)]
#[iden = "e"]
pub(crate) struct E;

/// The `temporal_entities` alias, `te`.
#[derive(Iden, Clone, Copy)]
#[iden = "te"]
pub(crate) struct Te;

/// The `temporal_attribute_streams` alias, `s`.
#[derive(Iden, Clone, Copy)]
#[iden = "s"]
pub(crate) struct S;

/// The alias of one bound intent inside the bound source, `input`.
#[derive(Iden, Clone, Copy)]
#[iden = "input"]
struct Input;

/// The intents one application records, as its statements read them.
#[derive(Debug)]
pub enum IntentSource<'a> {
    /// The claimed rows of `temporal_append_intents` named by their keys.
    Queued(&'a [i64]),
    /// One commit's intents, recorded under the commit's write identity.
    Bound {
        /// The write identity and instant every intent shares.
        record: &'a WriteRecord,
        /// The intents, in commit order, which is the order their keys take.
        intents: &'a [TemporalIntentRecord],
    },
}

/// What a statement needs among the entries for it to change anything.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EntryNeed {
    /// Nothing: it runs for every source.
    Any,
    /// An intent stating types.
    TypedIntent,
    /// An intent stating no type that holds no entry or one that ends nothing.
    UntypedObservation,
    /// An observation or the end of an instance.
    AttributeEntry,
    /// An attribute entry carrying a client-supplied `instanceId`.
    VerbatimInstance,
    /// A Scope entry.
    ScopeEntry,
}

impl IntentSource<'_> {
    /// The source relation under `i`: the claimed rows `$1` names, or the
    /// intents `$1` binds beside the write identity `$2` and its instant `$3`,
    /// each keyed by its position.
    fn relation(&self) -> SelectStatement {
        let mut relation = Query::select();
        match self {
            IntentSource::Queued(_) => relation
                .column(Asterisk)
                .from(TemporalAppendIntents::Table)
                .and_where(Expr::col(TemporalAppendIntents::IntentKey).eq(any(cast(external_placeholder(1), PostgresType::BigintArray)))),
            IntentSource::Bound { .. } => relation
                .expr_as(cast(external_placeholder(2), PostgresType::Uuid), TemporalAppendIntents::WriteId)
                .expr_as(cast(external_placeholder(3), PostgresType::Timestamptz), TemporalAppendIntents::RecordedAt)
                .exprs(
                    [
                        TemporalAppendIntents::EntityId,
                        TemporalAppendIntents::EntityTypeNameKeys,
                        TemporalAppendIntents::BaseOrdinal,
                        TemporalAppendIntents::Entries,
                    ]
                    .map(|column| Expr::col((Input, column))),
                )
                .expr_as(Expr::col((Input, Ordinality)), TemporalAppendIntents::IntentKey)
                .from_function(unnest_of(cast(external_placeholder(1), PostgresType::TemporalIntentArray)), Input),
        };
        relation
    }

    /// Whether some entry of the source may meet `need`: a bound source says
    /// so from its entries, a queued one is never read back and may.
    pub(crate) fn may_meet(&self, need: EntryNeed) -> bool {
        let IntentSource::Bound { intents, .. } = self else {
            return true;
        };
        let entries = || intents.iter().flat_map(|intent| &intent.entries);
        let attribute = |entry: &TemporalEntryRecord| entry.entry_kind != TemporalEntryKind::Scope.code();
        match need {
            EntryNeed::Any => true,
            EntryNeed::TypedIntent => intents.iter().any(|intent| !intent.entity_type_name_keys.is_empty()),
            EntryNeed::UntypedObservation => intents.iter().any(|intent| {
                intent.entity_type_name_keys.is_empty()
                    && (intent.entries.is_empty() || intent.entries.iter().any(|entry| entry.entry_kind != TemporalEntryKind::Tombstone.code()))
            }),
            EntryNeed::AttributeEntry => entries().any(attribute),
            EntryNeed::VerbatimInstance => entries().any(|entry| attribute(entry) && entry.verbatim_instance_id.is_some()),
            EntryNeed::ScopeEntry => entries().any(|entry| !attribute(entry)),
        }
    }

    /// `query` with the source's values bound in marker order.
    pub(crate) fn bind<'q>(&'q self, query: SqlxQuery<'q, Postgres, PgArguments>) -> SqlxQuery<'q, Postgres, PgArguments> {
        match self {
            IntentSource::Queued(keys) => query.bind(*keys),
            IntentSource::Bound { record, intents } => query.bind(*intents).bind(record.write_id().as_uuid()).bind(record.recorded_at()),
        }
    }
}

/// Read the intents of `source` in `select`, under `i`.
pub(crate) fn source_intents(select: &mut SelectStatement, source: &IntentSource<'_>) {
    select.from_subquery(source.relation(), I);
}

/// Read every entry of the intents of `source` in `select`: the intents
/// under `i`, each entry under `e`.
pub(crate) fn source_entries(select: &mut SelectStatement, source: &IntentSource<'_>) {
    source_intents(select, source);
    select.from_function(unnest_of(Expr::col((I, TemporalAppendIntents::Entries))), E);
}

/// One field of the entry under `e`.
pub(crate) fn entry(field: TemporalEntry) -> SimpleExpr {
    Expr::col((E, field))
}

/// One column of the intent under `i`.
pub(crate) fn intent(column: TemporalAppendIntents) -> SimpleExpr {
    Expr::col((I, column))
}

/// The write ordinal of the entry under `e`: its intent's base ordinal plus
/// its position, counted from 0.
pub(crate) fn write_ordinal() -> SimpleExpr {
    intent(TemporalAppendIntents::BaseOrdinal).add(Expr::col((E, Ordinality)).sub(1))
}

/// Whether the entry under `e` is one of `kinds`.
pub(crate) fn entry_kind_in(kinds: &[i16]) -> SimpleExpr {
    entry(TemporalEntry::EntryKind).is_in(kinds.iter().copied())
}

/// The rendered statement, its entries and bound intents unnested with their
/// positions. Every value but the source's is a constant of the statement, so
/// the text is the same for every application over one kind of source.
pub(crate) fn rendered(statement: &impl QueryStatementWriter) -> String {
    let sql = apply_record_ordinality(statement.to_string(PostgresQueryBuilder), &E);
    apply_record_ordinality(sql, &Input)
}
