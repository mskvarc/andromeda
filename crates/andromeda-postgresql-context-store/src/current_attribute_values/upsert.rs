//! Writing an entity mutation's attribute instances into the projection.

use crate::{
    current_attribute_values::{columns::ProjectionBinds, row::ProjectedAttributeRow},
    error::ContextStoreError,
};
use andromeda_postgresql_schema::current::CurrentAttributeValues;
use andromeda_postgresql_seaquery::{
    pg_type::PostgresType,
    placeholder::external_placeholder,
    postgis::validated_geom_from_ewkb,
    unnest::{UnnestColumn, apply_with_ordinality, unnest_call},
};
use sea_query::{CaseStatement, Expr, ExprTrait, Iden, Keyword, OnConflict, Order, PostgresQueryBuilder, Query, SimpleExpr};
use sqlx::{Postgres, Transaction};

/// The `UNNEST` set-write's staging alias, `d`.
///
/// The rows of the parallel arrays are read under this alias before the insert
/// sees them; it is spelled nowhere else in the statement, so the single
/// `AS "d"` the table source emits is the one [`apply_with_ordinality`] rewrites.
#[derive(Iden, Clone, Copy)]
#[iden = "d"]
struct Staging;

/// The row-position column `WITH ORDINALITY` appends to the staging alias.
#[derive(Iden, Clone, Copy)]
#[iden = "ordinality"]
struct Ordinality;

/// Upsert a batch of attribute instances into the projection.
///
/// Runs in the caller's transaction, which is the transaction that writes the
/// entity itself: the projection and the entity state it projects commit
/// together or not at all, so a query can never be answered from a projection
/// that disagrees with `entities.payload`.
///
/// Set-based — one statement per batch regardless of how many instances a
/// mutation touches. A batch is permitted to name the same instance more than
/// once, which is what a clause 5.6.7 batch upsert does when two operations in
/// it address the same attribute; the later row wins, decided by position
/// within the batch rather than by any timestamp a client supplied.
///
/// The fourteen column arrays bind through `sqlx` rather than sea-query because
/// nine of them carry NULLs (`Vec<Option<T>>`), which `sea-query-sqlx` cannot
/// represent — see [`external_placeholder`]. The statement structure is still
/// built by sea-query; only the array values are bound outside it, in the marker
/// order [`projection_upsert_sql`] emits.
///
/// # Errors
///
/// Returns [`ContextStoreError::Encoding`] when a geometry cannot be encoded as
/// EWKB, and [`ContextStoreError::Database`] when the statement fails.
pub async fn upsert_projection_in_tx(tx: &mut Transaction<'_, Postgres>, rows: &[ProjectedAttributeRow]) -> Result<(), ContextStoreError> {
    if rows.is_empty() {
        return Ok(());
    }

    let binds = ProjectionBinds::from_rows(rows)?;
    let sql = projection_upsert_sql()?;
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(&binds.entity_keys)
        .bind(&binds.modified_ats)
        .bind(&binds.attribute_name_ids)
        .bind(&binds.attribute_kinds)
        .bind(&binds.value_kinds)
        .bind(&binds.value_datetimes)
        .bind(&binds.unit_code_ids)
        .bind(&binds.value_bools)
        .bind(&binds.dataset_ids)
        .bind(&binds.value_numbers)
        .bind(&binds.value_strings)
        .bind(&binds.value_geos)
        .bind(&binds.value_objects)
        .bind(&binds.value_uris)
        .execute(&mut **tx)
        .await?;

    Ok(())
}

/// The fourteen staging columns, in the order the batch binds them.
///
/// Each pairs the name the row column takes in the `AS d(…)` alias with the
/// array element type its `UNNEST` argument casts to. The `value_number` array
/// is `numeric[]`, so exact decimals are bound without a text or binary64 hop.
fn staging_columns() -> [UnnestColumn; 14] {
    [
        UnnestColumn::new(CurrentAttributeValues::EntityKey, PostgresType::BigintArray),
        UnnestColumn::new(CurrentAttributeValues::ModifiedAt, PostgresType::TimestamptzArray),
        UnnestColumn::new(CurrentAttributeValues::AttributeNameId, PostgresType::IntegerArray),
        UnnestColumn::new(CurrentAttributeValues::AttributeKind, PostgresType::SmallintArray),
        UnnestColumn::new(CurrentAttributeValues::ValueKind, PostgresType::SmallintArray),
        UnnestColumn::new(CurrentAttributeValues::ValueDatetime, PostgresType::TimestamptzArray),
        UnnestColumn::new(CurrentAttributeValues::UnitCodeId, PostgresType::SmallintArray),
        UnnestColumn::new(CurrentAttributeValues::ValueBool, PostgresType::BooleanArray),
        UnnestColumn::new(CurrentAttributeValues::DatasetId, PostgresType::TextArray),
        UnnestColumn::new(CurrentAttributeValues::ValueNumber, PostgresType::NumericArray),
        UnnestColumn::new(CurrentAttributeValues::ValueString, PostgresType::TextArray),
        UnnestColumn::new(CurrentAttributeValues::ValueGeo, PostgresType::ByteaArray),
        UnnestColumn::new(CurrentAttributeValues::ValueObject, PostgresType::JsonbArray),
        UnnestColumn::new(CurrentAttributeValues::ValueUri, PostgresType::TextArray),
    ]
}

