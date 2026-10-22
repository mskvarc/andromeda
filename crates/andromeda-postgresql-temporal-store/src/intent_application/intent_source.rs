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
    external_arguments::ExternalArguments,
    pg_array::any,
    pg_type::PostgresType,
    statement::Statement,
    unnest::{Ordinality, unnest_of},
};
use sea_query::{Asterisk, Expr, ExprTrait, Iden, IntoIden, Query, SelectStatement, SimpleExpr};
use sqlx::PgConnection;

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
pub(crate) struct Input;

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

/// The needs the claimed rows `keys` names meet, one code a row, read from
/// their `entry_needs` and never their entries.
pub(crate) fn reach_statement(keys: &[i64]) -> SelectStatement {
    let mut statement = Query::select();
    statement
        .distinct()
        .expr(unnest_of(Expr::col(TemporalAppendIntents::EntryNeeds)))
        .from(TemporalAppendIntents::Table)
        .and_where(Expr::col(TemporalAppendIntents::IntentKey).eq(any(cast(Expr::val(keys.to_vec()), PostgresType::BigintArray))));
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
        let codes: Vec<i16> = reach_statement(keys).to_query_scalar().fetch_all(connection).await?;
        let reach = codes
            .into_iter()
            .map(TemporalIntentReach::try_from)
            .collect::<Result<Vec<_>, _>>()
            .map_err(StoredCatalogError::IntentReach)?;
        Ok(IntentSource::Queued { keys, reach })
    }

    /// Whether some intent of the source meets `need`.
    pub(crate) fn may_meet(&self, need: TemporalIntentReach) -> bool {
        match self {
            IntentSource::Queued { reach, .. } => reach.contains(&need),
            IntentSource::Bound { intents, .. } => intents.iter().any(|intent| intent.reaches(need)),
        }
    }
}

/// An [`IntentSource`] staged for the statements of one step: a bound
/// source's intents carry no more of their entries than the step reads, and
/// travel as the external argument the statement's source relation names.
pub(crate) struct StagedSource<'a> {
    /// The relation the statements read the intents from.
    relation: StagedRelation<'a>,
    /// The keys of the Temporal Evolutions the application created so far.
    created: &'a [i64],
    /// The external arguments the statements of the step carry.
    external: ExternalArguments<'a>,
}

/// Where the staged statements read their intents from.
enum StagedRelation<'a> {
    /// The claimed rows of the queue named by their keys.
    Queued {
        /// The claimed intents' keys.
        keys: &'a [i64],
    },
    /// The commit's intents, bound as one array beside its write identity.
    Bound {
        /// The write identity and instant every intent shares.
        record: &'a WriteRecord,
        /// The expression standing for the bound intents.
        intents: SimpleExpr,
    },
}

impl<'a> StagedSource<'a> {
    /// `source` staged for a step reading `reads` of the entries, with
    /// `created` the keys of the evolutions the application created so far.
    pub(crate) fn new(source: &'a IntentSource<'a>, reads: EntryReads, created: &'a [i64]) -> StagedSource<'a> {
        let mut external = ExternalArguments::default();
        let relation = match source {
            IntentSource::Queued { keys, .. } => StagedRelation::Queued { keys },
            IntentSource::Bound { record, intents } => StagedRelation::Bound {
                record,
                intents: match reads {
                    EntryReads::Nothing => external.push(intents.iter().map(TemporalIntentRecord::head).collect::<Vec<_>>()),
                    EntryReads::Identities => external.push(intents.iter().map(TemporalIntentRecord::identities).collect::<Vec<_>>()),
                    EntryReads::Everything => external.push(*intents),
                },
            },
        };
        StagedSource { relation, created, external }
    }

    /// The external arguments the staged statements carry, to bind with them.
    pub(crate) fn into_external(self) -> ExternalArguments<'a> {
        self.external
    }

    /// The source relation under `i`: the claimed rows named by their keys,
    /// or the bound intents beside the write identity and its instant, each
    /// keyed by its position.
    fn relation(&self) -> SelectStatement {
        let mut relation = Query::select();
        match &self.relation {
            StagedRelation::Queued { keys } => relation
                .column(Asterisk)
                .from(TemporalAppendIntents::Table)
                .and_where(Expr::col(TemporalAppendIntents::IntentKey).eq(any(cast(Expr::val(keys.to_vec()), PostgresType::BigintArray)))),
            StagedRelation::Bound { record, intents } => relation
                .expr_as(cast(Expr::val(record.write_id().as_uuid()), PostgresType::Uuid), TemporalAppendIntents::WriteId)
                .expr_as(
                    cast(Expr::val(record.recorded_at()), PostgresType::Timestamptz),
                    TemporalAppendIntents::RecordedAt,
                )
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
                .from_function(unnest_of(cast(intents.clone(), PostgresType::TemporalIntentArray)), Input), // clone: the relation owns the expression it places
        };
        relation
    }

    /// The keys of the Temporal Evolutions the application created, as a
    /// `bigint[]` value.
    pub(crate) fn created_evolutions(&self) -> SimpleExpr {
        cast(Expr::val(self.created.to_vec()), PostgresType::BigintArray)
    }
}

/// Read the intents of `source` in `select`, under `i`.
pub(crate) fn source_intents(select: &mut SelectStatement, source: &StagedSource<'_>) {
    source_intents_as(select, source, I);
}

/// Read the intents of `source` in `select` under `alias`, beside the ones
/// under `i`, for a statement that compares one intent with the others.
pub(crate) fn source_intents_as(select: &mut SelectStatement, source: &StagedSource<'_>, alias: impl IntoIden) {
    select.from_subquery(source.relation(), alias);
}

/// Read every entry of the intents of `source` in `select`: the intents
/// under `i`, each entry under `e`.
pub(crate) fn source_entries(select: &mut SelectStatement, source: &StagedSource<'_>) {
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
