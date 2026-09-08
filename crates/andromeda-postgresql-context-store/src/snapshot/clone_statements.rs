//! `SeaQuery` statements that copy the branch-owned current and temporal relations.

use crate::snapshot::persistence_error;
use andromeda_postgresql_schema::{
    current::{CurrentAttributeValues, Entities, EntityDocuments, EntitySearch},
    temporal::{AttributeHistory, AttributeHistoryInstanceClaims, ScopeHistory, TemporalAttributeStreams, TemporalEntities},
};
use andromeda_postgresql_seaquery::pg_operator::PgBinaryOperator;
use andromeda_snapshot::error::SnapshotError;
use sea_query::{Expr, ExprTrait, Iden, InsertStatement, JoinType, Query, SimpleExpr};

#[derive(Clone, Copy, Iden)]
#[iden = "source"]
/// Source current or temporal catalogue alias.
struct Source;

#[derive(Clone, Copy, Iden)]
#[iden = "target"]
/// Target current or temporal catalogue alias.
struct Target;

#[derive(Clone, Copy, Iden)]
#[iden = "document"]
/// Source entity-document alias.
struct Document;

#[derive(Clone, Copy, Iden)]
#[iden = "search"]
/// Source entity-search alias.
struct Search;

#[derive(Clone, Copy, Iden)]
#[iden = "value"]
/// Source current-attribute-value alias.
struct ValueRow;

#[derive(Clone, Copy, Iden)]
#[iden = "stream"]
/// Source temporal-attribute-stream alias.
struct Stream;

#[derive(Clone, Copy, Iden)]
#[iden = "history"]
/// Source history-row alias.
struct History;

#[derive(Clone, Copy, Iden)]
#[iden = "source_stream"]
/// Source stream alias used while remapping history keys.
struct SourceStream;

#[derive(Clone, Copy, Iden)]
#[iden = "source_entity"]
/// Source temporal-entity alias used while remapping history keys.
struct SourceEntity;

#[derive(Clone, Copy, Iden)]
#[iden = "target_entity"]
/// Target temporal-entity alias used while remapping history keys.
struct TargetEntity;

#[derive(Clone, Copy, Iden)]
#[iden = "target_stream"]
/// Target stream alias used while remapping history keys.
struct TargetStream;

#[derive(Clone, Copy, Iden)]
#[iden = "claim"]
/// Source verbatim-instance claim alias.
struct Claim;

/// Build every branch copy statement in foreign-key dependency order.
pub(crate) fn clone_statements(source_key: i64, target_key: i64) -> Result<[InsertStatement; 9], SnapshotError> {
    Ok([
        entities(source_key, target_key)?,
        entity_documents(source_key, target_key)?,
        entity_search(source_key, target_key)?,
        current_attribute_values(source_key, target_key)?,
        temporal_entities(source_key, target_key)?,
        temporal_attribute_streams(source_key, target_key)?,
        attribute_history(source_key, target_key)?,
        scope_history(source_key, target_key)?,
        attribute_history_instance_claims(source_key, target_key)?,
    ])
}

/// Copy current Entity catalogue rows into the target branch.
fn entities(source_key: i64, target_key: i64) -> Result<InsertStatement, SnapshotError> {
    let select = Query::select()
        .expr(Expr::val(target_key))
        .columns([Entities::CreatedAt, Entities::ExpiresAt, Entities::MinAttributeExpiresAt, Entities::Id])
        .from(Entities::Table)
        .and_where(Expr::col(Entities::ContextSpaceKey).eq(source_key))
        .to_owned();
    let mut insert = Query::insert();
    insert
        .into_table(Entities::Table)
        .columns([
            Entities::ContextSpaceKey,
            Entities::CreatedAt,
            Entities::ExpiresAt,
            Entities::MinAttributeExpiresAt,
            Entities::Id,
        ])
        .select_from(select)
        .map_err(persistence_error)?;
    Ok(insert.clone())
}

