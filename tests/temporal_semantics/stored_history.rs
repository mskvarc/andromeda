//! The stored temporal rows one verification test reads back from the tenant.
//!
//! Most temporal guarantees are properties of storage — which column a
//! timestamp landed in, which of the two instance-identity columns is
//! populated, whether a tombstone carries a typed value — so the suite reads
//! the rows themselves rather than inferring them from a rendering that could
//! agree with the specification while the stored row does not.

use andromeda_postgresql_schema::{
    current::{CurrentAttributes, Entities},
    name_dictionary::AttributeNames,
    temporal::{AttributeHistory, ScopeHistory, TemporalAttributeStreams},
};
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    cast::cast,
    catalog_function::num_nonnulls,
    pg_type::PostgresType,
    scalar_subquery::scalar_subquery,
    statement::Statement,
};
use chrono::{DateTime, Utc};
use sea_query::{Alias, Expr, ExprTrait, Order, Query, SelectStatement};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// One `attribute_history` row, with the catalogue names its keys resolve to.
#[derive(Clone, Debug)]
pub struct StoredHistoryRow {
    /// Expanded attribute name the row's stream is keyed by.
    pub attribute_name: String,
    /// Stream the row belongs to.
    pub stream_key: i64,
    /// `datasetId` of the stream, absent for the default instance.
    pub dataset_id: Option<String>,
    /// Migration-owned attribute-family code carried on the row.
    pub attribute_kind: i16,
    /// Migration-owned value-shape code, `0` for a tombstone.
    pub value_kind: i16,
    /// Operational write clock and hypertable partition column.
    pub recorded_at: DateTime<Utc>,
    /// Identity of the atomic write the row belongs to.
    pub write_id: Uuid,
    /// Position of the row inside its write.
    pub write_ordinal: i32,
    /// `observedAt` axis, present only when the client declared one.
    pub observed_at: Option<DateTime<Utc>>,
    /// `createdAt` axis.
    pub created_at: Option<DateTime<Utc>>,
    /// `modifiedAt` axis.
    pub modified_at: Option<DateTime<Utc>>,
    /// `deletedAt` axis, present exactly on a tombstone.
    pub deleted_at: Option<DateTime<Utc>>,
    /// Broker-minted instance identifier, when the row carries one.
    pub generated_instance_id: Option<Uuid>,
    /// Client-supplied instance identifier, kept byte for byte.
    pub verbatim_instance_id: Option<String>,
    /// Exact decimal spelling of the numeric value column.
    pub value_number: Option<String>,
    /// String value column.
    pub value_string: Option<String>,
    /// URI value column, which a Relationship target occupies.
    pub value_uri: Option<String>,
    /// How many typed value columns the row populates.
    pub populated_value_columns: i32,
}

/// Read every history row recorded for one Entity, in write order.
pub async fn stored_history(pool: &PgPool, entity_id: &str) -> Vec<StoredHistoryRow> {
    stored_history_statement(entity_id)
        .to_query()
        .fetch_all(pool)
        .await
        .expect("stored attribute history")
        .into_iter()
        .map(|row| StoredHistoryRow {
            attribute_name: row.try_get("attribute_name").expect("the attribute_name column decodes"),
            stream_key: row.try_get("stream_key").expect("the stream_key column decodes"),
            dataset_id: row.try_get("dataset_id").expect("the dataset_id column decodes"),
            attribute_kind: row.try_get("attribute_kind").expect("the attribute_kind column decodes"),
            value_kind: row.try_get("value_kind").expect("the value_kind column decodes"),
            recorded_at: row.try_get("recorded_at").expect("the recorded_at column decodes"),
            write_id: row.try_get("write_id").expect("the write_id column decodes"),
            write_ordinal: row.try_get("write_ordinal").expect("the write_ordinal column decodes"),
            observed_at: row.try_get("observed_at").expect("the observed_at column decodes"),
            created_at: row.try_get("created_at").expect("the created_at column decodes"),
            modified_at: row.try_get("modified_at").expect("the modified_at column decodes"),
            deleted_at: row.try_get("deleted_at").expect("the deleted_at column decodes"),
            generated_instance_id: row.try_get("generated_instance_id").expect("the generated_instance_id column decodes"),
            verbatim_instance_id: row.try_get("verbatim_instance_id").expect("the verbatim_instance_id column decodes"),
            value_number: row.try_get("value_number").expect("the value_number column decodes"),
            value_string: row.try_get("value_string").expect("the value_string column decodes"),
            value_uri: row.try_get("value_uri").expect("the value_uri column decodes"),
            populated_value_columns: row.try_get("populated_value_columns").expect("the populated_value_columns column decodes"),
        })
        .collect()
}