/// A staging column reference, `d.<column>`.
fn staged(column: CurrentAttributeValues) -> SimpleExpr {
    Expr::col((Staging, column))
}

/// Build the projection upsert statement for one batch.
///
/// `DISTINCT ON` collapses repeats of the clause 4.5.5 grain before the insert
/// sees them. Without it a batch naming one instance twice would abort with
/// "cannot affect row a second time", since `ON CONFLICT DO UPDATE` refuses to
/// touch a row twice in one statement. The ordinality the `UNNEST` carries is
/// the tie-break: the last position in the batch wins, which is the order the
/// caller's operations were applied to the entity payload in.
///
/// The conflict target is the natural grain, whose unique index treats two
/// absent `datasetId` values as the same instance. That is the whole reason a
/// default instance stays singular under repeated writes rather than
/// accumulating a row per write.
///
/// The `value_geo` column is wrapped as
/// `CASE WHEN d.value_geo IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.value_geo)) ELSE NULL END`:
/// the EWKB already carries its SRID, so no re-SRID is applied, and a NULL EWKB
/// projects to a NULL geometry rather than a call on a NULL argument.
///
/// # Errors
///
/// Returns [`ContextStoreError::Internal`] if the projection's expression count
/// ever stops matching the insert's fourteen-wide column list, which the fixed
/// shapes here make unreachable.
fn projection_upsert_sql() -> Result<String, ContextStoreError> {
    let columns = staging_columns();
    let call = unnest_call(columns.iter().enumerate().map(|(index, column)| (column, external_placeholder(index + 1))));

    let geometry: SimpleExpr = CaseStatement::new()
        .case(
            Expr::col((Staging, CurrentAttributeValues::ValueGeo)).is_not_null(),
            validated_geom_from_ewkb(staged(CurrentAttributeValues::ValueGeo)),
        )
        .finally(Keyword::Null)
        .into();

    let mut select = Query::select();
    select
        .distinct_on([
            (Staging, CurrentAttributeValues::EntityKey),
            (Staging, CurrentAttributeValues::AttributeNameId),
            (Staging, CurrentAttributeValues::DatasetId),
        ])
        .expr(staged(CurrentAttributeValues::EntityKey))
        .expr(staged(CurrentAttributeValues::ModifiedAt))
        .expr(staged(CurrentAttributeValues::AttributeNameId))
        .expr(staged(CurrentAttributeValues::AttributeKind))
        .expr(staged(CurrentAttributeValues::ValueKind))
        .expr(staged(CurrentAttributeValues::ValueDatetime))
        .expr(staged(CurrentAttributeValues::UnitCodeId))
        .expr(staged(CurrentAttributeValues::ValueBool))
        .expr(staged(CurrentAttributeValues::DatasetId))
        .expr(staged(CurrentAttributeValues::ValueNumber))
        .expr(staged(CurrentAttributeValues::ValueString))
        .expr(geometry)
        .expr(staged(CurrentAttributeValues::ValueObject))
        .expr(staged(CurrentAttributeValues::ValueUri))
        .from_function(call, Staging)
        .order_by((Staging, CurrentAttributeValues::EntityKey), Order::Asc)
        .order_by((Staging, CurrentAttributeValues::AttributeNameId), Order::Asc)
        .order_by((Staging, CurrentAttributeValues::DatasetId), Order::Asc)
        .order_by((Staging, Ordinality), Order::Desc);
    let select = select;

    let conflict = OnConflict::columns([
        CurrentAttributeValues::EntityKey,
        CurrentAttributeValues::AttributeNameId,
        CurrentAttributeValues::DatasetId,
    ])
    .update_columns([
        CurrentAttributeValues::ModifiedAt,
        CurrentAttributeValues::AttributeKind,
        CurrentAttributeValues::ValueKind,
        CurrentAttributeValues::ValueDatetime,
        CurrentAttributeValues::UnitCodeId,
        CurrentAttributeValues::ValueBool,
        CurrentAttributeValues::ValueNumber,
        CurrentAttributeValues::ValueString,
        CurrentAttributeValues::ValueGeo,
        CurrentAttributeValues::ValueObject,
        CurrentAttributeValues::ValueUri,
    ])
    .to_owned();

    let mut insert = Query::insert();
    insert.into_table(CurrentAttributeValues::Table).columns([
        CurrentAttributeValues::EntityKey,
        CurrentAttributeValues::ModifiedAt,
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
    ]);
    // `select_from` fails only on a column/expression arity mismatch, which the
    // fixed fourteen-wide column list and projection above make unreachable.
    insert.select_from(select).map_err(|error| ContextStoreError::Internal(error.to_string()))?;
    insert.on_conflict(conflict);

    let sql = insert.to_string(PostgresQueryBuilder);
    Ok(apply_with_ordinality(sql, &Staging, &columns, &Ordinality))
}

