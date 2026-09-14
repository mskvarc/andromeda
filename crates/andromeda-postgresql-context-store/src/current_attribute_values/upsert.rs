//! Writing an entity mutation's attribute instances into the projection.

use crate::{
    current_attribute_values::{columns::ProjectionBinds, row::ProjectedAttributeRow},
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::current::CurrentAttributeValues;
use andromeda_postgresql_seaquery::{
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    placeholder::external_placeholder,
    postgis::validated_geom_from_ewkb,
    unnest::{UnnestColumn, apply_with_ordinality, unnest_call},
};
use andromeda_telemetry::statement::{StatementName, observe_statement};
use sea_query::{
    CaseStatement,
    CommonTableExpression,
    Condition,
    Expr,
    ExprTrait,
    Iden,
    Keyword,
    LockType,
    OnConflict,
    Order,
    PostgresQueryBuilder,
    Query,
    SelectStatement,
    SimpleExpr,
    WithClause,
};
use sqlx::{Postgres, Transaction};

/// The statement name the projection upsert is observed under.
const CAV_UPSERT: StatementName = StatementName("cav-upsert");

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

/// The deduplicated and geometry-validated batch rows.
#[derive(Iden, Clone, Copy)]
#[iden = "input"]
struct Input;

/// Input rows whose projection grain already exists, together with its TID.
#[derive(Iden, Clone, Copy)]
#[iden = "matched"]
struct Matched;

/// Existing projection rows changed by the batch.
#[derive(Iden, Clone, Copy)]
#[iden = "updated"]
struct Updated;

/// The source row's position in the caller's batch.
#[derive(Iden, Clone, Copy)]
#[iden = "input_id"]
struct InputId;

/// The physical identifier of a matched projection row.
#[derive(Iden, Clone, Copy)]
#[iden = "target_tid"]
struct TargetTid;

/// PostgreSQL's physical tuple-identifier system column.
#[derive(Iden, Clone, Copy)]
#[iden = "ctid"]
struct Ctid;

/// The `excluded` pseudo-relation an `ON CONFLICT DO UPDATE` action reads the
/// proposed row from.
#[derive(Iden, Clone, Copy)]
#[iden = "excluded"]
struct Excluded;

/// The columns the upsert refreshes on conflict, in table order: everything the
/// row carries except the three grain columns the conflict target names.
///
/// This is also the exact column set the no-op guard compares, so a projected
/// row and its stored predecessor are value-identical when and only when every
/// column here is; the grain columns are equal by construction on a conflict.
const REFRESHED_COLUMNS: [CurrentAttributeValues; 10] = [
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
];

/// All persisted projection columns, in table order.
const PROJECTION_COLUMNS: [CurrentAttributeValues; 13] = [
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
];

/// Upsert a batch of attribute instances into the projection.
///
/// Runs in the caller's transaction, which is the transaction that writes the
/// entity itself: the projection and the entity state it projects commit
/// together or not at all, so a query can never be answered from a projection
/// that disagrees with `entity_documents.payload`.
///
/// Set-based — one statement per batch regardless of how many instances a
/// mutation touches. A batch is permitted to name the same instance more than
/// once, which is what a clause 5.6.7 batch upsert does when two operations in
/// it address the same attribute; the later row wins, decided by position
/// within the batch rather than by any timestamp a client supplied.
///
/// Existing grains take an update-first path: the statement locks each match,
/// updates it directly by `ctid` when a refreshed column differs, then inserts
/// only inputs that had no match. `ON CONFLICT` remains on that insert solely
/// for a concurrent transaction that creates the same missing grain. This
/// avoids routing the normal update workload through speculative insertion.
///
/// A row whose projection is value-identical to what is stored is not written
/// at all. Both the direct update and concurrent-insert fallback compare every
/// non-grain column with `ROW(stored) IS DISTINCT FROM ROW(proposed)`, so a
/// restated reading leaves no new heap or value-index tuple. The equality is
/// each column type's own -- `PostGIS` geometry `=` is exact geometry equality,
/// jsonb `=` is structural, and `NUMERIC` `=` ignores scale (`1.0 = 1.00`),
/// which is acceptable because no reader renders a projected number, only
/// compares it.
///
/// The thirteen column arrays bind through `sqlx` rather than sea-query because
/// nine of them carry NULLs (`Vec<Option<T>>`), which `sea-query-sqlx` cannot
/// represent — see [`external_placeholder`]. The statement structure is still
/// built by sea-query; only the array values are bound outside it, in the marker
/// order [`projection_upsert_sql`] emits.
///
/// # Errors
///
/// Returns [`ContextStoreError::Encoding`] when a geometry cannot be encoded as
/// EWKB, and [`ContextStoreError::Database`] when the statement fails.
pub async fn upsert_projection_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    rows: &[ProjectedAttributeRow],
) -> Result<(), ContextStoreError> {
    if rows.is_empty() {
        return Ok(());
    }

    let binds = ProjectionBinds::from_rows(rows)?;
    let sql = projection_upsert_sql(target)?;
    let statement = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(&binds.entity_keys)
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
        .execute(&mut **tx);
    observe_statement(CAV_UPSERT, statement).await?;

    Ok(())
}

