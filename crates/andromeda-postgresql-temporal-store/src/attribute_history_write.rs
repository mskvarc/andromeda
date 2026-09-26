//! Bulk inserts into the rebuilt `attribute_history` hypertable.

use crate::{
    attribute_history_row::AttributeHistoryRow,
    bulk_write_mode::BulkWriteMode,
    copy::insert_history_batch_via_copy_in_tx,
    error::TemporalStoreError,
    repo::TemporalRepository,
    stored_instance_columns::{generated_instance_id, verbatim_instance_id},
    verbatim_instance_claim::{claim_verbatim_instances, run_history_dedup_deletes, verbatim_instance_targets},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, stream_key::StreamKey, value_kind::ValueKind};
use andromeda_postgresql_encoding::ewkb_binding::geometry_bytes;
use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::{
    pg_type::PostgresType,
    placeholder::external_placeholder,
    postgis::validated_geom_from_ewkb,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use andromeda_telemetry::statement::{StatementName, observe_statement};
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use sea_query::{CaseStatement, Expr, ExprTrait, Iden, Keyword, OnConflict, PostgresQueryBuilder, Query, SimpleExpr};
use serde_json::Value;
use sqlx::{AssertSqlSafe, Postgres, Transaction};
use uuid::Uuid;

/// The statement name the history batch insert is observed under, in either
/// bulk-write mode.
const HISTORY_INSERT: StatementName = StatementName("attribute-history-insert");

/// The `UNNEST` staging alias, `d`.
#[derive(Iden, Clone, Copy)]
#[iden = "d"]
struct D;

/// The staging columns of an attribute-history row, named as the `AS d(…)` alias
/// spells them rather than as the `attribute_history` columns they land in.
#[derive(Iden, Clone, Copy)]
enum HistoryStaged {
    /// Write clock of the row.
    RecordedAt,
    /// Atomic write the row belongs to.
    WriteId,
    /// Stream the row extends.
    StreamKey,
    /// Position of the row within its atomic write.
    WriteOrdinal,
    /// Attribute family the row was written as.
    AttributeKind,
    /// Stored value shape of the row.
    ValueKind,
    /// Client-declared observation instant.
    ObservedAt,
    /// Instant the instance was first recorded at.
    CreatedAt,
    /// Instant the instance was last altered at.
    ModifiedAt,
    /// Deletion instant, present only on a tombstone.
    DeletedAt,
    /// Timestamp value.
    ValueDateTime,
    /// Broker-minted instance identifier.
    GeneratedInstanceId,
    /// Catalog identity of the instance's unit.
    UnitCodeId,
    /// Boolean value.
    ValueBoolean,
    /// Instance identifier stored exactly as received.
    VerbatimInstanceId,
    /// Arbitrary-precision value bound directly as `NUMERIC`.
    ValueNumber,
    /// String value.
    ValueString,
    /// Geometry value as EWKB.
    ValueGeometry,
    /// Structured value.
    ValueObject,
    /// URI value.
    ValueUri,
    /// Retained expanded members.
    Metadata,
}

impl TemporalRepository {
    /// Bulk-insert attribute-history rows in the caller's transaction.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Validation`] when a row's position within
    /// its write exceeds the stored column's width or a geometry cannot be
    /// encoded, and [`TemporalStoreError::Database`] when a statement fails.
    pub async fn insert_history_batch_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        rows: &[AttributeHistoryRow],
    ) -> Result<(), TemporalStoreError> {
        if rows.is_empty() {
            return Ok(());
        }
        let targets = verbatim_instance_targets(rows);
        run_history_dedup_deletes(tx, target, &targets).await?;
        claim_verbatim_instances(tx, target, &targets).await?;
        match self.bulk_write_mode {
            BulkWriteMode::Unnest => observe_statement(HISTORY_INSERT, insert_history_batch_unnest_in_tx(tx, target, rows)).await?,
            BulkWriteMode::CopyStaging => observe_statement(HISTORY_INSERT, insert_history_batch_via_copy_in_tx(tx, target, rows)).await?,
        }
        Ok(())
    }
}

