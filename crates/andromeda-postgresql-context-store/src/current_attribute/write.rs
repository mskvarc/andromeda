//! The statements that write an Entity's `current_attributes` rows from the
//! `instances` staging, inside the statement that writes its Entity row.
//!
//! Each reads an anchor: the common table expression holding the key and
//! identifier of every Entity row the statement wrote. Only the Entities the
//! anchor holds get rows, and a staged row finds its Entity by identifier.
//! Every depth of an instance's sub-attribute tree travels inside its row, so
//! one statement writes an Entity of any nesting.

use crate::{
    current_attribute::staging::{Instances, ROW_COLUMNS},
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::current::{CurrentAttributes, Entities};
use andromeda_postgresql_seaquery::pg_operator::PgBinaryOperator;
use sea_query::{Cond, DeleteStatement, Expr, ExprTrait, InsertStatement, IntoIden, JoinType, OnConflict, Query, SimpleExpr};

/// The rows of every Entity `anchor` holds, inserted:
/// `INSERT INTO current_attributes (entity_key, …) SELECT anchor.entity_key, instances.… FROM anchor JOIN instances ON instances.id = anchor.id`.
///
/// # Errors
///
/// Returns [`ContextStoreError::StatementBuild`] if the selection ever stops
/// matching the column list, which the shared column set makes unreachable.
pub(crate) fn inserted_rows(target: &ContextTarget, anchor: impl IntoIden) -> Result<InsertStatement, ContextStoreError> {
    let anchor = anchor.into_iden();
    let mut columns = vec![CurrentAttributes::EntityKey];
    columns.extend(ROW_COLUMNS);
    let mut select = Query::select();
    select.expr(Expr::col((anchor.clone(), Entities::EntityKey))); // clone: the anchor names both the key and the join
    for column in ROW_COLUMNS {
        select.expr(Expr::col((Instances, column)));
    }
    select
        .from(anchor.clone()) // clone: the anchor names both the source and the join
        .join(JoinType::Join, Instances, Expr::col((Instances, Entities::Id)).equals((anchor, Entities::Id)));
    let mut insert = Query::insert();
    insert
        .into_table(table_of(target, CurrentAttributes::Table))
        .columns(columns)
        .select_from(select.take())?;
    Ok(insert)
}

/// The rows of every Entity `anchor` holds, written whatever is stored: the
/// insert of [`inserted_rows`], with a stored grain (clause 4.5.5) taking the
/// staged row's every column, `created_at` included.
///
/// # Errors
///
/// Returns [`ContextStoreError::StatementBuild`] if the selection ever stops
/// matching the column list, which the shared column set makes unreachable.
pub(crate) fn replaced_rows(target: &ContextTarget, anchor: impl IntoIden) -> Result<InsertStatement, ContextStoreError> {
    let mut insert = inserted_rows(target, anchor)?;
    insert.on_conflict(
        OnConflict::columns([CurrentAttributes::EntityKey, CurrentAttributes::AttributeNameKey, CurrentAttributes::DatasetId])
            .update_columns(
                ROW_COLUMNS
                    .into_iter()
                    .filter(|column| !matches!(column, CurrentAttributes::AttributeNameKey | CurrentAttributes::DatasetId)),
            )
            .to_owned(),
    );
    Ok(insert)
}

/// The stored rows of every Entity `anchor` holds whose grain (clause 4.5.5)
/// no staged row of that Entity has:
/// `DELETE FROM current_attributes WHERE entity_key IN (SELECT entity_key FROM anchor)
/// AND NOT EXISTS (SELECT 1 FROM instances JOIN anchor ON anchor.id = instances.id
/// WHERE anchor.entity_key = current_attributes.entity_key AND instances.attribute_name_key = current_attributes.attribute_name_key
/// AND instances.dataset_id IS NOT DISTINCT FROM current_attributes.dataset_id)`.
///
/// The grains it removes and the ones [`replaced_rows`] writes are disjoint, so
/// the two run in one statement without touching a row twice.
pub(crate) fn vanished_rows(target: &ContextTarget, anchor: impl IntoIden) -> DeleteStatement {
    let anchor = anchor.into_iden();
    let stored = |column: CurrentAttributes| Expr::col((CurrentAttributes::Table, column));
    let staged = Query::select()
        .expr(Expr::val(1))
        .from(Instances)
        .join(
            JoinType::Join,
            anchor.clone(), // clone: the anchor names the join and both of its key columns
            Expr::col((anchor.clone(), Entities::Id)).equals((Instances, Entities::Id)),
        )
        .cond_where(
            Cond::all()
                .add(Expr::col((anchor.clone(), Entities::EntityKey)).equals((CurrentAttributes::Table, CurrentAttributes::EntityKey)))
                .add(Expr::col((Instances, CurrentAttributes::AttributeNameKey)).equals((CurrentAttributes::Table, CurrentAttributes::AttributeNameKey)))
                .add(SimpleExpr::from(Expr::col((Instances, CurrentAttributes::DatasetId))).not_distinct_from(stored(CurrentAttributes::DatasetId))),
        )
        .take();
    Query::delete()
        .from_table(table_of(target, CurrentAttributes::Table))
        .and_where(stored(CurrentAttributes::EntityKey).in_subquery(Query::select().column(Entities::EntityKey).from(anchor).take()))
        .and_where(Expr::exists(staged).not())
        .take()
}

#[cfg(test)]
mod tests {
    use crate::current_attribute::write::{inserted_rows, replaced_rows, vanished_rows};
    use andromeda_context_space::context_target::ContextTarget;
    use sea_query::{Alias, PostgresQueryBuilder};

    fn anchor() -> Alias {
        Alias::new("ins")
    }

    #[test]
    fn the_insert_pairs_every_staged_row_with_its_entity_by_identifier() {
        let sql = inserted_rows(&ContextTarget::Live, anchor())
            .expect("the insert builds")
            .to_string(PostgresQueryBuilder);
        assert!(
            sql.starts_with(r#"INSERT INTO "current_attributes" ("entity_key", "attribute_name_key", "dataset_id", "ordinal""#),
            "{sql}"
        );
        assert!(sql.contains(r#"SELECT "ins"."entity_key", "instances"."attribute_name_key""#), "{sql}");
        assert!(sql.ends_with(r#"FROM "ins" JOIN "instances" ON "instances"."id" = "ins"."id""#), "{sql}");
        assert!(!sql.to_lowercase().contains("ordinality"), "{sql}");
    }

    /// A replaced row takes every staged column but the grain it is matched
    /// by: the state it was flattened from carries the `createdAt` the write
    /// keeps.
    #[test]
    fn a_replaced_row_takes_every_staged_column_but_its_grain() {
        let sql = replaced_rows(&ContextTarget::Live, anchor())
            .expect("the upsert builds")
            .to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"ON CONFLICT ("entity_key", "attribute_name_key", "dataset_id") DO UPDATE SET"#),
            "{sql}"
        );
        for column in ["ordinal", "value_kind", "created_at", "modified_at", "sub_attributes", "proof_seals"] {
            assert!(sql.contains(&format!(r#""{column}" = "excluded"."{column}""#)), "{column}: {sql}");
        }
        for grain in ["entity_key", "attribute_name_key", "dataset_id"] {
            assert!(!sql.contains(&format!(r#""{grain}" = "excluded"."{grain}""#)), "{grain}: {sql}");
        }
    }

    #[test]
    fn a_vanished_grain_is_found_by_its_whole_grain() {
        let sql = vanished_rows(&ContextTarget::Live, anchor()).to_string(PostgresQueryBuilder);
        assert!(
            sql.starts_with(r#"DELETE FROM "current_attributes" WHERE "current_attributes"."entity_key" IN (SELECT "entity_key" FROM "ins") AND (NOT EXISTS("#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""instances"."dataset_id" IS NOT DISTINCT FROM "current_attributes"."dataset_id""#),
            "{sql}"
        );
        assert!(sql.contains(r#""ins"."entity_key" = "current_attributes"."entity_key""#), "{sql}");
    }

    #[test]
    fn the_rows_land_in_the_branch_schema() {
        use andromeda_context_space::{snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
        let key = SnapshotKey::try_from(5).expect("positive key");
        let target = ContextTarget::Snapshot(SnapshotTarget::staging(key, "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri")));
        let insert = inserted_rows(&target, anchor()).expect("the insert builds").to_string(PostgresQueryBuilder);
        assert!(insert.contains(r#"INSERT INTO "snap_5_stg"."current_attributes""#), "{insert}");
        let delete = vanished_rows(&target, anchor()).to_string(PostgresQueryBuilder);
        assert!(delete.contains(r#"DELETE FROM "snap_5_stg"."current_attributes""#), "{delete}");
    }
}
