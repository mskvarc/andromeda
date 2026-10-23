//! The temporal catalogue an application establishes before it records
//! history.
//!
//! A stream is inserted complete: with the shape mask and the extent of the
//! instances the entries of its grain record, so the growth that follows the
//! history leaves it alone, and the row is written once.

use crate::{
    intent_application::{
        deletion_judgement::{created_by_application, ended_before_evolution},
        evolution_upsert::{Created, Evolved, NewTypes, OldTypes, with_evolutions},
        intent_source::{E, I, StagedSource, entry, entry_kind_in, intent, source_entries, source_intents, write_ordinal},
    },
    stream_census::{StreamChange, counted_stream_addition, stream_census_change, typed_rows},
};
use andromeda_postgresql_codes::temporal_entry_kind::TemporalEntryKind;
use andromeda_postgresql_query::{
    entity_sides::holds_evolution,
    history_source::entry_values::{attribute_entry_values, recorded_geometry},
    tombstone::entry_carries_a_value,
};
use andromeda_postgresql_schema::{current::Entities, queue::TemporalAppendIntents, temporal::TemporalAttributeStreams, temporal_intent::TemporalEntry};
use andromeda_postgresql_seaquery::{
    bit_string::bit_union_of_rows,
    branch::qualified,
    pg_array::{any, array_of_subquery, has_elements},
    postgis::{st_collect_rows, st_envelope},
    unnest::unnest_of,
};
use andromeda_postgresql_temporal_query::{
    alias::{Ah, Te},
    shape_mask::{no_shapes, recorded_combination},
};
use sea_query::{
    CommonTableExpression,
    Cond,
    DynIden,
    Expr,
    ExprTrait,
    Func,
    Iden,
    InsertStatement,
    IntoIden,
    JoinType,
    OnConflict,
    Order,
    Query,
    SelectStatement,
    SimpleExpr,
    UnionType,
    WithClause,
    WithQuery,
    error::Error as SeaQueryError,
};

/// The stream grains the entries of the evolved Entities extend, `grains`.
#[derive(Iden, Clone, Copy)]
#[iden = "grains"]
struct Grains;

/// The insert of the streams of the evolutions the statement created,
/// `created_streams`.
#[derive(Iden, Clone, Copy)]
#[iden = "created_streams"]
struct CreatedStreams;

/// The insert of the streams of the evolutions that existed,
/// `existing_streams`.
#[derive(Iden, Clone, Copy)]
#[iden = "existing_streams"]
struct ExistingStreams;

/// The census change of the streams the statement inserted,
/// `stream_census`, whose only purpose is to run.
#[derive(Iden, Clone, Copy)]
#[iden = "stream_census"]
struct StreamCensus;

/// Alias `gt`: one type an evolution is recorded under after the statement.
#[derive(Iden, Clone, Copy)]
#[iden = "gt"]
struct GainedTypeKey;

/// The first intent of the source holding an entry of a grain,
/// `first_intent`, a column of `grains`.
#[derive(Iden, Clone, Copy)]
#[iden = "first_intent"]
struct FirstIntent;

/// The lowest write ordinal of the grain's entries, `first_ordinal`, a column
/// of `grains`.
#[derive(Iden, Clone, Copy)]
#[iden = "first_ordinal"]
struct FirstOrdinal;

/// The geometry an entry records, which its stream's extent covers,
/// `extent_geometry`.
#[derive(Iden, Clone, Copy)]
#[iden = "extent_geometry"]
struct ExtentGeometry;

/// The grain a stream is unique on, which a stream insert arbitrates on.
const GRAIN_COLUMNS: [TemporalAttributeStreams; 3] = [
    TemporalAttributeStreams::EntityKey,
    TemporalAttributeStreams::AttributeNameKey,
    TemporalAttributeStreams::DatasetId,
];

/// The catalogue columns a stream is inserted with beside its grain.
const SEEDED_COLUMNS: [TemporalAttributeStreams; 2] = [TemporalAttributeStreams::ShapeMask, TemporalAttributeStreams::ValueExtent];