/// Column-major binds for one attribute-history batch.
struct AttributeHistoryBinds<'a> {
    /// Write clock of each row, and the hypertable's partition column.
    recorded_ats: Vec<DateTime<Utc>>,
    /// Atomic write each row belongs to.
    write_ids: Vec<Uuid>,
    /// Stream each row extends.
    stream_keys: Vec<i64>,
    /// Position of each row within its atomic write.
    write_ordinals: Vec<i32>,
    /// Attribute family each row was written as.
    attribute_kinds: Vec<i16>,
    /// Stored value shape of each row.
    value_kinds: Vec<i16>,
    /// Client-declared observation instants.
    observed_ats: Vec<Option<DateTime<Utc>>>,
    /// Instants the instances were first recorded at.
    created_ats: Vec<Option<DateTime<Utc>>>,
    /// Instants the instances were last altered at.
    modified_ats: Vec<Option<DateTime<Utc>>>,
    /// Deletion instants, present only on tombstones.
    deleted_ats: Vec<Option<DateTime<Utc>>>,
    /// Timestamp values.
    value_date_times: Vec<Option<DateTime<Utc>>>,
    /// Broker-minted instance identifiers.
    generated_instance_ids: Vec<Option<Uuid>>,
    /// Catalog identities of the instances' units.
    unit_code_ids: Vec<Option<i16>>,
    /// Boolean values.
    value_booleans: Vec<Option<bool>>,
    /// Instance identifiers stored exactly as received.
    verbatim_instance_ids: Vec<Option<&'a str>>,
    /// Arbitrary-precision values bound directly as `numeric[]`.
    value_numbers: Vec<Option<BigDecimal>>,
    /// String values.
    value_strings: Vec<Option<&'a str>>,
    /// Geometry values as EWKB, projected server-side.
    value_geometries: Vec<Option<Vec<u8>>>,
    /// Structured values.
    value_objects: Vec<Option<&'a Value>>,
    /// URI values, which is how a relationship object is stored.
    value_uris: Vec<Option<&'a str>>,
    /// Retained expanded members.
    metadatas: Vec<Option<&'a Value>>,
}

/// Spread one attribute-history batch into its column-major binds.
fn attribute_history_binds(rows: &[AttributeHistoryRow]) -> Result<AttributeHistoryBinds<'_>, TemporalStoreError> {
    let mut binds = AttributeHistoryBinds {
        recorded_ats: Vec::with_capacity(rows.len()),
        write_ids: Vec::with_capacity(rows.len()),
        stream_keys: Vec::with_capacity(rows.len()),
        write_ordinals: Vec::with_capacity(rows.len()),
        attribute_kinds: Vec::with_capacity(rows.len()),
        value_kinds: Vec::with_capacity(rows.len()),
        observed_ats: Vec::with_capacity(rows.len()),
        created_ats: Vec::with_capacity(rows.len()),
        modified_ats: Vec::with_capacity(rows.len()),
        deleted_ats: Vec::with_capacity(rows.len()),
        value_date_times: Vec::with_capacity(rows.len()),
        generated_instance_ids: Vec::with_capacity(rows.len()),
        unit_code_ids: Vec::with_capacity(rows.len()),
        value_booleans: Vec::with_capacity(rows.len()),
        verbatim_instance_ids: Vec::with_capacity(rows.len()),
        value_numbers: Vec::with_capacity(rows.len()),
        value_strings: Vec::with_capacity(rows.len()),
        value_geometries: Vec::with_capacity(rows.len()),
        value_objects: Vec::with_capacity(rows.len()),
        value_uris: Vec::with_capacity(rows.len()),
        metadatas: Vec::with_capacity(rows.len()),
    };
    for row in rows {
        binds.recorded_ats.push(row.recorded_at);
        binds.write_ids.push(row.write_id());
        binds.stream_keys.push(StreamKey::get(row.stream_key));
        binds.write_ordinals.push(row.write_ordinal()?);
        binds.attribute_kinds.push(AttributeKind::code(row.attribute_kind));
        binds.value_kinds.push(ValueKind::code(row.value.kind()));
        binds.observed_ats.push(row.axes.observed_at);
        binds.created_ats.push(row.axes.created_at);
        binds.modified_ats.push(row.axes.modified_at);
        binds.deleted_ats.push(row.value.deleted_at());
        binds.value_date_times.push(row.value.date_time());
        binds.generated_instance_ids.push(generated_instance_id(&row.instance));
        binds.unit_code_ids.push(row.unit_code_id.map(UnitCodeId::get));
        binds.value_booleans.push(row.value.boolean());
        binds.verbatim_instance_ids.push(verbatim_instance_id(&row.instance));
        // SQLx owns the array values. Clone the decimal itself so the bind is
        // exact and never passes through text or binary64.
        binds.value_numbers.push(row.value.number().cloned());
        binds.value_strings.push(row.value.string());
        binds.value_geometries.push(geometry_bytes(row.value.geometry())?);
        binds.value_objects.push(row.value.object());
        binds.value_uris.push(row.value.uri().map(Uri::as_str));
        binds.metadatas.push(row.metadata.as_ref());
    }
    Ok(binds)
}