/// Copy Entity documents while remapping their generated entity keys.
fn entity_documents(source_key: i64, target_key: i64) -> Result<InsertStatement, SnapshotError> {
    let select = Query::select()
        .expr(Expr::val(target_key))
        .column((Target, Entities::EntityKey))
        .columns([(Document, EntityDocuments::ModifiedAt), (Document, EntityDocuments::Payload)])
        .from_as(EntityDocuments::Table, Document)
        .join_as(
            JoinType::InnerJoin,
            Entities::Table,
            Source,
            Expr::col((Source, Entities::ContextSpaceKey))
                .eq(source_key)
                .and(Expr::col((Source, Entities::EntityKey)).eq(Expr::col((Document, EntityDocuments::EntityKey)))),
        )
        .join_as(
            JoinType::InnerJoin,
            Entities::Table,
            Target,
            Expr::col((Target, Entities::ContextSpaceKey))
                .eq(target_key)
                .and(Expr::col((Target, Entities::Id)).eq(Expr::col((Source, Entities::Id)))),
        )
        .and_where(Expr::col((Document, EntityDocuments::ContextSpaceKey)).eq(source_key))
        .to_owned();
    let mut insert = Query::insert();
    insert
        .into_table(EntityDocuments::Table)
        .columns([
            EntityDocuments::ContextSpaceKey,
            EntityDocuments::EntityKey,
            EntityDocuments::ModifiedAt,
            EntityDocuments::Payload,
        ])
        .select_from(select)
        .map_err(persistence_error)?;
    Ok(insert.clone())
}

/// Copy Entity search projections while remapping their generated entity keys.
fn entity_search(source_key: i64, target_key: i64) -> Result<InsertStatement, SnapshotError> {
    let select = Query::select()
        .expr(Expr::val(target_key))
        .column((Target, Entities::EntityKey))
        .columns([
            (Search, EntitySearch::LocationExpiresAt),
            (Search, EntitySearch::TypeIds),
            (Search, EntitySearch::AttributeNameIds),
            (Search, EntitySearch::ScopePaths),
            (Search, EntitySearch::Location),
            (Search, EntitySearch::ObservationSpace),
            (Search, EntitySearch::OperationSpace),
        ])
        .from_as(EntitySearch::Table, Search)
        .join_as(
            JoinType::InnerJoin,
            Entities::Table,
            Source,
            Expr::col((Source, Entities::ContextSpaceKey))
                .eq(source_key)
                .and(Expr::col((Source, Entities::EntityKey)).eq(Expr::col((Search, EntitySearch::EntityKey)))),
        )
        .join_as(
            JoinType::InnerJoin,
            Entities::Table,
            Target,
            Expr::col((Target, Entities::ContextSpaceKey))
                .eq(target_key)
                .and(Expr::col((Target, Entities::Id)).eq(Expr::col((Source, Entities::Id)))),
        )
        .and_where(Expr::col((Search, EntitySearch::ContextSpaceKey)).eq(source_key))
        .to_owned();
    let mut insert = Query::insert();
    insert
        .into_table(EntitySearch::Table)
        .columns([
            EntitySearch::ContextSpaceKey,
            EntitySearch::EntityKey,
            EntitySearch::LocationExpiresAt,
            EntitySearch::TypeIds,
            EntitySearch::AttributeNameIds,
            EntitySearch::ScopePaths,
            EntitySearch::Location,
            EntitySearch::ObservationSpace,
            EntitySearch::OperationSpace,
        ])
        .select_from(select)
        .map_err(persistence_error)?;
    Ok(insert.clone())
}

/// Copy typed current values while remapping their generated entity keys.
fn current_attribute_values(source_key: i64, target_key: i64) -> Result<InsertStatement, SnapshotError> {
    let select = Query::select()
        .expr(Expr::val(target_key))
        .column((Target, Entities::EntityKey))
        .columns([
            (ValueRow, CurrentAttributeValues::AttributeNameId),
            (ValueRow, CurrentAttributeValues::AttributeKind),
            (ValueRow, CurrentAttributeValues::ValueKind),
            (ValueRow, CurrentAttributeValues::ValueDatetime),
            (ValueRow, CurrentAttributeValues::UnitCodeId),
            (ValueRow, CurrentAttributeValues::ValueBool),
            (ValueRow, CurrentAttributeValues::DatasetId),
            (ValueRow, CurrentAttributeValues::ValueNumber),
            (ValueRow, CurrentAttributeValues::ValueString),
            (ValueRow, CurrentAttributeValues::ValueGeo),
            (ValueRow, CurrentAttributeValues::ValueObject),
            (ValueRow, CurrentAttributeValues::ValueUri),
        ])
        .from_as(CurrentAttributeValues::Table, ValueRow)
        .join_as(
            JoinType::InnerJoin,
            Entities::Table,
            Source,
            Expr::col((Source, Entities::ContextSpaceKey))
                .eq(source_key)
                .and(Expr::col((Source, Entities::EntityKey)).eq(Expr::col((ValueRow, CurrentAttributeValues::EntityKey)))),
        )
        .join_as(
            JoinType::InnerJoin,
            Entities::Table,
            Target,
            Expr::col((Target, Entities::ContextSpaceKey))
                .eq(target_key)
                .and(Expr::col((Target, Entities::Id)).eq(Expr::col((Source, Entities::Id)))),
        )
        .and_where(Expr::col((ValueRow, CurrentAttributeValues::ContextSpaceKey)).eq(source_key))
        .to_owned();
    let mut insert = Query::insert();
    insert
        .into_table(CurrentAttributeValues::Table)
        .columns([
            CurrentAttributeValues::ContextSpaceKey,
            CurrentAttributeValues::EntityKey,
            CurrentAttributeValues::AttributeNameId,
            CurrentAttributeValues::AttributeKind,
            CurrentAttributeValues::ValueKind,
            CurrentAttributeValues::ValueDatetime,
            CurrentAttributeValues::UnitCodeId,
            CurrentAttributeValues::ValueBool,
            CurrentAttributeValues::DatasetId,
            CurrentAttributeValues::ValueNumber,
            CurrentAttributeValues::ValueString,
            CurrentAttributeValues::ValueGeo,
            CurrentAttributeValues::ValueObject,
            CurrentAttributeValues::ValueUri,
        ])
        .select_from(select)
        .map_err(persistence_error)?;
    Ok(insert.clone())
}

