//! Bulk inserts into the `attribute_history` hypertable.

use crate::{
    attribute_history_binds::{attribute_history_binds, element_trees},
    attribute_history_row::AttributeHistoryRow,
    error::TemporalStoreError,
    repo::TemporalRepository,
    verbatim_instance_claim::{claim_verbatim_instances, run_history_dedup_deletes, verbatim_instance_targets},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_encoding::array_literal::IntegerArrayLiteral;
use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::{
    cast::cast,
    pg_type::PostgresType,
    placeholder::external_placeholder,
    postgis::validated_geom_from_ewkb,
    unnest::{UnnestColumn, apply_column_alias, unnest_call},
};
use andromeda_telemetry::statement::{StatementName, observe_statement};
use sea_query::{CaseStatement, Expr, ExprTrait, Iden, Keyword, OnConflict, PostgresQueryBuilder, Query, SimpleExpr, TableRef};
use sqlx::{AssertSqlSafe, PgConnection, Postgres, Transaction};

/// The statement name the history batch insert is observed under.
const HISTORY_INSERT: StatementName = StatementName("attribute-history-insert");

/// The `UNNEST` staging alias, `d`.
#[derive(Iden, Clone, Copy)]
#[iden = "d"]
struct D;

/// The staging columns of an attribute-history row, named as the `AS d(…)` alias spells them.
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
    /// The `bbox` and foreign members of the geometry value.
    ValueGeometryMembers,
    /// Structured value.
    ValueObject,
    /// URI value.
    ValueUri,
    /// `entity_type_names` key of `valueType`.
    ValueTypeKey,
    /// `entity_type_names` keys of `objectType`, as an array literal.
    ObjectTypeKeys,
    /// The seal members.
    ProofSeals,
    /// `expiresAt`.
    ExpiresAt,
    /// The sub-attribute elements, in their carrier composite.
    SubAttributes,
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
        rows: Vec<AttributeHistoryRow>,
    ) -> Result<(), TemporalStoreError> {
        if rows.is_empty() {
            return Ok(());
        }
        let targets = verbatim_instance_targets(&rows);
        run_history_dedup_deletes(tx, target, &targets).await?;
        claim_verbatim_instances(tx, target, &targets).await?;
        observe_statement(HISTORY_INSERT, insert_history_rows(tx, table_of(target, AttributeHistory::Table), rows)).await
    }
}

/// The twenty-six staging columns of an attribute-history row, in bind order.
fn history_staging_columns() -> [UnnestColumn; 26] {
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
        UnnestColumn::new(HistoryStaged::ValueGeometryMembers, PostgresType::JsonbArray),
        UnnestColumn::new(HistoryStaged::ValueObject, PostgresType::JsonbArray),
        UnnestColumn::new(HistoryStaged::ValueUri, PostgresType::TextArray),
        UnnestColumn::new(HistoryStaged::ValueTypeKey, PostgresType::IntegerArray),
        UnnestColumn::new(HistoryStaged::ObjectTypeKeys, PostgresType::TextArray),
        UnnestColumn::new(HistoryStaged::ProofSeals, PostgresType::JsonbArray),
        UnnestColumn::new(HistoryStaged::ExpiresAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(HistoryStaged::SubAttributes, PostgresType::SubAttributeTreeArray),
    ]
}

/// A staging column reference, `d.<column>`.
fn staged(column: HistoryStaged) -> SimpleExpr {
    Expr::col((D, column))
}

/// The bulk attribute-history insert into `table`.
///
/// The value number reaches the arbitrary-precision column through a native `numeric[]` bind, never widened to a double; the geometry is validated from its embedded-SRID EWKB, with a NULL EWKB projecting to a NULL geometry. The `objectType` keys of each row travel as an array literal, since the rows' lists are ragged, and the sub-attribute elements as one `sub_attribute_tree` per row, which `UNNEST` expands to the row's `sub_attribute[]`. The conflict target names the columns `uq_attribute_history_write_identity` covers, so a row replayed under its original write identity is a no-op.
fn attribute_history_insert_sql(table: TableRef) -> Result<String, TemporalStoreError> {
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
        .expr(staged(HistoryStaged::ValueGeometryMembers))
        .expr(staged(HistoryStaged::ValueObject))
        .expr(staged(HistoryStaged::ValueUri))
        .expr(staged(HistoryStaged::ValueTypeKey))
        .expr(cast(staged(HistoryStaged::ObjectTypeKeys), PostgresType::IntegerArray))
        .expr(staged(HistoryStaged::ProofSeals))
        .expr(staged(HistoryStaged::ExpiresAt))
        .expr(staged(HistoryStaged::SubAttributes))
        .from_function(call, D);
    let select = select;

    let mut insert = Query::insert();
    insert.into_table(table).columns([
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
        AttributeHistory::ValueGeometryMembers,
        AttributeHistory::ValueObject,
        AttributeHistory::ValueUri,
        AttributeHistory::ValueTypeKey,
        AttributeHistory::ObjectTypeKeys,
        AttributeHistory::ProofSeals,
        AttributeHistory::ExpiresAt,
        AttributeHistory::SubAttributes,
    ]);
    insert.select_from(select).map_err(TemporalStoreError::StatementBuild)?.on_conflict(
        OnConflict::columns([AttributeHistory::RecordedAt, AttributeHistory::WriteId, AttributeHistory::WriteOrdinal])
            .do_nothing()
            .to_owned(),
    );

    let sql = insert.to_string(PostgresQueryBuilder);
    Ok(apply_column_alias(sql, &D, &columns))
}