/// Read the rows of one attribute of one Entity, in write order.
pub async fn stored_attribute_history(pool: &PgPool, entity_id: &str, attribute_name: &str) -> Vec<StoredHistoryRow> {
    stored_history(pool, entity_id)
        .await
        .into_iter()
        .filter(|row| row.attribute_name == attribute_name)
        .collect()
}

/// Count the current projection rows of one Entity.
pub async fn current_projection_rows(pool: &PgPool, entity_id: &str) -> i64 {
    Query::select()
        .expr_as(count_all(), Alias::new("rows"))
        .from(CurrentAttributes::Table)
        .inner_join(
            Entities::Table,
            Expr::col((Entities::Table, Entities::EntityKey)).equals((CurrentAttributes::Table, CurrentAttributes::EntityKey)),
        )
        .and_where(Expr::col((Entities::Table, Entities::Id)).eq(entity_id))
        .to_query()
        .fetch_one(pool)
        .await
        .expect("current projection count")
        .try_get("rows")
        .expect("the rows column decodes")
}

/// Count the temporal catalogue rows of one Entity.
pub async fn temporal_catalog_rows(pool: &PgPool, entity_id: &str) -> (i64, i64) {
    let row = Query::select()
        .expr_as(
            scalar_subquery(
                Query::select()
                    .expr(count_all())
                    .from(Entities::Table)
                    .and_where(Expr::col(Entities::Id).eq(entity_id))
                    .to_owned(),
            ),
            Alias::new("entities"),
        )
        .expr_as(
            scalar_subquery(
                Query::select()
                    .expr(count_all())
                    .from(TemporalAttributeStreams::Table)
                    .inner_join(
                        Entities::Table,
                        Expr::col((Entities::Table, Entities::EntityKey)).equals((TemporalAttributeStreams::Table, TemporalAttributeStreams::EntityKey)),
                    )
                    .and_where(Expr::col((Entities::Table, Entities::Id)).eq(entity_id))
                    .to_owned(),
            ),
            Alias::new("streams"),
        )
        .to_query()
        .fetch_one(pool)
        .await
        .expect("temporal catalogue counts");
    (
        row.try_get("entities").expect("the entities column decodes"),
        row.try_get("streams").expect("the streams column decodes"),
    )
}

/// Count the temporal rows whose owning catalogue row no longer exists.
///
/// The hypertables carry no foreign key back to the catalogue, so this audit is
/// what stands in for one: every count is expected to be zero.
pub async fn orphaned_temporal_rows(pool: &PgPool) -> (i64, i64, i64) {
    let row = orphaned_temporal_rows_statement()
        .to_query()
        .fetch_one(pool)
        .await
        .expect("temporal orphan audit");
    (
        row.try_get("orphaned_attribute_history_rows")
            .expect("the orphaned_attribute_history_rows column decodes"),
        row.try_get("orphaned_scope_history_rows")
            .expect("the orphaned_scope_history_rows column decodes"),
        row.try_get("orphaned_streams").expect("the orphaned_streams column decodes"),
    )
}