/// The twenty-one staging columns of an attribute-history row, in bind order.
fn history_staging_columns() -> [UnnestColumn; 21] {
    [
        UnnestColumn::new(HistoryStaged::RecordedAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(HistoryStaged::WriteId, PostgresType::UuidArray),
        UnnestColumn::new(HistoryStaged::StreamKey, PostgresType::BigintArray),
        UnnestColumn::new(HistoryStaged::WriteOrdinal, PostgresType::IntegerArray),
        UnnestColumn::new(HistoryStaged::AttributeKind, PostgresType::SmallintArray),
        UnnestColumn::new(HistoryStaged::ValueKind, PostgresType::SmallintArray),
        UnnestColumn::new(HistoryStaged::ObservedAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(HistoryStaged::CreatedAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(HistoryStaged::ModifiedAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(HistoryStaged::DeletedAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(HistoryStaged::ValueDateTime, PostgresType::TimestamptzArray),
        UnnestColumn::new(HistoryStaged::GeneratedInstanceId, PostgresType::UuidArray),
        UnnestColumn::new(HistoryStaged::UnitCodeId, PostgresType::SmallintArray),
        UnnestColumn::new(HistoryStaged::ValueBoolean, PostgresType::BooleanArray),
        UnnestColumn::new(HistoryStaged::VerbatimInstanceId, PostgresType::TextArray),
        UnnestColumn::new(HistoryStaged::ValueNumber, PostgresType::NumericArray),
        UnnestColumn::new(HistoryStaged::ValueString, PostgresType::TextArray),
        UnnestColumn::new(HistoryStaged::ValueGeometry, PostgresType::ByteaArray),
        UnnestColumn::new(HistoryStaged::ValueObject, PostgresType::JsonbArray),
        UnnestColumn::new(HistoryStaged::ValueUri, PostgresType::TextArray),
        UnnestColumn::new(HistoryStaged::Metadata, PostgresType::JsonbArray),
    ]
}

/// A staging column reference, `d.<column>`.
fn staged(column: HistoryStaged) -> SimpleExpr {
    Expr::col((D, column))
}

/// The bulk attribute-history insert.
///
/// The value number reaches the arbitrary-precision column through a native
/// `numeric[]` bind, never widened to a double; the geometry is
/// validated from its embedded-SRID EWKB, with a NULL EWKB projecting to a NULL
/// geometry. The conflict target names the columns `uq_attribute_history_write_identity`
/// covers, so a row replayed under its original write identity is a no-op.
fn attribute_history_insert_sql(target: &ContextTarget) -> Result<String, TemporalStoreError> {
    let columns = history_staging_columns();
    let call = unnest_call(columns.iter().enumerate().map(|(index, column)| (column, external_placeholder(index + 1))));

    let geometry: SimpleExpr = CaseStatement::new()
        .case(
            Expr::col((D, HistoryStaged::ValueGeometry)).is_not_null(),
            validated_geom_from_ewkb(staged(HistoryStaged::ValueGeometry)),
        )
        .finally(Keyword::Null)
        .into();

    let mut select = Query::select();
    select
        .expr(staged(HistoryStaged::RecordedAt))
        .expr(staged(HistoryStaged::WriteId))
        .expr(staged(HistoryStaged::StreamKey))
        .expr(staged(HistoryStaged::WriteOrdinal))
        .expr(staged(HistoryStaged::AttributeKind))
        .expr(staged(HistoryStaged::ValueKind))
        .expr(staged(HistoryStaged::ObservedAt))
        .expr(staged(HistoryStaged::CreatedAt))
        .expr(staged(HistoryStaged::ModifiedAt))
        .expr(staged(HistoryStaged::DeletedAt))
        .expr(staged(HistoryStaged::ValueDateTime))
        .expr(staged(HistoryStaged::GeneratedInstanceId))
        .expr(staged(HistoryStaged::UnitCodeId))
        .expr(staged(HistoryStaged::ValueBoolean))
        .expr(staged(HistoryStaged::VerbatimInstanceId))
        .expr(staged(HistoryStaged::ValueNumber))
        .expr(staged(HistoryStaged::ValueString))
        .expr(geometry)
        .expr(staged(HistoryStaged::ValueObject))
        .expr(staged(HistoryStaged::ValueUri))
        .expr(staged(HistoryStaged::Metadata))
        .from_function(call, D);
    let select = select;

    let mut insert = Query::insert();
    insert.into_table(table_of(target, AttributeHistory::Table)).columns([
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
        AttributeHistory::GeneratedInstanceId,
        AttributeHistory::UnitCodeId,
        AttributeHistory::ValueBoolean,
        AttributeHistory::VerbatimInstanceId,
        AttributeHistory::ValueNumber,
        AttributeHistory::ValueString,
        AttributeHistory::ValueGeometry,
        AttributeHistory::ValueObject,
        AttributeHistory::ValueUri,
        AttributeHistory::Metadata,
    ]);
    insert
        .select_from(select)
        .map_err(|error| TemporalStoreError::Validation(error.to_string()))?
        .on_conflict(
            OnConflict::columns([AttributeHistory::RecordedAt, AttributeHistory::WriteId, AttributeHistory::WriteOrdinal])
                .do_nothing()
                .to_owned(),
        );

    let sql = insert.to_string(PostgresQueryBuilder);
    Ok(apply_column_alias(sql, &D, &columns))
}

/// Insert one batch by binding parallel arrays and expanding them with `UNNEST`.
async fn insert_history_batch_unnest_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    rows: &[AttributeHistoryRow],
) -> Result<(), TemporalStoreError> {
    let binds = attribute_history_binds(rows)?;
    let sql = attribute_history_insert_sql(target)?;
    sqlx::query(AssertSqlSafe(sql))
        .bind(&binds.recorded_ats)
        .bind(&binds.write_ids)
        .bind(&binds.stream_keys)
        .bind(&binds.write_ordinals)
        .bind(&binds.attribute_kinds)
        .bind(&binds.value_kinds)
        .bind(&binds.observed_ats)
        .bind(&binds.created_ats)
        .bind(&binds.modified_ats)
        .bind(&binds.deleted_ats)
        .bind(&binds.value_date_times)
        .bind(&binds.generated_instance_ids)
        .bind(&binds.unit_code_ids)
        .bind(&binds.value_booleans)
        .bind(&binds.verbatim_instance_ids)
        .bind(&binds.value_numbers)
        .bind(&binds.value_strings)
        .bind(&binds.value_geometries)
        .bind(&binds.value_objects)
        .bind(&binds.value_uris)
        .bind(&binds.metadatas)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::attribute_history_write::attribute_history_insert_sql;
    use andromeda_context_space::context_target::ContextTarget;

    #[test]
    fn the_insert_carries_the_partition_column_and_the_whole_write_identity() {
        let sql = attribute_history_insert_sql(&ContextTarget::Live).expect("the fixed attribute-history insert builds");
        assert!(sql.contains(r#"("recorded_at", "write_id", "stream_key", "write_ordinal""#), "{sql}");
    }

    #[test]
    fn a_row_replayed_under_its_original_write_identity_is_not_duplicated() {
        let sql = attribute_history_insert_sql(&ContextTarget::Live).expect("the fixed attribute-history insert builds");
        assert!(sql.contains(r#"ON CONFLICT ("recorded_at", "write_id", "write_ordinal") DO NOTHING"#), "{sql}");
    }

    #[test]
    fn the_numeric_value_reaches_storage_through_a_numeric_array_bind() {
        let sql = attribute_history_insert_sql(&ContextTarget::Live).expect("the fixed attribute-history insert builds");
        assert!(sql.contains(r"$16::numeric[]"), "{sql}");
        assert!(!sql.contains(r#""d"."value_number"::numeric"#), "{sql}");
        assert!(!sql.contains("double precision"), "{sql}");
    }
}
