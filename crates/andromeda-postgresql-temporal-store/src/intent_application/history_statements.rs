//! The history rows, Scope rows and instance claims an application records
//! from the claimed entries.
//!
//! Each statement joins the entries to their Entity's Temporal Evolution, so an
//! entry of an Entity without one records nothing; the catalogue statements
//! ran first and refused the only entries that would have needed one.

use crate::intent_application::queued_entries::{E, I, S, Te, claimed_entries, entry, entry_kind_in, intent, write_ordinal};
use andromeda_postgresql_codes::temporal_entry_kind::TemporalEntryKind;
use andromeda_postgresql_schema::{
    queue::TemporalAppendIntents,
    temporal::{AttributeHistory, AttributeHistoryInstanceClaims, ScopeHistory, TemporalAttributeStreams, TemporalEntities},
    temporal_intent::TemporalEntry,
};
use andromeda_postgresql_seaquery::{cast::cast, pg_operator::PgBinaryOperator, pg_type::PostgresType, postgis::st_make_valid};
use sea_query::{DeleteStatement, Expr, ExprTrait, Iden, InsertStatement, OnConflict, Query, SelectStatement, SimpleExpr, error::Error as SeaQueryError};

/// The sibling-stream alias the instance replacement searches, `s2`.
#[derive(Iden, Clone, Copy)]
#[iden = "s2"]
struct S2;

/// The two attribute entry kinds.
fn attribute_entries() -> SimpleExpr {
    entry_kind_in(&[TemporalEntryKind::Observation.code(), TemporalEntryKind::Tombstone.code()])
}

/// The claimed entries of `kinds` of Entities with a Temporal Evolution, the
/// evolution under `te`.
fn entries_with_evolution(keys: &[i64], kinds: SimpleExpr) -> SelectStatement {
    let mut select = Query::select();
    claimed_entries(&mut select, keys);
    select
        .from_as(TemporalEntities::Table, Te)
        .and_where(Expr::col((Te, TemporalEntities::Id)).equals((I, TemporalAppendIntents::EntityId)))
        .and_where(kinds);
    select
}

/// The claimed attribute entries beside the stream each extends, under `s`.
fn entries_with_stream(keys: &[i64]) -> SelectStatement {
    let mut select = entries_with_evolution(keys, attribute_entries());
    select
        .from_as(TemporalAttributeStreams::Table, S)
        .and_where(Expr::col((S, TemporalAttributeStreams::TemporalEntityKey)).equals((Te, TemporalEntities::TemporalEntityKey)))
        .and_where(Expr::col((S, TemporalAttributeStreams::AttributeNameKey)).equals((E, TemporalEntry::AttributeNameKey)))
        .and_where(SimpleExpr::from(Expr::col((S, TemporalAttributeStreams::DatasetId))).not_distinct_from(entry(TemporalEntry::DatasetId)));
    select
}

/// The attribute entries carrying a client-supplied `instanceId`.
fn verbatim_entries(keys: &[i64]) -> SelectStatement {
    let mut select = entries_with_evolution(keys, attribute_entries());
    select.and_where(entry(TemporalEntry::VerbatimInstanceId).is_not_null());
    select
}

/// Remove the history rows the claimed entries replace: a client that
/// supplied its own `instanceId` (clause 4.5.7) names one instance of the
/// attribute across every `datasetId`, so the row holding it on any sibling
/// stream goes first.
pub(crate) fn replaced_instances_statement(keys: &[i64]) -> DeleteStatement {
    let mut replaced = verbatim_entries(keys);
    replaced
        .expr(Expr::val(1))
        .from_as(TemporalAttributeStreams::Table, S2)
        .and_where(Expr::col((S2, TemporalAttributeStreams::TemporalEntityKey)).equals((Te, TemporalEntities::TemporalEntityKey)))
        .and_where(Expr::col((S2, TemporalAttributeStreams::AttributeNameKey)).equals((E, TemporalEntry::AttributeNameKey)))
        .and_where(Expr::col((AttributeHistory::Table, AttributeHistory::StreamKey)).equals((S2, TemporalAttributeStreams::StreamKey)))
        .and_where(Expr::col((AttributeHistory::Table, AttributeHistory::VerbatimInstanceId)).equals((E, TemporalEntry::VerbatimInstanceId)));
    Query::delete().from_table(AttributeHistory::Table).and_where(Expr::exists(replaced)).to_owned()
}