/// Copy temporal Entity catalogue rows into the target branch.
fn temporal_entities(source_key: i64, target_key: i64) -> Result<InsertStatement, SnapshotError> {
    let select = Query::select()
        .expr(Expr::val(target_key))
        .columns([
            TemporalEntities::CreatedAt,
            TemporalEntities::ModifiedAt,
            TemporalEntities::Id,
            TemporalEntities::TypeIds,
        ])
        .from(TemporalEntities::Table)
        .and_where(Expr::col(TemporalEntities::ContextSpaceKey).eq(source_key))
        .to_owned();
    let mut insert = Query::insert();
    insert
        .into_table(TemporalEntities::Table)
        .columns([
            TemporalEntities::ContextSpaceKey,
            TemporalEntities::CreatedAt,
            TemporalEntities::ModifiedAt,
            TemporalEntities::Id,
            TemporalEntities::TypeIds,
        ])
        .select_from(select)
        .map_err(persistence_error)?;
    Ok(insert.clone())
}

/// Copy temporal streams while remapping their generated temporal-entity keys.
fn temporal_attribute_streams(source_key: i64, target_key: i64) -> Result<InsertStatement, SnapshotError> {
    let select = Query::select()
        .expr(Expr::val(target_key))
        .column((Target, TemporalEntities::EntityKey))
        .columns([
            (Stream, TemporalAttributeStreams::AttributeNameId),
            (Stream, TemporalAttributeStreams::DatasetId),
        ])
        .from_as(TemporalAttributeStreams::Table, Stream)
        .join_as(
            JoinType::InnerJoin,
            TemporalEntities::Table,
            Source,
            Expr::col((Source, TemporalEntities::ContextSpaceKey))
                .eq(source_key)
                .and(Expr::col((Source, TemporalEntities::EntityKey)).eq(Expr::col((Stream, TemporalAttributeStreams::TemporalEntityKey)))),
        )
        .join_as(
            JoinType::InnerJoin,
            TemporalEntities::Table,
            Target,
            Expr::col((Target, TemporalEntities::ContextSpaceKey))
                .eq(target_key)
                .and(Expr::col((Target, TemporalEntities::Id)).eq(Expr::col((Source, TemporalEntities::Id)))),
        )
        .and_where(Expr::col((Stream, TemporalAttributeStreams::ContextSpaceKey)).eq(source_key))
        .to_owned();
    let mut insert = Query::insert();
    insert
        .into_table(TemporalAttributeStreams::Table)
        .columns([
            TemporalAttributeStreams::ContextSpaceKey,
            TemporalAttributeStreams::TemporalEntityKey,
            TemporalAttributeStreams::AttributeNameId,
            TemporalAttributeStreams::DatasetId,
        ])
        .select_from(select)
        .map_err(persistence_error)?;
    Ok(insert.clone())
}

