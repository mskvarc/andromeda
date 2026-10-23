//! Seeding temporal rows directly, for populations no API request can produce.
//!
//! Two properties under test need storage the broker never writes today:
//! instances whose broker-minted identifiers are migrated version-4 UUIDs, and
//! history old enough for an operator retention horizon to age out. Both are
//! properties of migrated tenants rather than of new writes, so the fixtures
//! are inserted through the tenant connection and then read back through the
//! ordinary paths.

use andromeda_postgresql_schema::{
    current::Entities,
    name_dictionary::{AttributeNames, EntityTypeNames},
    temporal::{AttributeHistory, TemporalAttributeStreams},
};
use andromeda_postgresql_seaquery::{clock::now, pg_array::array_literal, statement::Statement};
use chrono::{DateTime, Utc};
use sea_query::{Alias, CommonTableExpression, Expr, ExprTrait, OnConflict, Query, WithClause};
use sqlx::PgPool;
use uuid::Uuid;

/// The common table expression holding the interned entity type.
const TYPE_NAME: &str = "type_name";

/// One directly seeded `attribute_history` row.
pub struct SeededHistoryRow {
    /// Stream the row belongs to.
    pub stream_key: i64,
    /// Operational write clock, which decides the chunk the row lands in.
    pub recorded_at: DateTime<Utc>,
    /// Identity of the write the row is attributed to.
    pub write_id: Uuid,
    /// Broker-minted instance identifier of the row.
    pub generated_instance_id: Uuid,
    /// Declared observation instant.
    pub observed_at: DateTime<Utc>,
    /// Numeric value the row carries.
    pub value_number: i32,
}

/// Insert a temporal Entity and return its narrow key.
///
/// The type is interned through the dictionary inside the same statement, so
/// the catalogue row carries the key the broker's own write path would have
/// minted for it.
pub async fn seed_temporal_entity(pool: &PgPool, entity_id: &str, entity_type: &str) -> i64 {
    let type_name = Query::insert()
        .into_table(EntityTypeNames::Table)
        .columns([EntityTypeNames::Name])
        .values([Expr::val(entity_type)])
        .expect("one value per column")
        .on_conflict(OnConflict::column(EntityTypeNames::Name).update_column(EntityTypeNames::Name).to_owned())
        .returning_col(EntityTypeNames::EntityTypeNameKey)
        .to_owned();
    let entity = Query::insert()
        .into_table(Entities::Table)
        .columns([
            Entities::EvolutionCreatedAt,
            Entities::EvolutionModifiedAt,
            Entities::Id,
            Entities::EvolutionTypeNameKeys,
        ])
        .select_from(
            Query::select()
                .expr(now())
                .expr(now())
                .expr(Expr::val(entity_id))
                .expr(array_literal([Expr::col((Alias::new(TYPE_NAME), EntityTypeNames::EntityTypeNameKey))]))
                .from(Alias::new(TYPE_NAME))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .returning_col(Entities::EntityKey)
        .to_owned();
    WithClause::new()
        .cte(CommonTableExpression::new().table_name(Alias::new(TYPE_NAME)).query(type_name).to_owned())
        .to_owned()
        .query(entity)
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("seeded temporal entity")
}

/// Resolve, creating if needed, the dictionary identity of one attribute name.
pub async fn attribute_name_key(pool: &PgPool, attribute_name: &str) -> i32 {
    Query::insert()
        .into_table(AttributeNames::Table)
        .columns([AttributeNames::Name])
        .values([Expr::val(attribute_name)])
        .expect("one value per column")
        .on_conflict(OnConflict::column(AttributeNames::Name).update_column(AttributeNames::Name).to_owned())
        .returning_col(AttributeNames::AttributeNameKey)
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("attribute dictionary identity")
}

/// Insert one attribute stream and return the key the database generated for it.
pub async fn seed_stream(pool: &PgPool, temporal_entity_key: i64, attribute_name_key: i32) -> i64 {
    Query::insert()
        .into_table(TemporalAttributeStreams::Table)
        .columns([TemporalAttributeStreams::EntityKey, TemporalAttributeStreams::AttributeNameKey])
        .select_from(
            Query::select()
                .column(Entities::EntityKey)
                .expr(Expr::val(attribute_name_key))
                .from(Entities::Table)
                .and_where(Expr::col(Entities::EntityKey).eq(temporal_entity_key))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .returning_col(TemporalAttributeStreams::StreamKey)
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("seeded attribute stream")
}

/// Insert one history row.
pub async fn seed_history_row(pool: &PgPool, row: &SeededHistoryRow) {
    Query::insert()
        .into_table(AttributeHistory::Table)
        .columns([
            AttributeHistory::RecordedAt,
            AttributeHistory::WriteId,
            AttributeHistory::StreamKey,
            AttributeHistory::WriteOrdinal,
            AttributeHistory::AttributeKind,
            AttributeHistory::ValueKind,
            AttributeHistory::GeneratedInstanceId,
            AttributeHistory::ObservedAt,
            AttributeHistory::CreatedAt,
            AttributeHistory::ModifiedAt,
            AttributeHistory::ValueNumber,
        ])
        .select_from(
            Query::select()
                .expr(Expr::val(row.recorded_at))
                .expr(Expr::val(row.write_id))
                .column(TemporalAttributeStreams::StreamKey)
                .expr(Expr::val(0))
                .expr(Expr::val(1))
                .expr(Expr::val(1))
                .expr(Expr::val(row.generated_instance_id))
                .expr(Expr::val(row.observed_at))
                .expr(Expr::val(row.recorded_at))
                .expr(Expr::val(row.recorded_at))
                .expr(Expr::val(row.value_number))
                .from(TemporalAttributeStreams::Table)
                .and_where(Expr::col(TemporalAttributeStreams::StreamKey).eq(row.stream_key))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_query()
        .execute(pool)
        .await
        .expect("seeded history row");
}