/// Release the instance claims the claimed entries are about to stake again,
/// so a replacement is not refused as a conflict with the instance it
/// replaces.
pub(crate) fn released_claims_statement(keys: &[i64]) -> DeleteStatement {
    let mut released = verbatim_entries(keys);
    released
        .expr(Expr::val(1))
        .and_where(
            Expr::col((AttributeHistoryInstanceClaims::Table, AttributeHistoryInstanceClaims::TemporalEntityKey))
                .equals((Te, TemporalEntities::TemporalEntityKey)),
        )
        .and_where(
            Expr::col((AttributeHistoryInstanceClaims::Table, AttributeHistoryInstanceClaims::AttributeNameKey)).equals((E, TemporalEntry::AttributeNameKey)),
        )
        .and_where(
            Expr::col((AttributeHistoryInstanceClaims::Table, AttributeHistoryInstanceClaims::VerbatimInstanceId))
                .equals((E, TemporalEntry::VerbatimInstanceId)),
        );
    Query::delete()
        .from_table(AttributeHistoryInstanceClaims::Table)
        .and_where(Expr::exists(released))
        .to_owned()
}

/// Stake a claim on every `(entity, attribute, instanceId)` the claimed
/// entries carry; the claims' primary key refuses two instances of one
/// attribute naming one `instanceId`, within the batch or against another
/// writer.
pub(crate) fn claims_statement(keys: &[i64]) -> Result<InsertStatement, SeaQueryError> {
    let mut source = verbatim_entries(keys);
    source
        .column((Te, TemporalEntities::TemporalEntityKey))
        .expr(entry(TemporalEntry::AttributeNameKey))
        .expr(entry(TemporalEntry::VerbatimInstanceId));
    let mut statement = Query::insert();
    statement
        .into_table(AttributeHistoryInstanceClaims::Table)
        .columns([
            AttributeHistoryInstanceClaims::TemporalEntityKey,
            AttributeHistoryInstanceClaims::AttributeNameKey,
            AttributeHistoryInstanceClaims::VerbatimInstanceId,
        ])
        .select_from(source)?;
    Ok(statement)
}

/// The `attribute_history` row of every claimed attribute entry, numbered from
/// its intent's base ordinal.
///
/// The geometry is made valid as a commit's is. The conflict target is the
/// write identity, so an entry recorded once is never recorded twice.
pub(crate) fn history_statement(keys: &[i64]) -> Result<InsertStatement, SeaQueryError> {
    let mut source = entries_with_stream(keys);
    source
        .expr(intent(TemporalAppendIntents::RecordedAt))
        .expr(intent(TemporalAppendIntents::WriteId))
        .column((S, TemporalAttributeStreams::StreamKey))
        .expr(write_ordinal())
        .exprs(
            [
                TemporalEntry::AttributeKind,
                TemporalEntry::ValueKind,
                TemporalEntry::ObservedAt,
                TemporalEntry::CreatedAt,
                TemporalEntry::ModifiedAt,
                TemporalEntry::DeletedAt,
                TemporalEntry::ValueDateTime,
                TemporalEntry::ExpiresAt,
                TemporalEntry::GeneratedInstanceId,
                TemporalEntry::ValueTypeKey,
                TemporalEntry::UnitCodeId,
                TemporalEntry::ValueBoolean,
                TemporalEntry::VerbatimInstanceId,
                TemporalEntry::ValueNumber,
                TemporalEntry::ValueString,
            ]
            .map(entry),
        )
        .expr(st_make_valid(entry(TemporalEntry::ValueGeometry)))
        .exprs(
            [
                TemporalEntry::ValueGeometryMembers,
                TemporalEntry::ValueObject,
                TemporalEntry::ValueUri,
                TemporalEntry::ObjectTypeKeys,
                TemporalEntry::ProofSeals,
                TemporalEntry::SubAttributes,
            ]
            .map(entry),
        );
    let mut statement = Query::insert();
    statement
        .into_table(AttributeHistory::Table)
        .columns([
            AttributeHistory::RecordedAt,
            AttributeHistory::WriteId,
            AttributeHistory::StreamKey,
            AttributeHistory::WriteOrdinal,
            AttributeHistory::AttributeKind,
            AttributeHistory::ValueKind,
            AttributeHistory::ObservedAt,
            AttributeHistory::CreatedAt,
            AttributeHistory::ModifiedAt,
            AttributeHistory::DeletedAt,
            AttributeHistory::ValueDateTime,
            AttributeHistory::ExpiresAt,
            AttributeHistory::GeneratedInstanceId,
            AttributeHistory::ValueTypeKey,
            AttributeHistory::UnitCodeId,
            AttributeHistory::ValueBoolean,
            AttributeHistory::VerbatimInstanceId,
            AttributeHistory::ValueNumber,
            AttributeHistory::ValueString,
            AttributeHistory::ValueGeometry,
            AttributeHistory::ValueGeometryMembers,
            AttributeHistory::ValueObject,
            AttributeHistory::ValueUri,
            AttributeHistory::ObjectTypeKeys,
            AttributeHistory::ProofSeals,
            AttributeHistory::SubAttributes,
        ])
        .select_from(source)?
        .on_conflict(
            OnConflict::columns([AttributeHistory::RecordedAt, AttributeHistory::WriteId, AttributeHistory::WriteOrdinal])
                .do_nothing()
                .to_owned(),
        );
    Ok(statement)
}