/// Copy attribute history while remapping generated stream keys.
fn attribute_history(source_key: i64, target_key: i64) -> Result<InsertStatement, SnapshotError> {
    let select = Query::select()
        .column((History, AttributeHistory::RecordedAt))
        .expr(Expr::val(target_key))
        .column((History, AttributeHistory::WriteId))
        .column((TargetStream, TemporalAttributeStreams::StreamKey))
        .columns([
            (History, AttributeHistory::WriteOrdinal),
            (History, AttributeHistory::AttributeKind),
            (History, AttributeHistory::ValueKind),
            (History, AttributeHistory::ObservedAt),
            (History, AttributeHistory::CreatedAt),
            (History, AttributeHistory::ModifiedAt),
            (History, AttributeHistory::DeletedAt),
            (History, AttributeHistory::ValueDatetime),
            (History, AttributeHistory::GeneratedInstanceUuid),
            (History, AttributeHistory::UnitCodeId),
            (History, AttributeHistory::ValueBool),
            (History, AttributeHistory::VerbatimInstanceId),
            (History, AttributeHistory::ValueNumber),
            (History, AttributeHistory::ValueString),
            (History, AttributeHistory::ValueGeo),
            (History, AttributeHistory::ValueObject),
            (History, AttributeHistory::ValueUri),
            (History, AttributeHistory::Metadata),
        ])
        .from_as(AttributeHistory::Table, History)
        .join_as(
            JoinType::InnerJoin,
            TemporalAttributeStreams::Table,
            SourceStream,
            Expr::col((SourceStream, TemporalAttributeStreams::ContextSpaceKey))
                .eq(source_key)
                .and(Expr::col((SourceStream, TemporalAttributeStreams::StreamKey)).eq(Expr::col((History, AttributeHistory::StreamKey)))),
        )
        .join_as(
            JoinType::InnerJoin,
            TemporalEntities::Table,
            SourceEntity,
            Expr::col((SourceEntity, TemporalEntities::ContextSpaceKey))
                .eq(source_key)
                .and(Expr::col((SourceEntity, TemporalEntities::EntityKey)).eq(Expr::col((SourceStream, TemporalAttributeStreams::TemporalEntityKey)))),
        )
        .join_as(
            JoinType::InnerJoin,
            TemporalEntities::Table,
            TargetEntity,
            Expr::col((TargetEntity, TemporalEntities::ContextSpaceKey))
                .eq(target_key)
                .and(Expr::col((TargetEntity, TemporalEntities::Id)).eq(Expr::col((SourceEntity, TemporalEntities::Id)))),
        )
        .join_as(
            JoinType::InnerJoin,
            TemporalAttributeStreams::Table,
            TargetStream,
            Expr::col((TargetStream, TemporalAttributeStreams::ContextSpaceKey))
                .eq(target_key)
                .and(Expr::col((TargetStream, TemporalAttributeStreams::TemporalEntityKey)).eq(Expr::col((TargetEntity, TemporalEntities::EntityKey))))
                .and(
                    Expr::col((TargetStream, TemporalAttributeStreams::AttributeNameId))
                        .eq(Expr::col((SourceStream, TemporalAttributeStreams::AttributeNameId))),
                )
                .and(
                    SimpleExpr::from(Expr::col((TargetStream, TemporalAttributeStreams::DatasetId)))
                        .not_distinct_from(Expr::col((SourceStream, TemporalAttributeStreams::DatasetId))),
                ),
        )
        .and_where(Expr::col((History, AttributeHistory::ContextSpaceKey)).eq(source_key))
        .to_owned();
    let mut insert = Query::insert();
    insert
        .into_table(AttributeHistory::Table)
        .columns([
            AttributeHistory::RecordedAt,
            AttributeHistory::ContextSpaceKey,
            AttributeHistory::WriteId,
            AttributeHistory::StreamKey,
            AttributeHistory::WriteOrdinal,
            AttributeHistory::AttributeKind,
            AttributeHistory::ValueKind,
            AttributeHistory::ObservedAt,
            AttributeHistory::CreatedAt,
            AttributeHistory::ModifiedAt,
            AttributeHistory::DeletedAt,
            AttributeHistory::ValueDatetime,
            AttributeHistory::GeneratedInstanceUuid,
            AttributeHistory::UnitCodeId,
            AttributeHistory::ValueBool,
            AttributeHistory::VerbatimInstanceId,
            AttributeHistory::ValueNumber,
            AttributeHistory::ValueString,
            AttributeHistory::ValueGeo,
            AttributeHistory::ValueObject,
            AttributeHistory::ValueUri,
            AttributeHistory::Metadata,
        ])
        .select_from(select)
        .map_err(persistence_error)?;
    Ok(insert.clone())
}

