//! The write records and the temporal catalogue an application establishes
//! before it records history.

use crate::intent_application::intent_source::{E, I, IntentSource, Te, entry, entry_kind_in, intent, source_entries, source_intents, write_ordinal};
use andromeda_postgresql_codes::temporal_entry_kind::TemporalEntryKind;
use andromeda_postgresql_schema::{
    queue::{TemporalAppendIntents, WriteRecords},
    temporal::{TemporalAttributeStreams, TemporalEntities},
    temporal_intent::TemporalEntry,
};
use andromeda_postgresql_seaquery::{
    branch::qualified,
    clock::now,
    pg_array::has_elements,
    returning_image::{New, Old},
    unnest::unnest_of,
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
    OnConflict,
    Order,
    Query,
    SelectStatement,
    SimpleExpr,
    WindowStatement,
    WithClause,
    error::Error as SeaQueryError,
};

/// The Temporal Evolutions the statement wrote, `evolved`.
#[derive(Iden, Clone, Copy)]
#[iden = "evolved"]
struct Evolved;

/// The stream grains the entries of the evolved Entities extend, `grains`.
#[derive(Iden, Clone, Copy)]
#[iden = "grains"]
struct Grains;

/// The insert of the streams of the evolutions the statement created,
/// `created_streams`.
#[derive(Iden, Clone, Copy)]
#[iden = "created_streams"]
struct CreatedStreams;

/// Whether the statement inserted the evolution rather than updating one,
/// `created`, a column of `evolved` and `grains`.
#[derive(Iden, Clone, Copy)]
#[iden = "created"]
struct Created;

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

/// The bookkeeping record of every write the intents belong to.
///
/// The record persists `recorded_at` because the history rows deduplicate on
/// a key holding the partition column: a replay reads the stored instant back
/// and lands on the rows it already produced. `accepted_at` is when the
/// record itself was written. The record is no gate: a write replayed under
/// its original identity changes nothing here, and its rows collide on their
/// own write identity.
pub(crate) fn write_records_statement(source: &IntentSource<'_>) -> Result<InsertStatement, SeaQueryError> {
    let mut select = Query::select();
    select
        .distinct()
        .expr(intent(TemporalAppendIntents::WriteId))
        .expr(intent(TemporalAppendIntents::RecordedAt))
        .expr(now());
    source_intents(&mut select, source);
    let mut statement = Query::insert();
    statement
        .into_table(WriteRecords::Table)
        .columns([WriteRecords::WriteId, WriteRecords::RecordedAt, WriteRecords::AcceptedAt])
        .select_from(select)?
        .on_conflict(OnConflict::new().do_nothing().to_owned());
    Ok(statement)
}

/// The Temporal Evolution of every Entity an intent states types for, created
/// when it is missing and given the types of the latest such intent, beside
/// the stream of every attribute grain the entries of those Entities extend.
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
/// index led by `stream_key` at its right edge. An entry of an Entity no intent states types for reaches none
/// of them: [`untyped_streams_statement`] writes its stream.
pub(crate) fn evolutions_and_streams_statement(source: &IntentSource<'_>, schema: Option<&DynIden>) -> Result<InsertStatement, SeaQueryError> {
    let grain_columns = [
        TemporalAttributeStreams::TemporalEntityKey,
        TemporalAttributeStreams::AttributeNameKey,
        TemporalAttributeStreams::DatasetId,
    ];
    let mut grains = Query::select();
    grains
        .column((Evolved, TemporalEntities::TemporalEntityKey))
        .expr(entry(TemporalEntry::AttributeNameKey))
        .expr(entry(TemporalEntry::DatasetId))
        .column((Evolved, Created))
        .expr(Func::min(intent(TemporalAppendIntents::IntentKey)))
        .expr(Func::min(write_ordinal()));
    source_entries(&mut grains, source);
    grains
        .from(Evolved)
        .and_where(Expr::col((Evolved, TemporalEntities::Id)).equals((I, TemporalAppendIntents::EntityId)))
        .and_where(entry_kind_in(&[TemporalEntryKind::Observation.code(), TemporalEntryKind::Tombstone.code()]))
        .add_group_by([
            Expr::col((Evolved, TemporalEntities::TemporalEntityKey)),
            entry(TemporalEntry::AttributeNameKey),
            entry(TemporalEntry::DatasetId),
            Expr::col((Evolved, Created)),
        ]);
    let streams_of = |created: SimpleExpr| -> Result<InsertStatement, SeaQueryError> {
        let mut select = Query::select();
        select
            .columns(grain_columns.map(|column| (Grains, column)))
            .from(Grains)
            .and_where(created)
            .order_by((Grains, FirstIntent), Order::Asc)
            .order_by((Grains, FirstOrdinal), Order::Asc);
        let mut insert = Query::insert();
        insert
            .into_table(qualified(schema, TemporalAttributeStreams::Table))
            .columns(grain_columns)
            .select_from(select)?;
        Ok(insert)
    };
    let created = || SimpleExpr::from(Expr::col((Grains, Created)));
    let mut with = WithClause::new();
    with.cte(
        CommonTableExpression::new()
            .table_name(Evolved)
            .columns([
                TemporalEntities::TemporalEntityKey.into_iden(),
                TemporalEntities::Id.into_iden(),
                Created.into_iden(),
            ])
            .query(evolutions_statement(source, schema)?)
            .to_owned(),
    )
    .cte(
        CommonTableExpression::new()
            .table_name(Grains)
            .columns(
                grain_columns
                    .map(IntoIden::into_iden)
                    .into_iter()
                    .chain([Created.into_iden(), FirstIntent.into_iden(), FirstOrdinal.into_iden()]),
            )
            .query(grains)
            .to_owned(),
    )
    .cte(CommonTableExpression::new().table_name(CreatedStreams).query(streams_of(created())?).to_owned());
    let mut statement = streams_of(created().not())?;
    statement.on_conflict(OnConflict::columns(grain_columns).do_nothing().to_owned()).with_cte(with);
    Ok(statement)
}

