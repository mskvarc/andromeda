//! Writing an entity mutation's attribute instances into the projection.
//!
//! One set-based statement per batch, built in stages: [`staging`] unnests and
//! deduplicates the batch, [`update_first`] updates the grains that already
//! exist, and the final insert writes the rest under the conflict action
//! [`no_op_guard`] shares with the update. [`column_sets`] holds the column
//! lists all three agree on.

mod column_sets;
mod no_op_guard;
mod staging;
mod update_first;

use crate::{
    current_attribute_values::{
        columns::ProjectionBinds,
        row::ProjectedAttributeRow,
        upsert::{
            column_sets::{PROJECTION_COLUMNS, staging_columns},
            no_op_guard::concurrent_insert_conflict,
            staging::{Ordinality, Staging, input_cte},
            update_first::{matched_cte, unmatched_input_select, updated_cte},
        },
    },
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::current::CurrentAttributeValues;
use andromeda_postgresql_seaquery::unnest::apply_with_ordinality;
use andromeda_telemetry::statement::{StatementName, observe_statement};
use sea_query::{PostgresQueryBuilder, Query, WithClause};
use sqlx::{Postgres, Transaction};

/// The statement name the projection upsert is observed under.
const CAV_UPSERT: StatementName = StatementName("cav-upsert");

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
/// The fourteen column arrays bind through `sqlx` rather than sea-query because
/// ten of them carry NULLs (`Vec<Option<T>>`), which `sea-query-sqlx` cannot
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
        .bind(&binds.attribute_name_keys)
        .bind(&binds.attribute_kinds)
        .bind(&binds.value_kinds)
        .bind(&binds.value_date_times)
        .bind(&binds.unit_code_ids)
        .bind(&binds.value_booleans)
        .bind(&binds.dataset_ids)
        .bind(&binds.value_numbers)
        .bind(&binds.value_strings)
        .bind(&binds.value_geometries)
        .bind(&binds.value_objects)
        .bind(&binds.value_uris)
        .bind(&binds.value_long_texts)
        .execute(&mut **tx);
    observe_statement(CAV_UPSERT, statement).await?;

    Ok(())
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
        projection_upsert_sql(&ContextTarget::Live).expect("the fixed fourteen-wide projection builds")
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
            "$2::integer[]",
            "$3::smallint[]",
            "$4::smallint[]",
            "$5::timestamptz[]",
            "$7::boolean[]",
            "$9::numeric[]",
            "$11::bytea[]",
            "$12::jsonb[]",
            "$14::text[]",
        ] {
            assert!(sql.contains(cast), "missing {cast} in {sql}");
        }
    }

    #[test]
    fn the_upsert_collapses_repeats_of_the_instance_grain_before_inserting() {
        let sql = rendered();
        assert!(
            sql.contains(r#"DISTINCT ON ("d"."entity_key", "d"."attribute_name_key", "d"."dataset_id")"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"ORDER BY "d"."entity_key" ASC, "d"."attribute_name_key" ASC, "d"."dataset_id" ASC, "d"."ordinality" DESC"#),
            "the last operation in a batch must be the one that survives: {sql}"
        );
    }

    #[test]
    fn the_upsert_conflicts_on_the_instance_grain() {
        let sql = rendered();
        assert!(
            sql.contains(r#"ON CONFLICT ("entity_key", "attribute_name_key", "dataset_id") DO UPDATE SET"#),
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
            sql.contains(r#"ON CONFLICT ("entity_key", "attribute_name_key", "dataset_id") DO UPDATE SET"#),
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
        assert!(sql.contains(r#"WHEN ("d"."value_geometry" IS NOT NULL)"#), "{sql}");
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
                "WITH ORDINALITY AS d(entity_key, attribute_name_key, attribute_kind, value_kind, value_date_time, unit_code_id, value_boolean, dataset_id, value_number, value_string, value_geometry, value_object, value_uri, value_long_text, ordinality)"
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
            "value_date_time",
            "unit_code_id",
            "value_boolean",
            "value_number",
            "value_string",
            "value_geometry",
            "value_object",
            "value_uri",
            "value_long_text",
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
        assert!(guard.trim_end().ends_with("\"excluded\".\"value_long_text\")"), "{guard}");
        for grain in ["entity_key", "attribute_name_key", "dataset_id"] {
            assert!(
                !guard.contains(grain),
                "a grain column is equal on conflict by construction and must not be compared: {guard}"
            );
        }
        for column in [
            "attribute_kind",
            "value_kind",
            "value_date_time",
            "unit_code_id",
            "value_boolean",
            "value_number",
            "value_string",
            "value_geometry",
            "value_object",
            "value_uri",
            "value_long_text",
        ] {
            assert!(
                guard.contains(&format!("\"current_attribute_values\".\"{column}\"")) && guard.contains(&format!("\"excluded\".\"{column}\"")),
                "the guard must compare {column} on both sides: {guard}"
            );
        }
    }
}
