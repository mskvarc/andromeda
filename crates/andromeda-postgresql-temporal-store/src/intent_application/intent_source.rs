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

use crate::{error::TemporalStoreError, stored_catalog_error::StoredCatalogError, temporal_entry_record::TemporalIntentRecord};
use andromeda_context_commit::write_record::WriteRecord;
use andromeda_postgresql_codes::temporal_intent_reach::TemporalIntentReach;
use andromeda_postgresql_schema::{queue::TemporalAppendIntents, temporal_intent::TemporalEntry};
use andromeda_postgresql_seaquery::{
    cast::cast,
    pg_array::any,
    pg_type::PostgresType,
    placeholder::external_placeholder,
    unnest::{Ordinality, apply_record_ordinality, unnest_of},
};
use sea_query::{Asterisk, Expr, ExprTrait, Iden, IntoIden, PostgresQueryBuilder, Query, QueryStatementWriter, SelectStatement, SimpleExpr};
use sqlx::{AssertSqlSafe, PgConnection, Postgres, postgres::PgArguments, query::Query as SqlxQuery};

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
    /// The claimed rows of `temporal_append_intents` named by their keys,
    /// beside the needs their entries meet.
    Queued {
        /// The claimed intents' keys.
        keys: &'a [i64],
        /// The needs some claimed intent meets, read from the rows by
        /// [`IntentSource::claimed`].
        reach: Vec<TemporalIntentReach>,
    },
    /// One commit's intents, recorded under the commit's write identity.
    Bound {
        /// The write identity and instant every intent shares.
        record: &'a WriteRecord,
        /// The intents, in commit order, which is the order their keys take.
        intents: &'a [TemporalIntentRecord],
    },
}

/// What of the entries a statement reads, so a bound source binds no more:
/// the server decodes every value it is sent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EntryReads {
    /// No entry: the intents' identity, types and positions.
    Nothing,
    /// What identifies each entry: its kind, grain and instance identifiers.
    Identities,
    /// Every field of every entry.
    Everything,
}

/// The needs the claimed rows `$1` names meet, one code a row, read from
/// their `entry_needs` and never their entries.
pub(crate) fn reach_statement() -> SelectStatement {
    let mut statement = Query::select();
    statement
        .distinct()
        .expr(unnest_of(Expr::col(TemporalAppendIntents::EntryNeeds)))
        .from(TemporalAppendIntents::Table)
        .and_where(Expr::col(TemporalAppendIntents::IntentKey).eq(any(cast(external_placeholder(1), PostgresType::BigintArray))));
    statement
}

impl<'a> IntentSource<'a> {
    /// The queued intents `keys` names, which the caller holds claimed, with
    /// the needs their entries meet.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the needs cannot be read,
    /// and [`TemporalStoreError::StoredCatalog`] when a stored code names no
    /// need.
    pub async fn claimed(connection: &mut PgConnection, keys: &'a [i64]) -> Result<IntentSource<'a>, TemporalStoreError> {
        let codes: Vec<i16> = sqlx::query_scalar(AssertSqlSafe(reach_statement().to_string(PostgresQueryBuilder)))
            .bind(keys)
            .fetch_all(connection)
            .await?;
        let reach = codes
            .into_iter()
            .map(TemporalIntentReach::try_from)
            .collect::<Result<Vec<_>, _>>()
            .map_err(StoredCatalogError::IntentReach)?;
        Ok(IntentSource::Queued { keys, reach })
    }

    /// The source relation under `i`: the claimed rows `$1` names, or the
    /// intents `$1` binds beside the write identity `$2` and its instant `$3`,
    /// each keyed by its position.
    fn relation(&self) -> SelectStatement {
        let mut relation = Query::select();
        match self {
            IntentSource::Queued { .. } => relation
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

    /// How many values [`IntentSource::bind`] binds: the claimed keys, or the
    /// intents, the write identity and its instant.
    const fn bound_values(&self) -> usize {
        match self {
            IntentSource::Queued { .. } => 1,
            IntentSource::Bound { .. } => 3,
        }
    }

    /// The keys of the Temporal Evolutions the application created, as the
    /// array marker after the source's own values.
    pub(crate) fn created_evolutions(&self) -> SimpleExpr {
        cast(external_placeholder(self.bound_values() + 1), PostgresType::BigintArray)
    }

    /// Whether some intent of the source meets `need`.
    pub(crate) fn may_meet(&self, need: TemporalIntentReach) -> bool {
        match self {
            IntentSource::Queued { reach, .. } => reach.contains(&need),
            IntentSource::Bound { intents, .. } => intents.iter().any(|intent| intent.reaches(need)),
        }
    }

    /// `query` with the source's values bound in marker order, a bound
    /// source's intents carrying no more of their entries than `reads`.
    pub(crate) fn bind<'q>(&'q self, query: SqlxQuery<'q, Postgres, PgArguments>, reads: EntryReads) -> SqlxQuery<'q, Postgres, PgArguments> {
        match self {
            IntentSource::Queued { keys, .. } => query.bind(*keys),
            IntentSource::Bound { record, intents } => match reads {
                EntryReads::Nothing => query.bind(intents.iter().map(TemporalIntentRecord::head).collect::<Vec<_>>()),
                EntryReads::Identities => query.bind(intents.iter().map(TemporalIntentRecord::identities).collect::<Vec<_>>()),
                EntryReads::Everything => query.bind(*intents),
            }
            .bind(record.write_id().as_uuid())
            .bind(record.recorded_at()),
        }
    }
}

/// Read the intents of `source` in `select`, under `i`.
pub(crate) fn source_intents(select: &mut SelectStatement, source: &IntentSource<'_>) {
    source_intents_as(select, source, I);
}

/// Read the intents of `source` in `select` under `alias`, beside the ones
/// under `i`, for a statement that compares one intent with the others.
pub(crate) fn source_intents_as(select: &mut SelectStatement, source: &IntentSource<'_>, alias: impl IntoIden) {
    select.from_subquery(source.relation(), alias);
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
    let sql = apply_record_ordinality(&statement.to_string(PostgresQueryBuilder), &E);
    apply_record_ordinality(&sql, &Input)
}