/// Insert one batch into `table` by binding parallel arrays and expanding them
/// with `UNNEST`.
///
/// # Errors
///
/// Returns [`TemporalStoreError`] when a row does not bind or the statement
/// fails.
pub(crate) async fn insert_history_rows(connection: &mut PgConnection, table: TableRef, mut rows: Vec<AttributeHistoryRow>) -> Result<(), TemporalStoreError> {
    let sub_attributes = element_trees(&mut rows)?;
    let binds = attribute_history_binds(&rows)?;
    let object_type_keys: Vec<Option<&str>> = binds
        .object_type_keys
        .iter()
        .map(|keys| keys.as_ref().map(IntegerArrayLiteral::as_str))
        .collect();
    let sql = attribute_history_insert_sql(table)?;
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
        .bind(&binds.value_geometry_members)
        .bind(&binds.value_objects)
        .bind(&binds.value_uris)
        .bind(&binds.value_type_keys)
        .bind(object_type_keys)
        .bind(&binds.proof_seals)
        .bind(&binds.expires_ats)
        .bind(sub_attributes)
        .execute(connection)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_history_binds::element_trees,
        attribute_history_row::AttributeHistoryRow,
        attribute_history_value::AttributeHistoryValue,
        attribute_history_write::attribute_history_insert_sql,
        history_members::HistoryMembers,
        semantic_time_axes::SemanticTimeAxes,
    };
    use andromeda_context_commit::{write_id::WriteId, write_ordinal::WriteOrdinal};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::instance_identity::InstanceIdentity;
    use andromeda_postgresql_attribute_instance::{stored_members::InstanceMembers, stored_value::StoredValue, sub_attribute::SubAttributeElement};
    use andromeda_postgresql_branch::table_of;
    use andromeda_postgresql_codes::{attribute_kind::AttributeKind, stream_key::StreamKey};
    use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
    use andromeda_postgresql_schema::temporal::AttributeHistory;
    use andromeda_temporal::append_intent::TemporalObservationIdentity;
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    /// A row of `speed` at `ordinal` whose members hold `sub_attributes`.
    fn row(ordinal: u32, sub_attributes: Vec<SubAttributeElement>) -> AttributeHistoryRow {
        AttributeHistoryRow {
            recorded_at: Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant"),
            identity: TemporalObservationIdentity {
                write_id: WriteId::from_uuid(Uuid::nil()),
                ordinal: WriteOrdinal::at(ordinal),
            },
            stream_key: StreamKey::try_from(1).expect("positive"),
            attribute_kind: AttributeKind::Property,
            value: AttributeHistoryValue::Boolean(true),
            axes: SemanticTimeAxes::default(),
            instance: InstanceIdentity::Generated(Uuid::nil()),
            members: HistoryMembers {
                sub_attributes,
                ..HistoryMembers::default()
            },
        }
    }

    /// A Boolean sub-attribute element under the instance itself.
    fn element() -> SubAttributeElement {
        SubAttributeElement {
            parent: 0,
            members: InstanceMembers {
                attribute_name_key: AttributeNameKey::from(3),
                dataset_id: None,
                attribute_kind: AttributeKind::Property,
                value: StoredValue::Boolean { flag: true, datatype: None },
                value_type_key: None,
                object_type_keys: Vec::new(),
                unit_code_id: None,
                observed_at: None,
                expires_at: None,
                proof_seals: None,
            },
            created_at: None,
            modified_at: None,
        }
    }

    #[test]
    fn the_insert_binds_the_companion_columns_and_the_element_trees() {
        let sql = attribute_history_insert_sql(table_of(&ContextTarget::Live, AttributeHistory::Table)).expect("the fixed attribute-history insert builds");
        assert!(
            sql.contains(r#""value_uri", "value_type_key", "object_type_keys", "proof_seals", "expires_at", "sub_attributes")"#),
            "{sql}"
        );
        assert!(sql.contains(r"$26::sub_attribute_tree[]) AS d("), "{sql}");
        assert!(sql.contains(r#""d"."object_type_keys"::integer[]"#), "{sql}");
        let mut rows = vec![row(0, Vec::new()), row(1, vec![element()]), row(2, Vec::new())];
        let trees = element_trees(&mut rows).expect("the elements encode");
        let carried: Vec<Option<usize>> = trees.iter().map(|tree| tree.as_ref().map(|tree| tree.elements.len())).collect();
        pretty_assertions::assert_eq!(carried, vec![None, Some(1), None]);
    }

    #[test]
    fn the_insert_carries_the_partition_column_and_the_whole_write_identity() {
        let sql = attribute_history_insert_sql(table_of(&ContextTarget::Live, AttributeHistory::Table)).expect("the fixed attribute-history insert builds");
        assert!(sql.contains(r#"("recorded_at", "write_id", "stream_key", "write_ordinal""#), "{sql}");
    }

    #[test]
    fn a_row_replayed_under_its_original_write_identity_is_not_duplicated() {
        let sql = attribute_history_insert_sql(table_of(&ContextTarget::Live, AttributeHistory::Table)).expect("the fixed attribute-history insert builds");
        assert!(sql.contains(r#"ON CONFLICT ("recorded_at", "write_id", "write_ordinal") DO NOTHING"#), "{sql}");
    }

    #[test]
    fn the_numeric_value_reaches_storage_through_a_numeric_array_bind() {
        let sql = attribute_history_insert_sql(table_of(&ContextTarget::Live, AttributeHistory::Table)).expect("the fixed attribute-history insert builds");
        assert!(sql.contains(r"$16::numeric[]"), "{sql}");
        assert!(!sql.contains(r#""d"."value_number"::numeric"#), "{sql}");
        assert!(!sql.contains("double precision"), "{sql}");
    }
}