/// Extend `entries`, one row per entry under its grain columns, by the history
/// columns the entry records and the geometry it records, so the grains are
/// aggregated from them read as the `attribute_history` rows `ah`.
fn with_recorded_values(entries: &mut SelectStatement) {
    for (column, value) in attribute_entry_values(entry) {
        entries.expr_as(value, column);
    }
    entries.expr_as(recorded_geometry(entry), ExtentGeometry);
}

/// The [`SEEDED_COLUMNS`] of the stream of each grain the rows `ah` are grouped
/// by: every combination of family and value shape the grain's instances
/// carry, a deletion carrying none, and the box of every geometry they hold.
fn seeded_values(grains: &mut SelectStatement) {
    grains
        .expr(Func::coalesce([bit_union_of_rows(recorded_combination()), no_shapes()]))
        .expr(st_envelope(st_collect_rows(Expr::col((Ah, ExtentGeometry)))));
}

/// The Temporal Evolution of every Entity an intent states types for, created
/// when it is missing and given every type those intents state that it lacks,
/// beside the stream of every attribute grain the entries of those Entities
/// extend, returning each evolution it created or gave a type, with whether
/// it created it and the types an existing one gained.
///
/// The end of an instance of an evolution the statement created extends no
/// stream when it comes before every intent of its Entity stating types: it
/// records nothing ([`ended_before_evolution`]).
///
/// Each evolution is returned with whether the statement created it. An
/// evolution it created is invisible to every other transaction until this
/// one commits, so no other writer can give it a stream, and its streams are
/// inserted without arbitration; a stream of an evolution that existed may
/// already be there, or be another writer's, so those arbitrate on the grain.
/// Both read the evolution's key from what the evolution insert returned, so
/// an evolution another writer committed while the statement ran still gets
/// its streams. The streams are inserted in the order the history insert
/// later reaches their entries (intent, then write ordinal), so the keys
/// they are given ascend in that order and the history rows extend every
/// index led by `stream_key` at its right edge. An entry of an Entity no
/// intent states types for reaches none of them: [`untyped_streams_statement`]
/// writes its stream. The evolutions and the type memberships a temporal type
/// selection walks are written first, in the same statement
/// ([`with_evolutions`]).
///
/// The streams it inserts are counted into the stream census under the types
/// their evolution holds; the types an existing evolution gained are counted
/// over all its streams once the statement has run, when every stream another
/// writer committed before the upsert locked the row is visible
/// ([`gained_types_census`](crate::stream_census::gained_types_census)).
pub(crate) fn evolutions_and_streams_statement(source: &StagedSource<'_>, schema: Option<&DynIden>) -> Result<WithQuery, SeaQueryError> {
    let stream_columns = [
        TemporalAttributeStreams::EntityKey,
        TemporalAttributeStreams::AttributeNameKey,
        TemporalAttributeStreams::DatasetId,
    ];
    let mut entries = Query::select();
    entries
        .expr_as(Expr::col((Evolved, Entities::EntityKey)), TemporalAttributeStreams::EntityKey)
        .expr_as(entry(TemporalEntry::AttributeNameKey), TemporalAttributeStreams::AttributeNameKey)
        .expr_as(entry(TemporalEntry::DatasetId), TemporalAttributeStreams::DatasetId)
        .expr_as(Expr::col((Evolved, Created)), Created)
        .expr_as(intent(TemporalAppendIntents::IntentKey), FirstIntent)
        .expr_as(write_ordinal(), FirstOrdinal);
    with_recorded_values(&mut entries);
    source_entries(&mut entries, source);
    entries
        .from(Evolved)
        .and_where(Expr::col((Evolved, Entities::Id)).equals((I, TemporalAppendIntents::EntityId)))
        .and_where(entry_kind_in(&[TemporalEntryKind::Observation.code(), TemporalEntryKind::Tombstone.code()]))
        .and_where(ended_before_evolution(source, created_evolution()).not());
    let mut grains = Query::select();
    grains
        .columns(stream_columns.map(|column| (Ah, column)))
        .column((Ah, Created))
        .expr(Func::min(Expr::col((Ah, FirstIntent))))
        .expr(Func::min(Expr::col((Ah, FirstOrdinal))));
    seeded_values(&mut grains);
    grains
        .from_subquery(entries, Ah)
        .group_by_columns(stream_columns.map(|column| (Ah, column)))
        .group_by_col((Ah, Created));
    let streams_of = |created: SimpleExpr| -> Result<InsertStatement, SeaQueryError> {
        let mut select = Query::select();
        select
            .columns(stream_columns.into_iter().chain(SEEDED_COLUMNS).map(|column| (Grains, column)))
            .from(Grains)
            .and_where(created)
            .order_by((Grains, FirstIntent), Order::Asc)
            .order_by((Grains, FirstOrdinal), Order::Asc);
        let mut insert = Query::insert();
        insert
            .into_table(qualified(schema, TemporalAttributeStreams::Table))
            .columns(stream_columns.into_iter().chain(SEEDED_COLUMNS))
            .select_from(select)?
            .returning(Query::returning().columns([TemporalAttributeStreams::EntityKey, TemporalAttributeStreams::AttributeNameKey]));
        Ok(insert)
    };
    let created = || SimpleExpr::from(Expr::col((Grains, Created)));
    let mut with = WithClause::new();
    with_evolutions(&mut with, source, schema)?;
    with.cte(
        CommonTableExpression::new()
            .table_name(Grains)
            .columns(
                stream_columns
                    .map(IntoIden::into_iden)
                    .into_iter()
                    .chain([Created.into_iden(), FirstIntent.into_iden(), FirstOrdinal.into_iden()])
                    .chain(SEEDED_COLUMNS.map(IntoIden::into_iden)),
            )
            .query(grains)
            .to_owned(),
    )
    .cte(CommonTableExpression::new().table_name(CreatedStreams).query(streams_of(created())?).to_owned());
    let mut existing_streams = streams_of(created().not())?;
    existing_streams.on_conflict(OnConflict::columns(GRAIN_COLUMNS).do_nothing().to_owned());
    with.cte(CommonTableExpression::new().table_name(ExistingStreams).query(existing_streams).to_owned())
        .cte(
            CommonTableExpression::new()
                .table_name(StreamCensus)
                .query(stream_census_change(schema, census_rows(), StreamChange::Added)?)
                .to_owned(),
        );
    let mut changed = Query::select();
    changed
        .column((Evolved, Entities::EntityKey))
        .column((Evolved, Created))
        .expr(gained_types())
        .from(Evolved)
        .cond_where(Cond::any().add(created_evolution()).add(has_elements(gained_types())));
    Ok(with.query(changed))
}

