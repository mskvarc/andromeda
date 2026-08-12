//! Writing an entity mutation's attribute instances into the projection.

use crate::{
    current_attribute_values::{columns::ProjectionBinds, row::ProjectedAttributeRow},
    error::ContextStoreError,
};
use sqlx::{Postgres, Transaction};

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
/// # Errors
///
/// Returns [`ContextStoreError::Encoding`] when a geometry cannot be encoded as
/// EWKB, and [`ContextStoreError::Database`] when the statement fails.
pub async fn upsert_projection_in_tx(tx: &mut Transaction<'_, Postgres>, rows: &[ProjectedAttributeRow]) -> Result<(), ContextStoreError> {
    if rows.is_empty() {
        return Ok(());
    }

    let binds = ProjectionBinds::from_rows(rows)?;
    sqlx::query(UPSERT_PROJECTION_SQL)
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

/// Upsert statement for one projection batch.
///
/// `DISTINCT ON` collapses repeats of the clause 4.5.5 grain before the insert
/// sees them. Without it a batch naming one instance twice would abort with
/// "cannot affect row a second time", since `ON CONFLICT DO UPDATE` refuses to
/// touch a row twice in one statement. The ordinality the `UNNEST` carries is
/// the tie-break: last position in the batch wins, which is the order the
/// caller's operations were applied to the entity payload in.
///
/// The conflict target is the natural grain, whose unique index treats two
/// absent `datasetId` values as the same instance. That is the whole reason a
/// default instance stays singular under repeated writes rather than
/// accumulating a row per write.
const UPSERT_PROJECTION_SQL: &str = r#"
INSERT INTO current_attribute_values (
    entity_key, modified_at, attribute_name_id, attribute_kind, value_kind,
    value_datetime, unit_code_id, value_bool, dataset_id,
    value_number, value_string, value_geo, value_object, value_uri
)
SELECT DISTINCT ON (d.entity_key, d.attribute_name_id, d.dataset_id)
    d.entity_key, d.modified_at, d.attribute_name_id, d.attribute_kind, d.value_kind,
    d.value_datetime, d.unit_code_id, d.value_bool, d.dataset_id,
    d.value_number::numeric, d.value_string,
    CASE WHEN d.value_geo IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.value_geo)) ELSE NULL END,
    d.value_object, d.value_uri
FROM UNNEST(
    $1::bigint[], $2::timestamptz[], $3::integer[], $4::smallint[], $5::smallint[],
    $6::timestamptz[], $7::smallint[], $8::boolean[], $9::text[],
    $10::text[], $11::text[], $12::bytea[], $13::jsonb[], $14::text[]
) WITH ORDINALITY AS d(entity_key, modified_at, attribute_name_id, attribute_kind, value_kind,
       value_datetime, unit_code_id, value_bool, dataset_id,
       value_number, value_string, value_geo, value_object, value_uri, ordinality)
ORDER BY d.entity_key, d.attribute_name_id, d.dataset_id, d.ordinality DESC
ON CONFLICT (entity_key, attribute_name_id, dataset_id) DO UPDATE SET
    modified_at    = EXCLUDED.modified_at,
    attribute_kind = EXCLUDED.attribute_kind,
    value_kind     = EXCLUDED.value_kind,
    value_datetime = EXCLUDED.value_datetime,
    unit_code_id   = EXCLUDED.unit_code_id,
    value_bool     = EXCLUDED.value_bool,
    value_number   = EXCLUDED.value_number,
    value_string   = EXCLUDED.value_string,
    value_geo      = EXCLUDED.value_geo,
    value_object   = EXCLUDED.value_object,
    value_uri      = EXCLUDED.value_uri
"#;

#[cfg(test)]
mod tests {
    use crate::current_attribute_values::upsert::UPSERT_PROJECTION_SQL;
    use std::collections::BTreeSet;

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
        pretty_assertions::assert_eq!(placeholders(UPSERT_PROJECTION_SQL), (1..=14).collect::<BTreeSet<u32>>());
    }

    #[test]
    fn the_upsert_collapses_repeats_of_the_instance_grain_before_inserting() {
        assert!(UPSERT_PROJECTION_SQL.contains("SELECT DISTINCT ON (d.entity_key, d.attribute_name_id, d.dataset_id)"));
        assert!(
            UPSERT_PROJECTION_SQL.contains("ORDER BY d.entity_key, d.attribute_name_id, d.dataset_id, d.ordinality DESC"),
            "the last operation in a batch must be the one that survives"
        );
    }

    #[test]
    fn the_upsert_conflicts_on_the_instance_grain() {
        assert!(UPSERT_PROJECTION_SQL.contains("ON CONFLICT (entity_key, attribute_name_id, dataset_id) DO UPDATE SET"));
    }

    #[test]
    fn a_number_reaches_the_arbitrary_precision_column_rather_than_a_double() {
        assert!(UPSERT_PROJECTION_SQL.contains("d.value_number::numeric"));
        assert!(!UPSERT_PROJECTION_SQL.contains("double precision"));
    }

    #[test]
    fn the_upsert_refreshes_every_projected_column() {
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
                UPSERT_PROJECTION_SQL.contains(&format!("= EXCLUDED.{column}")),
                "a superseded row must not keep its old {column}"
            );
        }
    }
}