/// The history rows of one Entity with the names their keys resolve to, in
/// write order.
fn stored_history_statement(entity_id: &str) -> SelectStatement {
    let history = |column: AttributeHistory| Expr::col((AttributeHistory::Table, column));
    Query::select()
        .expr_as(Expr::col((AttributeNames::Table, AttributeNames::Name)), Alias::new("attribute_name"))
        .expr_as(
            Expr::col((TemporalAttributeStreams::Table, TemporalAttributeStreams::StreamKey)),
            Alias::new("stream_key"),
        )
        .column((TemporalAttributeStreams::Table, TemporalAttributeStreams::DatasetId))
        .column((AttributeHistory::Table, AttributeHistory::AttributeKind))
        .column((AttributeHistory::Table, AttributeHistory::ValueKind))
        .column((AttributeHistory::Table, AttributeHistory::RecordedAt))
        .column((AttributeHistory::Table, AttributeHistory::WriteId))
        .column((AttributeHistory::Table, AttributeHistory::WriteOrdinal))
        .column((AttributeHistory::Table, AttributeHistory::ObservedAt))
        .column((AttributeHistory::Table, AttributeHistory::CreatedAt))
        .column((AttributeHistory::Table, AttributeHistory::ModifiedAt))
        .column((AttributeHistory::Table, AttributeHistory::DeletedAt))
        .column((AttributeHistory::Table, AttributeHistory::GeneratedInstanceId))
        .column((AttributeHistory::Table, AttributeHistory::VerbatimInstanceId))
        .expr_as(cast(history(AttributeHistory::ValueNumber), PostgresType::Text), Alias::new("value_number"))
        .column((AttributeHistory::Table, AttributeHistory::ValueString))
        .column((AttributeHistory::Table, AttributeHistory::ValueUri))
        .expr_as(
            cast(
                num_nonnulls([
                    history(AttributeHistory::ValueNumber),
                    history(AttributeHistory::ValueString),
                    history(AttributeHistory::ValueBoolean),
                    history(AttributeHistory::ValueGeometry),
                    history(AttributeHistory::ValueObject),
                    history(AttributeHistory::ValueDateTime),
                    history(AttributeHistory::ValueUri),
                ]),
                PostgresType::Integer,
            ),
            Alias::new("populated_value_columns"),
        )
        .from(AttributeHistory::Table)
        .inner_join(
            TemporalAttributeStreams::Table,
            Expr::col((TemporalAttributeStreams::Table, TemporalAttributeStreams::StreamKey)).equals((AttributeHistory::Table, AttributeHistory::StreamKey)),
        )
        .inner_join(
            Entities::Table,
            Expr::col((Entities::Table, Entities::EntityKey)).equals((TemporalAttributeStreams::Table, TemporalAttributeStreams::EntityKey)),
        )
        .inner_join(
            AttributeNames::Table,
            Expr::col((AttributeNames::Table, AttributeNames::AttributeNameKey))
                .equals((TemporalAttributeStreams::Table, TemporalAttributeStreams::AttributeNameKey)),
        )
        .and_where(Expr::col((Entities::Table, Entities::Id)).eq(entity_id))
        .order_by((AttributeHistory::Table, AttributeHistory::RecordedAt), Order::Asc)
        .order_by((AttributeHistory::Table, AttributeHistory::WriteId), Order::Asc)
        .order_by((AttributeHistory::Table, AttributeHistory::WriteOrdinal), Order::Asc)
        .to_owned()
}

/// Three counts: history rows without a stream, scope rows without a temporal
/// Entity, streams without a temporal Entity.
fn orphaned_temporal_rows_statement() -> SelectStatement {
    Query::select()
        .expr_as(
            scalar_subquery(
                Query::select()
                    .expr(count_all())
                    .from(AttributeHistory::Table)
                    .left_join(
                        TemporalAttributeStreams::Table,
                        Expr::col((TemporalAttributeStreams::Table, TemporalAttributeStreams::StreamKey))
                            .equals((AttributeHistory::Table, AttributeHistory::StreamKey)),
                    )
                    .and_where(Expr::col((TemporalAttributeStreams::Table, TemporalAttributeStreams::StreamKey)).is_null())
                    .to_owned(),
            ),
            Alias::new("orphaned_attribute_history_rows"),
        )
        .expr_as(
            scalar_subquery(
                Query::select()
                    .expr(count_all())
                    .from(ScopeHistory::Table)
                    .left_join(
                        Entities::Table,
                        Expr::col((Entities::Table, Entities::EntityKey)).equals((ScopeHistory::Table, ScopeHistory::EntityKey)),
                    )
                    .and_where(Expr::col((Entities::Table, Entities::EntityKey)).is_null())
                    .to_owned(),
            ),
            Alias::new("orphaned_scope_history_rows"),
        )
        .expr_as(
            scalar_subquery(
                Query::select()
                    .expr(count_all())
                    .from(TemporalAttributeStreams::Table)
                    .left_join(
                        Entities::Table,
                        Expr::col((Entities::Table, Entities::EntityKey)).equals((TemporalAttributeStreams::Table, TemporalAttributeStreams::EntityKey)),
                    )
                    .and_where(Expr::col((Entities::Table, Entities::EntityKey)).is_null())
                    .to_owned(),
            ),
            Alias::new("orphaned_streams"),
        )
        .to_owned()
}