/// The types the evolution under `evolved` is recorded under after the
/// statement that it was not before, empty for one the statement created:
/// `ARRAY(SELECT gt FROM UNNEST(new_types) AS gt WHERE NOT gt = ANY(old_types))`.
fn gained_types() -> SimpleExpr {
    let mut gained = Query::select();
    gained
        .column(GainedTypeKey)
        .from_function(unnest_of(Expr::col((Evolved, NewTypes))), GainedTypeKey)
        .and_where(Expr::col(GainedTypeKey).eq(any(Expr::col((Evolved, OldTypes)))).not());
    array_of_subquery(gained.take())
}

/// The stream census rows of the streams [`evolutions_and_streams_statement`]
/// inserts: those of an evolution it created under every type the evolution
/// has, those of one that existed under the types it held before. The types
/// such an evolution gained are counted over all its streams afterwards, the
/// inserted ones among them.
fn census_rows() -> SelectStatement {
    let evolution_of = |streams: DynIden| {
        // clone: the join names the streams the census rows are read from.
        Query::select()
            .from(streams.clone())
            .join(
                JoinType::Join,
                Evolved,
                Expr::col((Evolved, Entities::EntityKey)).equals((streams, TemporalAttributeStreams::EntityKey)),
            )
            .take()
    };
    let inserted = |streams: DynIden, types: DynIden| {
        // clone: the census reads the attribute key of the same streams.
        typed_rows(
            evolution_of(streams.clone()),
            Expr::col((Evolved, types)),
            Expr::col((streams, TemporalAttributeStreams::AttributeNameKey)),
        )
    };
    let mut rows = inserted(CreatedStreams.into_iden(), NewTypes.into_iden());
    rows.union(UnionType::All, inserted(ExistingStreams.into_iden(), OldTypes.into_iden()));
    rows
}

/// Whether the evolution under `evolved` is one the statement created.
fn created_evolution() -> SimpleExpr {
    Expr::col((Evolved, Created))
}

