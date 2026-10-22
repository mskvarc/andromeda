//! The history rows, stream extents and shapes, Scope rows and instance claims
//! an application records from the entries of its source.
//!
//! Each statement joins the entries to their Entity's Temporal Evolution, so an
//! entry of an Entity without one records nothing; the catalogue statements
//! ran first and refused the only entries that would have needed one. An
//! attribute entry is read only when the application records it: the end of
//! an instance of an evolution the application created before every intent
//! of its Entity stating types records nothing, as applying the intents one
//! after the other leaves it.

use crate::intent_application::{
    deletion_judgement::{created_by_application, ended_before_evolution},
    intent_source::{E, I, IntentSource, S, Te, entry, entry_kind_in, intent, source_entries, write_ordinal},
    scope_judgement::recorded_scope_entry,
};
use andromeda_postgresql_codes::temporal_entry_kind::TemporalEntryKind;
use andromeda_postgresql_query::{
    history_source::entry_values::{attribute_entry_values, recorded_geometry, scope_entry_values},
    tombstone::carries_a_value,
};
use andromeda_postgresql_schema::{
    queue::TemporalAppendIntents,
    temporal::{AttributeHistory, AttributeHistoryInstanceClaims, ScopeHistory, TemporalAttributeStreams, TemporalEntities, TemporalStreamShapes},
    temporal_intent::TemporalEntry,
};
use andromeda_postgresql_seaquery::{
    branch::qualified,
    pg_operator::PgBinaryOperator,
    postgis::{bounding_box_contains, st_collect, st_collect_rows, st_envelope},
};
use andromeda_postgresql_temporal_query::{alias::Ah, recorded_shape::recorded_shape};
use sea_query::{
    Condition,
    DeleteStatement,
    DynIden,
    Expr,
    ExprTrait,
    Iden,
    InsertStatement,
    IntoIden,
    OnConflict,
    Query,
    SelectStatement,
    SimpleExpr,
    TableRef,
    UpdateStatement,
    error::Error as SeaQueryError,
};

/// The sibling-stream alias the instance replacement searches, `s2`.
#[derive(Iden, Clone, Copy)]
#[iden = "s2"]
struct S2;

/// The alias of the box each stream's entries span, `grown`.
#[derive(Iden, Clone, Copy)]
#[iden = "grown"]
struct Grown;

/// The box one stream's entries span, `extent`.
#[derive(Iden, Clone, Copy)]
#[iden = "extent"]
struct Extent;

/// The two attribute entry kinds.
fn attribute_entries() -> SimpleExpr {
    entry_kind_in(&[TemporalEntryKind::Observation.code(), TemporalEntryKind::Tombstone.code()])
}

/// The entries of `kinds` of Entities with a Temporal Evolution, the
/// evolution under `te`.
fn entries_with_evolution(source: &IntentSource<'_>, schema: Option<&DynIden>, kinds: SimpleExpr) -> SelectStatement {
    let mut select = Query::select();
    source_entries(&mut select, source);
    select
        .from_as(qualified(schema, TemporalEntities::Table), Te)
        .and_where(Expr::col((Te, TemporalEntities::Id)).equals((I, TemporalAppendIntents::EntityId)))
        .and_where(kinds);
    select
}

/// The attribute entries the application records, the evolution under `te`.
fn recorded_attribute_entries(source: &IntentSource<'_>, schema: Option<&DynIden>) -> SelectStatement {
    let mut select = entries_with_evolution(source, schema, attribute_entries());
    select.and_where(ended_before_evolution(source, created_by_application(source)).not());
    select
}

/// The attribute entries the application records beside the stream each
/// extends, under `s`.
fn entries_with_stream(source: &IntentSource<'_>, schema: Option<&DynIden>) -> SelectStatement {
    let mut select = recorded_attribute_entries(source, schema);
    select
        .from_as(qualified(schema, TemporalAttributeStreams::Table), S)
        .and_where(Expr::col((S, TemporalAttributeStreams::TemporalEntityKey)).equals((Te, TemporalEntities::TemporalEntityKey)))
        .and_where(Expr::col((S, TemporalAttributeStreams::AttributeNameKey)).equals((E, TemporalEntry::AttributeNameKey)))
        .and_where(SimpleExpr::from(Expr::col((S, TemporalAttributeStreams::DatasetId))).not_distinct_from(entry(TemporalEntry::DatasetId)));
    select
}

/// The attribute entries the application records that carry a
/// client-supplied `instanceId`.
fn verbatim_entries(source: &IntentSource<'_>, schema: Option<&DynIden>) -> SelectStatement {
    let mut select = recorded_attribute_entries(source, schema);
    select.and_where(entry(TemporalEntry::VerbatimInstanceId).is_not_null());
    select
}