/// The Scope entries of Entities with a Temporal Evolution.
fn scope_entries(keys: &[i64]) -> SelectStatement {
    entries_with_evolution(keys, entry_kind_in(&[TemporalEntryKind::Scope.code()]))
}

/// Remove the Scope rows a claimed Scope entry records again under its own
/// instance identity.
pub(crate) fn replaced_scopes_statement(keys: &[i64]) -> DeleteStatement {
    let mut replaced = scope_entries(keys);
    replaced
        .expr(Expr::val(1))
        .and_where(Expr::col((ScopeHistory::Table, ScopeHistory::TemporalEntityKey)).equals((Te, TemporalEntities::TemporalEntityKey)))
        .and_where(
            SimpleExpr::from(Expr::col((ScopeHistory::Table, ScopeHistory::GeneratedInstanceId))).not_distinct_from(entry(TemporalEntry::GeneratedInstanceId)),
        )
        .and_where(
            SimpleExpr::from(Expr::col((ScopeHistory::Table, ScopeHistory::VerbatimInstanceId))).not_distinct_from(entry(TemporalEntry::VerbatimInstanceId)),
        );
    Query::delete().from_table(ScopeHistory::Table).and_where(Expr::exists(replaced)).to_owned()
}

/// The `scope_history` row of every claimed Scope entry (clause 4.18), its
/// label path cast to the `ltree` a `scopeQ` matches.
pub(crate) fn scope_history_statement(keys: &[i64]) -> Result<InsertStatement, SeaQueryError> {
    let mut source = scope_entries(keys);
    source
        .expr(intent(TemporalAppendIntents::RecordedAt))
        .column((Te, TemporalEntities::TemporalEntityKey))
        .expr(intent(TemporalAppendIntents::WriteId))
        .expr(write_ordinal())
        .exprs(
            [
                TemporalEntry::ValueKind,
                TemporalEntry::ObservedAt,
                TemporalEntry::CreatedAt,
                TemporalEntry::ModifiedAt,
                TemporalEntry::DeletedAt,
                TemporalEntry::GeneratedInstanceId,
                TemporalEntry::VerbatimInstanceId,
            ]
            .map(entry),
        )
        .expr(cast(entry(TemporalEntry::ScopePath), PostgresType::Ltree))
        .expr(entry(TemporalEntry::VerbatimScopePath));
    let mut statement = Query::insert();
    statement
        .into_table(ScopeHistory::Table)
        .columns([
            ScopeHistory::RecordedAt,
            ScopeHistory::TemporalEntityKey,
            ScopeHistory::WriteId,
            ScopeHistory::WriteOrdinal,
            ScopeHistory::ValueKind,
            ScopeHistory::ObservedAt,
            ScopeHistory::CreatedAt,
            ScopeHistory::ModifiedAt,
            ScopeHistory::DeletedAt,
            ScopeHistory::GeneratedInstanceId,
            ScopeHistory::VerbatimInstanceId,
            ScopeHistory::ScopePath,
            ScopeHistory::VerbatimScopePath,
        ])
        .select_from(source)?
        .on_conflict(
            OnConflict::columns([ScopeHistory::RecordedAt, ScopeHistory::WriteId, ScopeHistory::WriteOrdinal])
                .do_nothing()
                .to_owned(),
        );
    Ok(statement)
}