/// The intents that observe an Entity which has no Temporal Evolution and
/// state no type to create one with, each with its Entity.
///
/// Read after the evolutions are written, so an intent the same source gives
/// an evolution through an earlier typed intent is not among them. An intent
/// observes when it holds no entry or any entry that does not end an
/// instance: clause 4.5.1 requires only `id` and `type` of an Entity, so a
/// write observing one with no attribute still extends its evolution.
pub(crate) fn untyped_observations_statement(source: &StagedSource<'_>, schema: Option<&DynIden>) -> SelectStatement {
    let mut ends_nothing = Query::select();
    ends_nothing
        .expr(Expr::val(1))
        .from_function(unnest_of(intent(TemporalAppendIntents::Entries)), E)
        .and_where(entry_carries_a_value(E));
    let mut evolution = Query::select();
    evolution
        .expr(Expr::val(1))
        .from_as(qualified(schema, Entities::Table), Te)
        .and_where(Expr::col((Te, Entities::Id)).equals((I, TemporalAppendIntents::EntityId)))
        .and_where(holds_evolution(Te));
    let mut statement = Query::select();
    statement
        .expr(intent(TemporalAppendIntents::IntentKey))
        .expr(intent(TemporalAppendIntents::EntityId));
    source_intents(&mut statement, source);
    statement
        .and_where(has_elements(intent(TemporalAppendIntents::EntityTypeNameKeys)).not())
        .cond_where(
            Cond::any()
                .add(has_elements(intent(TemporalAppendIntents::Entries)).not())
                .add(Expr::exists(ends_nothing)),
        )
        .and_where(Expr::exists(evolution).not())
        .order_by((I, TemporalAppendIntents::IntentKey), Order::Asc);
    statement
}

/// The stream of every attribute grain the entries of the intents stating no
/// type extend, created when it is missing.
///
/// Read after the pre-check, in a snapshot of its own, so an evolution
/// another writer committed in the meantime is found, and counted into the
/// stream census under the types the evolution holds. An entry of an Entity
/// with no Temporal Evolution reaches no stream: the join to the identity
/// rows holding one drops it, which is how the end of an instance of an
/// evolution clause 5.6.16 already deleted records nothing. Neither does the
/// end of an instance of an evolution the application created before every
/// intent of its Entity stating types ([`ended_before_evolution`]).
pub(crate) fn untyped_streams_statement(source: &StagedSource<'_>, schema: Option<&DynIden>) -> Result<WithQuery, SeaQueryError> {
    let mut entries = Query::select();
    entries
        .expr_as(Expr::col((Te, Entities::EntityKey)), TemporalAttributeStreams::EntityKey)
        .expr_as(entry(TemporalEntry::AttributeNameKey), TemporalAttributeStreams::AttributeNameKey)
        .expr_as(entry(TemporalEntry::DatasetId), TemporalAttributeStreams::DatasetId);
    with_recorded_values(&mut entries);
    source_entries(&mut entries, source);
    entries
        .from_as(qualified(schema, Entities::Table), Te)
        .and_where(Expr::col((Te, Entities::Id)).equals((I, TemporalAppendIntents::EntityId)))
        .and_where(holds_evolution(Te))
        .and_where(has_elements(intent(TemporalAppendIntents::EntityTypeNameKeys)).not())
        .and_where(entry_kind_in(&[TemporalEntryKind::Observation.code(), TemporalEntryKind::Tombstone.code()]))
        .and_where(ended_before_evolution(source, created_by_application(source)).not());
    let mut select = Query::select();
    select.columns(GRAIN_COLUMNS.map(|column| (Ah, column)));
    seeded_values(&mut select);
    select.from_subquery(entries, Ah).group_by_columns(GRAIN_COLUMNS.map(|column| (Ah, column)));
    let mut statement = Query::insert();
    statement
        .into_table(qualified(schema, TemporalAttributeStreams::Table))
        .columns(GRAIN_COLUMNS.into_iter().chain(SEEDED_COLUMNS))
        .select_from(select)?
        .on_conflict(OnConflict::columns(GRAIN_COLUMNS).do_nothing().to_owned());
    counted_stream_addition(schema, statement)
}