/// The history rows the entries replace, by stream and write identity: a
/// client that supplied its own `instanceId` (clause 4.5.7) names one
/// instance of the attribute across every `datasetId`, so the row holding it
/// on any sibling stream goes first.
pub(crate) fn replaced_instances_statement(source: &IntentSource<'_>, schema: Option<&DynIden>) -> SelectStatement {
    let mut replaced = verbatim_entries(source, schema);
    replaced
        .column((AttributeHistory::Table, AttributeHistory::StreamKey))
        .column((AttributeHistory::Table, AttributeHistory::RecordedAt))
        .column((AttributeHistory::Table, AttributeHistory::WriteId))
        .column((AttributeHistory::Table, AttributeHistory::WriteOrdinal))
        .from_as(qualified(schema, TemporalAttributeStreams::Table), S2)
        .from(qualified(schema, AttributeHistory::Table))
        .and_where(Expr::col((S2, TemporalAttributeStreams::TemporalEntityKey)).equals((Te, TemporalEntities::TemporalEntityKey)))
        .and_where(Expr::col((S2, TemporalAttributeStreams::AttributeNameKey)).equals((E, TemporalEntry::AttributeNameKey)))
        .and_where(Expr::col((AttributeHistory::Table, AttributeHistory::StreamKey)).equals((S2, TemporalAttributeStreams::StreamKey)))
        .and_where(Expr::col((AttributeHistory::Table, AttributeHistory::VerbatimInstanceId)).equals((E, TemporalEntry::VerbatimInstanceId)));
    replaced
}

/// Release the instance claims the entries are about to stake again,
/// so a replacement is not refused as a conflict with the instance it
/// replaces.
pub(crate) fn released_claims_statement(source: &IntentSource<'_>, schema: Option<&DynIden>) -> DeleteStatement {
    let mut released = verbatim_entries(source, schema);
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
        .from_table(qualified(schema, AttributeHistoryInstanceClaims::Table))
        .and_where(Expr::exists(released))
        .to_owned()
}

/// Stake a claim on every `(entity, attribute, instanceId)` the entries
/// carry; the claims' primary key refuses two instances of one attribute
/// naming one `instanceId`, within the source or against another writer, so
/// the insert carries no `ON CONFLICT`.
pub(crate) fn claims_statement(source: &IntentSource<'_>, schema: Option<&DynIden>) -> Result<InsertStatement, SeaQueryError> {
    let mut select = verbatim_entries(source, schema);
    select
        .column((Te, TemporalEntities::TemporalEntityKey))
        .expr(entry(TemporalEntry::AttributeNameKey))
        .expr(entry(TemporalEntry::VerbatimInstanceId));
    let mut statement = Query::insert();
    statement
        .into_table(qualified(schema, AttributeHistoryInstanceClaims::Table))
        .columns([
            AttributeHistoryInstanceClaims::TemporalEntityKey,
            AttributeHistoryInstanceClaims::AttributeNameKey,
            AttributeHistoryInstanceClaims::VerbatimInstanceId,
        ])
        .select_from(select)?;
    Ok(statement)
}

/// The `attribute_history` row of every attribute entry, numbered from its
/// intent's base ordinal.
///
/// The values are the ones a temporal read of the live branch projects from
/// the entry while it is still queued. The conflict target is the write
/// identity, so an entry recorded once is never recorded twice.
pub(crate) fn history_statement(source: &IntentSource<'_>, schema: Option<&DynIden>) -> Result<InsertStatement, SeaQueryError> {
    let (value_columns, values): (Vec<AttributeHistory>, Vec<SimpleExpr>) = attribute_entry_values(entry).into_iter().unzip();
    let mut select = entries_with_stream(source, schema);
    select
        .expr(intent(TemporalAppendIntents::RecordedAt))
        .expr(intent(TemporalAppendIntents::WriteId))
        .column((S, TemporalAttributeStreams::StreamKey))
        .expr(write_ordinal())
        .exprs(values);
    let mut statement = Query::insert();
    statement
        .into_table(qualified(schema, AttributeHistory::Table))
        .columns(
            [
                AttributeHistory::RecordedAt,
                AttributeHistory::WriteId,
                AttributeHistory::StreamKey,
                AttributeHistory::WriteOrdinal,
            ]
            .into_iter()
            .chain(value_columns),
        )
        .select_from(select)?
        .on_conflict(
            OnConflict::columns([AttributeHistory::RecordedAt, AttributeHistory::WriteId, AttributeHistory::WriteOrdinal])
                .do_nothing()
                .to_owned(),
        );
    Ok(statement)
}