/// The thirteen staging columns, in the order the batch binds them.
///
/// Each pairs the name the row column takes in the `AS d(…)` alias with the
/// array element type its `UNNEST` argument casts to. The `value_number` array
/// is `numeric[]`, so exact decimals are bound without a text or binary64 hop.
fn staging_columns() -> [UnnestColumn; 13] {
    [
        UnnestColumn::new(CurrentAttributeValues::EntityKey, PostgresType::BigintArray),
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

/// The no-op guard of the conflict action: the stored row's refreshed columns,
/// as one row value, are distinct from the proposed row's.
///
/// `IS DISTINCT FROM` over row values compares field by field with NULL
/// counting as a value, so a NULL typed column that stays NULL is equal, and a
/// value that moves into a different typed column is a difference on two
/// fields at once. Written over every refreshed column rather than a subset so
/// the guard and the `SET` list can never disagree about what "identical" means.
fn stored_row_differs_from<T>(proposed_source: T) -> SimpleExpr
where
    T: Iden + Copy + Send + Sync + 'static,
{
    let stored = Expr::tuple(REFRESHED_COLUMNS.map(|column| Expr::col((CurrentAttributeValues::Table, column))));
    let proposed = Expr::tuple(REFRESHED_COLUMNS.map(|column| Expr::col((proposed_source, column))));
    stored.distinct_from(proposed)
}

/// The `input` common table expression: the batch, deduplicated to the clause
/// 4.5.5 grain and with its geometries validated.
///
/// `DISTINCT ON` collapses repeats of the grain before the insert sees them.
/// Without it a batch naming one instance twice would abort with "cannot affect
/// row a second time", since `ON CONFLICT DO UPDATE` refuses to touch a row
/// twice in one statement. The ordinality the `UNNEST` carries is the tie-break:
/// the last position in the batch wins, which is the order the caller's
/// operations were applied to the entity payload in.
///
/// The `value_geo` column is wrapped as
/// `CASE WHEN d.value_geo IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.value_geo)) ELSE NULL END`:
/// the EWKB already carries its SRID, so no re-SRID is applied, and a NULL EWKB
/// projects to a NULL geometry rather than a call on a NULL argument.
fn input_cte(columns: &[UnnestColumn]) -> CommonTableExpression {
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
        .expr(Expr::col((Staging, Ordinality)))
        .from_function(call, Staging)
        .order_by((Staging, CurrentAttributeValues::EntityKey), Order::Asc)
        .order_by((Staging, CurrentAttributeValues::AttributeNameId), Order::Asc)
        .order_by((Staging, CurrentAttributeValues::DatasetId), Order::Asc)
        .order_by((Staging, Ordinality), Order::Desc);
    let select = select;

    CommonTableExpression::new()
        .table_name(Input)
        .columns(PROJECTION_COLUMNS)
        .column(InputId)
        .query(select)
        .materialized(true)
        .to_owned()
}

/// The `matched` common table expression: input rows whose grain already exists,
/// locked, each carrying the physical tuple ID of the stored row.
///
/// The join uses NULL-safe equality for `datasetId` because absent dataset IDs
/// identify the single default attribute instance. Locking here is what lets the
/// update stage change rows by `ctid` without racing a concurrent writer.
fn matched_cte(target: &ContextTarget) -> CommonTableExpression {
    let mut select = Query::select();
    select
        .columns(PROJECTION_COLUMNS.map(|column| (Input, column)))
        .column((Input, InputId))
        .expr_as(Expr::col((CurrentAttributeValues::Table, Ctid)), TargetTid)
        .from(Input)
        .inner_join(
            table_of(target, CurrentAttributeValues::Table),
            Condition::all()
                .add(Expr::col((CurrentAttributeValues::Table, CurrentAttributeValues::EntityKey)).equals((Input, CurrentAttributeValues::EntityKey)))
                .add(
                    Expr::col((CurrentAttributeValues::Table, CurrentAttributeValues::AttributeNameId))
                        .equals((Input, CurrentAttributeValues::AttributeNameId)),
                )
                .add(
                    SimpleExpr::from(Expr::col((CurrentAttributeValues::Table, CurrentAttributeValues::DatasetId)))
                        .not_distinct_from(Expr::col((Input, CurrentAttributeValues::DatasetId))),
                ),
        )
        .lock_with_tables(LockType::Update, [CurrentAttributeValues::Table]);
    let select = select;

    CommonTableExpression::new()
        .table_name(Matched)
        .columns(PROJECTION_COLUMNS)
        .column(InputId)
        .column(TargetTid)
        .query(select)
        .materialized(true)
        .to_owned()
}

/// The `updated` common table expression: the direct update of every matched row
/// whose refreshed columns differ, addressed by its physical tuple ID.
fn updated_cte(target: &ContextTarget) -> CommonTableExpression {
    let mut update = Query::update();
    update
        .table(table_of(target, CurrentAttributeValues::Table))
        .values(REFRESHED_COLUMNS.map(|column| (column, Expr::col((Matched, column)))))
        .from(Matched)
        .and_where(Expr::col((CurrentAttributeValues::Table, Ctid)).equals((Matched, TargetTid)))
        .and_where(stored_row_differs_from(Matched))
        .returning_col((CurrentAttributeValues::Table, CurrentAttributeValues::EntityKey));
    let update = update;

    CommonTableExpression::new().table_name(Updated).query(update).to_owned()
}

/// The rows the final insert writes: every input the match stage did not claim,
/// including inputs whose stored row was value-identical and so left unchanged.
fn unmatched_input_select() -> SelectStatement {
    let matched_input = Query::select()
        .expr(Expr::val(1))
        .from(Matched)
        .and_where(Expr::col((Matched, InputId)).equals((Input, InputId)))
        .to_owned();

    Query::select()
        .columns(PROJECTION_COLUMNS.map(|column| (Input, column)))
        .from(Input)
        .and_where(Expr::exists(matched_input).not())
        .to_owned()
}

/// The natural-grain conflict action retained on the insert solely for a
/// concurrent transaction that created the same missing grain in between.
fn concurrent_insert_conflict() -> OnConflict {
    OnConflict::columns([
        CurrentAttributeValues::EntityKey,
        CurrentAttributeValues::AttributeNameId,
        CurrentAttributeValues::DatasetId,
    ])
    .update_columns(REFRESHED_COLUMNS)
    .action_and_where(stored_row_differs_from(Excluded))
    .to_owned()
}

/// Build the projection upsert statement for one batch.
///
/// # Errors
///
/// Returns [`ContextStoreError::Internal`] if the projection's expression count
/// ever stops matching the insert's fourteen-wide column list, which the fixed
/// shapes of the stages make unreachable.
fn projection_upsert_sql(target: &ContextTarget) -> Result<String, ContextStoreError> {
    let columns = staging_columns();
    let stages = WithClause::new()
        .cte(input_cte(&columns))
        .cte(matched_cte(target))
        .cte(updated_cte(target))
        .to_owned();

    let mut insert = Query::insert();
    insert.into_table(table_of(target, CurrentAttributeValues::Table)).columns(PROJECTION_COLUMNS);
    // `select_from` fails only on a column/expression arity mismatch, which the
    // fixed fourteen-wide column list and projection above make unreachable.
    insert
        .select_from(unmatched_input_select())
        .map_err(|error| ContextStoreError::Internal(error.to_string()))?;
    insert.on_conflict(concurrent_insert_conflict()).with_cte(stages);

    let sql = insert.to_string(PostgresQueryBuilder);
    Ok(apply_with_ordinality(sql, &Staging, &columns, &Ordinality))
}

#[cfg(test)]
mod tests {
    use crate::current_attribute_values::upsert::projection_upsert_sql;
    use andromeda_context_space::context_target::ContextTarget;
    use std::collections::BTreeSet;

    /// The rendered upsert statement, unwrapped for assertion.
    fn rendered() -> String {
        projection_upsert_sql(&ContextTarget::Live).expect("the fixed thirteen-wide projection builds")
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
        pretty_assertions::assert_eq!(placeholders(&rendered()), (1..=13).collect::<BTreeSet<u32>>());
    }

    #[test]
    fn each_array_argument_casts_to_its_element_type() {
        let sql = rendered();
        for cast in [
            "$1::bigint[]",
            "$2::integer[]",
            "$3::smallint[]",
            "$4::smallint[]",
            "$5::timestamptz[]",
            "$7::boolean[]",
            "$9::numeric[]",
            "$11::bytea[]",
            "$12::jsonb[]",
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
    fn existing_grains_are_locked_and_updated_before_missing_grains_are_inserted() {
        let sql = rendered();
        let input = sql.find(r#"WITH "input""#).expect("the statement materializes its deduplicated input");
        let matched = sql.find(r#", "matched""#).expect("the statement materializes existing grains");
        let updated = sql.find(r#", "updated""#).expect("the statement carries a data-modifying update CTE");
        let insert = sql
            .find(r#"INSERT INTO "current_attribute_values""#)
            .expect("the statement inserts missing grains");
        assert!(
            input < matched && matched < updated && updated < insert,
            "the update-first stages are out of order: {sql}"
        );
        assert!(
            sql.contains(r#"FOR UPDATE OF "current_attribute_values""#),
            "existing grains must be locked before routing: {sql}"
        );
        assert!(
            sql.contains(r#"UPDATE "current_attribute_values" SET "attribute_kind" = "matched"."attribute_kind""#),
            "existing grains must take the direct update path: {sql}"
        );
        assert!(
            sql.contains(r#""current_attribute_values"."ctid" = "matched"."target_tid""#),
            "the direct update must address exactly the tuple locked by matched: {sql}"
        );
    }

    #[test]
    fn default_dataset_instances_match_with_null_safe_equality() {
        let sql = rendered();
        assert!(
            sql.contains(r#""current_attribute_values"."dataset_id" IS NOT DISTINCT FROM "input"."dataset_id""#),
            "an absent datasetId identifies the one default instance: {sql}"
        );
    }

    #[test]
    fn only_inputs_without_a_locked_match_reach_the_insert() {
        let sql = rendered();
        assert!(
            sql.contains(r#"FROM "input" WHERE NOT EXISTS(SELECT 1 FROM "matched" WHERE "matched"."input_id" = "input"."input_id")"#),
            "matched changes and matched no-ops must both bypass insertion: {sql}"
        );
        assert!(
            sql.contains(r#"ON CONFLICT ("entity_key", "attribute_name_id", "dataset_id") DO UPDATE SET"#),
            "a concurrent creator of an unmatched grain still requires an atomic fallback: {sql}"
        );
    }

    #[test]
    fn a_number_reaches_the_arbitrary_precision_column_rather_than_a_double() {
        let sql = rendered();
        assert!(sql.contains(r"$9::numeric[]"), "{sql}");
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
                "WITH ORDINALITY AS d(entity_key, attribute_name_id, attribute_kind, value_kind, value_datetime, unit_code_id, value_bool, dataset_id, value_number, value_string, value_geo, value_object, value_uri, ordinality)"
            ),
            "{sql}"
        );
    }

    #[test]
    fn the_upsert_refreshes_every_projected_column() {
        let sql = rendered();
        for column in [
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
                "the concurrent-insert fallback must not keep its old {column}: {sql}"
            );
            assert!(
                sql.contains(&format!(r#""{column}" = "matched"."{column}""#)),
                "the direct update must not keep its old {column}: {sql}"
            );
        }
    }

    #[test]
    fn the_upsert_touches_no_row_whose_projection_is_value_identical() {
        let sql = rendered();
        let update_start = sql.find(r#"UPDATE "current_attribute_values""#).expect("the statement has a direct update");
        let insert_start = sql.find(r#"INSERT INTO "current_attribute_values""#).expect("the statement has a final insert");
        let update = &sql[update_start..insert_start];
        assert!(
            update.contains(r#") IS DISTINCT FROM ("matched"."attribute_kind", "#),
            "the direct update must suppress identical rows: {update}"
        );
        let (_, action) = sql.split_once("DO UPDATE SET").expect("the statement carries a conflict action");
        let (_, guard) = action.split_once(" WHERE ").expect("the conflict action is guarded");
        assert!(guard.starts_with("(\"current_attribute_values\".\"attribute_kind\", "), "{guard}");
        assert!(guard.contains(") IS DISTINCT FROM (\"excluded\".\"attribute_kind\", "), "{guard}");
        assert!(guard.trim_end().ends_with("\"excluded\".\"value_uri\")"), "{guard}");
        for grain in ["entity_key", "attribute_name_id", "dataset_id"] {
            assert!(
                !guard.contains(grain),
                "a grain column is equal on conflict by construction and must not be compared: {guard}"
            );
        }
        for column in [
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
                guard.contains(&format!("\"current_attribute_values\".\"{column}\"")) && guard.contains(&format!("\"excluded\".\"{column}\"")),
                "the guard must compare {column} on both sides: {guard}"
            );
        }
    }
}
