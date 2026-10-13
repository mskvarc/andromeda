//! The write records and the temporal catalogue an application establishes
//! before it records history.

use crate::intent_application::intent_source::{E, I, IntentSource, Te, entry, entry_kind_in, intent, source_entries, source_intents};
use andromeda_postgresql_codes::temporal_entry_kind::TemporalEntryKind;
use andromeda_postgresql_schema::{
    queue::{TemporalAppendIntents, WriteRecords},
    temporal::{TemporalAttributeStreams, TemporalEntities},
    temporal_intent::TemporalEntry,
};
use andromeda_postgresql_seaquery::{branch::qualified, clock::now, pg_array::has_elements, unnest::unnest_of};
use sea_query::{
    Cond,
    DynIden,
    Expr,
    ExprTrait,
    Func,
    InsertStatement,
    OnConflict,
    Order,
    Query,
    SelectStatement,
    WindowStatement,
    error::Error as SeaQueryError,
};

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
/// when it is missing and given the types of the latest such intent.
///
/// Clause 4.5.6 makes the Temporal Evolution "the Entity [...] as mandated by
/// clause 4.5.1", so an intent stating types brings it into being; one stating
/// none (every deletion among them) only extends one that exists. A source can
/// hold several intents of one Entity, and a conflict action may change a row
/// once per statement, so one row per Entity is inserted: the types of its
/// latest intent, its first intent's instant as `created_at` and its latest's
/// as `modified_at`, which is what applying them one after the other leaves.
/// `created_at` is never moved (clause 4.8).
pub(crate) fn evolutions_statement(source: &IntentSource<'_>, schema: Option<&DynIden>) -> Result<InsertStatement, SeaQueryError> {
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
        );
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

/// The stream of every attribute grain the entries extend, created when it
/// is missing.
///
/// An entry of an Entity with no Temporal Evolution reaches no stream: the
/// join to `temporal_entities` drops it, which is how the end of an instance
/// of an evolution clause 5.6.16 already deleted records nothing.
pub(crate) fn streams_statement(source: &IntentSource<'_>, schema: Option<&DynIden>) -> Result<InsertStatement, SeaQueryError> {
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