/// Grow the extent of every stream an entry of the source records a geometry
/// on to cover it.
///
/// The box each stream's entries span is taken from the geometry the history
/// row stores, so the extent covers exactly what was recorded. A stream whose
/// extent already covers the box is left alone, so a write inside it updates
/// no row. An extent never shrinks: one too large still holds every geometry
/// of its stream.
pub(crate) fn extent_growth_statement(source: &IntentSource<'_>, schema: Option<&DynIden>) -> UpdateStatement {
    let geometry = recorded_geometry(entry);
    let mut grown = entries_with_stream(source, schema);
    grown
        .column((S, TemporalAttributeStreams::StreamKey))
        .expr_as(st_envelope(st_collect_rows(geometry)), Extent)
        .and_where(entry(TemporalEntry::ValueGeometry).is_not_null())
        .group_by_col((S, TemporalAttributeStreams::StreamKey));
    let extent = || Expr::col((TemporalAttributeStreams::Table, TemporalAttributeStreams::ValueExtent));
    let box_of = || Expr::col((Grown, Extent));
    Query::update()
        .table(qualified(schema, TemporalAttributeStreams::Table))
        .value(TemporalAttributeStreams::ValueExtent, st_envelope(st_collect(extent(), box_of())))
        .from(TableRef::SubQuery(Box::new(grown), Grown.into_iden()))
        .and_where(Expr::col((TemporalAttributeStreams::Table, TemporalAttributeStreams::StreamKey)).equals((Grown, TemporalAttributeStreams::StreamKey)))
        .cond_where(Condition::any().add(extent().is_null()).add(bounding_box_contains(extent(), box_of()).not()))
        .to_owned()
}

/// Add to the shapes of every stream an attribute entry of the source records
/// an instance on the combination of family and value shape that instance
/// carries.
///
/// The shape is projected from the values the history insert writes, through
/// the projection an aggregation reads, so the catalogue names exactly the
/// shapes the stream's rows carry. A combination the stream already holds
/// conflicts and writes nothing, which is the steady state of a stream. A
/// tombstone carries no value and adds no shape.
pub(crate) fn stream_shapes_statement(source: &IntentSource<'_>, schema: Option<&DynIden>) -> Result<InsertStatement, SeaQueryError> {
    let mut entries = entries_with_stream(source, schema);
    entries
        .column((S, TemporalAttributeStreams::StreamKey))
        .column((S, TemporalAttributeStreams::AttributeNameKey));
    for (column, value) in attribute_entry_values(entry) {
        entries.expr_as(value, column);
    }
    let mut select = Query::select();
    select
        .distinct()
        .column((Ah, TemporalAttributeStreams::StreamKey))
        .column((Ah, TemporalAttributeStreams::AttributeNameKey))
        .column((Ah, AttributeHistory::AttributeKind))
        .expr(recorded_shape())
        .from_subquery(entries, Ah)
        .and_where(carries_a_value(Ah));
    let mut statement = Query::insert();
    statement
        .into_table(qualified(schema, TemporalStreamShapes::Table))
        .columns([
            TemporalStreamShapes::StreamKey,
            TemporalStreamShapes::AttributeNameKey,
            TemporalStreamShapes::AttributeKind,
            TemporalStreamShapes::RecordedShape,
        ])
        .select_from(select)?
        .on_conflict(
            OnConflict::columns([
                TemporalStreamShapes::StreamKey,
                TemporalStreamShapes::AttributeKind,
                TemporalStreamShapes::RecordedShape,
            ])
            .do_nothing()
            .to_owned(),
        );
    Ok(statement)
}

/// The Scope entries of Entities with a Temporal Evolution, events and
/// assertions alike.
fn scope_entries(source: &IntentSource<'_>, schema: Option<&DynIden>) -> SelectStatement {
    entries_with_evolution(
        source,
        schema,
        entry_kind_in(&[TemporalEntryKind::Scope.code(), TemporalEntryKind::ScopeAssertion.code()]),
    )
}

/// The `scope_history` row of every Scope entry the application records
/// (clause 4.18), with the values a temporal read of the live branch projects
/// from the entry while it is still queued: every event, and every assertion
/// of a Scope not already in force at its write.
///
/// The conflict target is the write identity, which is what names a Scope
/// row: clause 4.5.6 gives the temporal Scope no `instanceId`. An entry
/// recorded once is never recorded twice.
pub(crate) fn scope_history_statement(source: &IntentSource<'_>, schema: Option<&DynIden>) -> Result<InsertStatement, SeaQueryError> {
    let (value_columns, values): (Vec<ScopeHistory>, Vec<SimpleExpr>) = scope_entry_values(entry).into_iter().unzip();
    let mut select = scope_entries(source, schema);
    select
        .cond_where(recorded_scope_entry(source, schema))
        .expr(intent(TemporalAppendIntents::RecordedAt))
        .column((Te, TemporalEntities::TemporalEntityKey))
        .expr(intent(TemporalAppendIntents::WriteId))
        .expr(write_ordinal())
        .exprs(values);
    let mut statement = Query::insert();
    statement
        .into_table(qualified(schema, ScopeHistory::Table))
        .columns(
            [
                ScopeHistory::RecordedAt,
                ScopeHistory::TemporalEntityKey,
                ScopeHistory::WriteId,
                ScopeHistory::WriteOrdinal,
            ]
            .into_iter()
            .chain(value_columns),
        )
        .select_from(select)?
        .on_conflict(
            OnConflict::columns([ScopeHistory::RecordedAt, ScopeHistory::WriteId, ScopeHistory::WriteOrdinal])
                .do_nothing()
                .to_owned(),
        );
    Ok(statement)
}