/// Copy scope history while remapping generated temporal-entity keys.
fn scope_history(source_key: i64, target_key: i64) -> Result<InsertStatement, SnapshotError> {
    let select = Query::select()
        .column((History, ScopeHistory::RecordedAt))
        .expr(Expr::val(target_key))
        .column((Target, TemporalEntities::EntityKey))
        .columns([
            (History, ScopeHistory::WriteId),
            (History, ScopeHistory::WriteOrdinal),
            (History, ScopeHistory::ValueKind),
            (History, ScopeHistory::ObservedAt),
            (History, ScopeHistory::CreatedAt),
            (History, ScopeHistory::ModifiedAt),
            (History, ScopeHistory::DeletedAt),
            (History, ScopeHistory::GeneratedInstanceUuid),
            (History, ScopeHistory::VerbatimInstanceId),
            (History, ScopeHistory::ScopePath),
        ])
        .from_as(ScopeHistory::Table, History)
        .join_as(
            JoinType::InnerJoin,
            TemporalEntities::Table,
            Source,
            Expr::col((Source, TemporalEntities::ContextSpaceKey))
                .eq(source_key)
                .and(Expr::col((Source, TemporalEntities::EntityKey)).eq(Expr::col((History, ScopeHistory::TemporalEntityKey)))),
        )
        .join_as(
            JoinType::InnerJoin,
            TemporalEntities::Table,
            Target,
            Expr::col((Target, TemporalEntities::ContextSpaceKey))
                .eq(target_key)
                .and(Expr::col((Target, TemporalEntities::Id)).eq(Expr::col((Source, TemporalEntities::Id)))),
        )
        .and_where(Expr::col((History, ScopeHistory::ContextSpaceKey)).eq(source_key))
        .to_owned();
    let mut insert = Query::insert();
    insert
        .into_table(ScopeHistory::Table)
        .columns([
            ScopeHistory::RecordedAt,
            ScopeHistory::ContextSpaceKey,
            ScopeHistory::TemporalEntityKey,
            ScopeHistory::WriteId,
            ScopeHistory::WriteOrdinal,
            ScopeHistory::ValueKind,
            ScopeHistory::ObservedAt,
            ScopeHistory::CreatedAt,
            ScopeHistory::ModifiedAt,
            ScopeHistory::DeletedAt,
            ScopeHistory::GeneratedInstanceUuid,
            ScopeHistory::VerbatimInstanceId,
            ScopeHistory::ScopePath,
        ])
        .select_from(select)
        .map_err(persistence_error)?;
    Ok(insert.clone())
}

/// Copy verbatim-instance uniqueness claims into the target branch.
fn attribute_history_instance_claims(source_key: i64, target_key: i64) -> Result<InsertStatement, SnapshotError> {
    let select = Query::select()
        .expr(Expr::val(target_key))
        .column((Target, TemporalEntities::EntityKey))
        .columns([
            (Claim, AttributeHistoryInstanceClaims::AttributeNameId),
            (Claim, AttributeHistoryInstanceClaims::VerbatimInstanceId),
        ])
        .from_as(AttributeHistoryInstanceClaims::Table, Claim)
        .join_as(
            JoinType::InnerJoin,
            TemporalEntities::Table,
            Source,
            Expr::col((Source, TemporalEntities::ContextSpaceKey))
                .eq(source_key)
                .and(Expr::col((Source, TemporalEntities::EntityKey)).eq(Expr::col((Claim, AttributeHistoryInstanceClaims::TemporalEntityKey)))),
        )
        .join_as(
            JoinType::InnerJoin,
            TemporalEntities::Table,
            Target,
            Expr::col((Target, TemporalEntities::ContextSpaceKey))
                .eq(target_key)
                .and(Expr::col((Target, TemporalEntities::Id)).eq(Expr::col((Source, TemporalEntities::Id)))),
        )
        .and_where(Expr::col((Claim, AttributeHistoryInstanceClaims::ContextSpaceKey)).eq(source_key))
        .to_owned();
    let mut insert = Query::insert();
    insert
        .into_table(AttributeHistoryInstanceClaims::Table)
        .columns([
            AttributeHistoryInstanceClaims::ContextSpaceKey,
            AttributeHistoryInstanceClaims::TemporalEntityKey,
            AttributeHistoryInstanceClaims::AttributeNameId,
            AttributeHistoryInstanceClaims::VerbatimInstanceId,
        ])
        .select_from(select)
        .map_err(persistence_error)?;
    Ok(insert.clone())
}