#[cfg(test)]
mod tests {
    use crate::current_attribute_values::upsert::projection_upsert_sql;
    use std::collections::BTreeSet;

    /// The rendered upsert statement, unwrapped for assertion.
    fn rendered() -> String {
        projection_upsert_sql().expect("the fixed fourteen-wide projection builds")
    }

    /// Every `$n` placeholder in `sql`, as numbers.
    fn placeholders(sql: &str) -> BTreeSet<u32> {
        sql.match_indices('$')
            .filter_map(|(at, _)| {
                let digits: String = sql[at + 1..].chars().take_while(char::is_ascii_digit).collect();
                digits.parse().ok()
            })
            .collect()
    }

    #[test]
    fn the_upsert_binds_one_contiguous_array_per_column() {
        pretty_assertions::assert_eq!(placeholders(&rendered()), (1..=14).collect::<BTreeSet<u32>>());
    }

    #[test]
    fn each_array_argument_casts_to_its_element_type() {
        let sql = rendered();
        for cast in [
            "$1::bigint[]",
            "$2::timestamptz[]",
            "$3::integer[]",
            "$4::smallint[]",
            "$8::boolean[]",
            "$10::numeric[]",
            "$12::bytea[]",
            "$13::jsonb[]",
        ] {
            assert!(sql.contains(cast), "missing {cast} in {sql}");
        }
    }

    #[test]
    fn the_upsert_collapses_repeats_of_the_instance_grain_before_inserting() {
        let sql = rendered();
        assert!(
            sql.contains(r#"DISTINCT ON ("d"."entity_key", "d"."attribute_name_id", "d"."dataset_id")"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"ORDER BY "d"."entity_key" ASC, "d"."attribute_name_id" ASC, "d"."dataset_id" ASC, "d"."ordinality" DESC"#),
            "the last operation in a batch must be the one that survives: {sql}"
        );
    }

    #[test]
    fn the_upsert_conflicts_on_the_instance_grain() {
        let sql = rendered();
        assert!(
            sql.contains(r#"ON CONFLICT ("entity_key", "attribute_name_id", "dataset_id") DO UPDATE SET"#),
            "{sql}"
        );
    }

    #[test]
    fn a_number_reaches_the_arbitrary_precision_column_rather_than_a_double() {
        let sql = rendered();
        assert!(sql.contains(r#"$10::numeric[]"#), "{sql}");
        assert!(!sql.contains(r#""d"."value_number"::numeric"#), "{sql}");
        assert!(!sql.contains("double precision"), "{sql}");
        assert!(!sql.contains("float8"), "{sql}");
    }

    #[test]
    fn a_geometry_is_validated_from_its_embedded_srid_and_a_null_stays_null() {
        let sql = rendered();
        assert!(sql.contains("ST_MakeValid(ST_GeomFromEWKB("), "{sql}");
        assert!(sql.contains(r#"WHEN ("d"."value_geo" IS NOT NULL)"#), "{sql}");
        assert!(sql.contains("ELSE NULL END"), "{sql}");
        assert!(!sql.contains("ST_SetSRID"), "{sql}");
        assert!(!sql.contains("ST_GeomFromGeoJSON"), "{sql}");
    }

    #[test]
    fn the_staging_source_carries_ordinality_and_names_every_column() {
        let sql = rendered();
        assert!(!sql.contains(r#"AS "d""#), "the closed table-source alias must be rewritten: {sql}");
        assert!(
            sql.contains(
                "WITH ORDINALITY AS d(entity_key, modified_at, attribute_name_id, attribute_kind, value_kind, value_datetime, unit_code_id, value_bool, dataset_id, value_number, value_string, value_geo, value_object, value_uri, ordinality)"
            ),
            "{sql}"
        );
    }

    #[test]
    fn the_upsert_refreshes_every_projected_column() {
        let sql = rendered();
        for column in [
            "modified_at",
            "attribute_kind",
            "value_kind",
            "value_datetime",
            "unit_code_id",
            "value_bool",
            "value_number",
            "value_string",
            "value_geo",
            "value_object",
            "value_uri",
        ] {
            assert!(
                sql.contains(&format!(r#""{column}" = "excluded"."{column}""#)),
                "a superseded row must not keep its old {column}: {sql}"
            );
        }
    }
}