/// The Temporal Evolution of every Entity an intent states types for, created
/// when it is missing and given the types of the latest such intent,
/// returning its key, its identifier and whether the insert created it.
///
/// Clause 4.5.6 makes the Temporal Evolution "the Entity [...] as mandated by
/// clause 4.5.1", so an intent stating types brings it into being; one stating
/// none (every deletion among them) only extends one that exists. A source can
/// hold several intents of one Entity, and a conflict action may change a row
/// once per statement, so one row per Entity is inserted: the types of its
/// latest intent, its first intent's instant as `created_at` and its latest's
/// as `modified_at`, which is what applying them one after the other leaves.
/// `created_at` is never moved (clause 4.8).
fn evolutions_statement(source: &IntentSource<'_>, schema: Option<&DynIden>) -> Result<InsertStatement, SeaQueryError> {
    let mut select = Query::select();
    select
        .distinct_on([(I, TemporalAppendIntents::EntityId)])
        .expr(intent(TemporalAppendIntents::EntityId))
        .expr_window(
            Func::min(intent(TemporalAppendIntents::RecordedAt)),
            WindowStatement::partition_by((I, TemporalAppendIntents::EntityId)),
        )
        .expr(intent(TemporalAppendIntents::RecordedAt))
        .expr(intent(TemporalAppendIntents::EntityTypeNameKeys));
    source_intents(&mut select, source);
    select
        .and_where(has_elements(intent(TemporalAppendIntents::EntityTypeNameKeys)))
        .order_by((I, TemporalAppendIntents::EntityId), Order::Asc)
        .order_by((I, TemporalAppendIntents::IntentKey), Order::Desc);
    let mut statement = Query::insert();
    statement
        .into_table(qualified(schema, TemporalEntities::Table))
        .columns([
            TemporalEntities::Id,
            TemporalEntities::CreatedAt,
            TemporalEntities::ModifiedAt,
            TemporalEntities::EntityTypeNameKeys,
        ])
        .select_from(select)?
        .on_conflict(
            OnConflict::column(TemporalEntities::Id)
                .update_columns([TemporalEntities::ModifiedAt, TemporalEntities::EntityTypeNameKeys])
                .to_owned(),
        )
        .returning(Query::returning().exprs([
            Expr::col((New, TemporalEntities::TemporalEntityKey)),
            Expr::col((New, TemporalEntities::Id)),
            Expr::col((Old, TemporalEntities::TemporalEntityKey)).is_null(),
        ]));
    Ok(statement)
}

/// The intents that observe an Entity which has no Temporal Evolution and
/// state no type to create one with, each with its Entity.
///
/// Read after the evolutions are written, so an intent the same source gives
/// an evolution through an earlier typed intent is not among them. An intent
/// observes when it holds no entry or any entry that does not end an
/// instance: clause 4.5.1 requires only `id` and `type` of an Entity, so a
/// write observing one with no attribute still extends its evolution.
pub(crate) fn untyped_observations_statement(source: &IntentSource<'_>, schema: Option<&DynIden>) -> SelectStatement {
    let mut ends_nothing = Query::select();
    ends_nothing
        .expr(Expr::val(1))
        .from_function(unnest_of(intent(TemporalAppendIntents::Entries)), E)
        .and_where(entry(TemporalEntry::EntryKind).ne(TemporalEntryKind::Tombstone.code()));
    let mut evolution = Query::select();
    evolution
        .expr(Expr::val(1))
        .from_as(qualified(schema, TemporalEntities::Table), Te)
        .and_where(Expr::col((Te, TemporalEntities::Id)).equals((I, TemporalAppendIntents::EntityId)));
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
/// another writer committed in the meantime is found. An entry of an Entity
/// with no Temporal Evolution reaches no stream: the join to
/// `temporal_entities` drops it, which is how the end of an instance of an
/// evolution clause 5.6.16 already deleted records nothing.
pub(crate) fn untyped_streams_statement(source: &IntentSource<'_>, schema: Option<&DynIden>) -> Result<InsertStatement, SeaQueryError> {
    let mut select = Query::select();
    select
        .distinct()
        .column((Te, TemporalEntities::TemporalEntityKey))
        .expr(entry(TemporalEntry::AttributeNameKey))
        .expr(entry(TemporalEntry::DatasetId));
    source_entries(&mut select, source);
    select
        .from_as(qualified(schema, TemporalEntities::Table), Te)
        .and_where(Expr::col((Te, TemporalEntities::Id)).equals((I, TemporalAppendIntents::EntityId)))
        .and_where(has_elements(intent(TemporalAppendIntents::EntityTypeNameKeys)).not())
        .and_where(entry_kind_in(&[TemporalEntryKind::Observation.code(), TemporalEntryKind::Tombstone.code()]));
    let mut statement = Query::insert();
    statement
        .into_table(qualified(schema, TemporalAttributeStreams::Table))
        .columns([
            TemporalAttributeStreams::TemporalEntityKey,
            TemporalAttributeStreams::AttributeNameKey,
            TemporalAttributeStreams::DatasetId,
        ])
        .select_from(select)?
        .on_conflict(
            OnConflict::columns([
                TemporalAttributeStreams::TemporalEntityKey,
                TemporalAttributeStreams::AttributeNameKey,
                TemporalAttributeStreams::DatasetId,
            ])
            .do_nothing()
            .to_owned(),
        );
    Ok(statement)
}
