//! The statements that write an Entity's `current_attributes` rows from the
//! `instances` and `grains` stagings, inside the statement that writes its
//! Entity row.
//!
//! Each reads an anchor: the common table expression holding the key and
//! identifier of every Entity row the statement wrote. Only the Entities the
//! anchor holds get rows, and a staged row or grain finds its Entity by
//! identifier. Every depth of an instance's sub-attribute tree travels inside
//! its row, so one statement writes an Entity of any nesting.
//!
//! An update touches only the rows its edit names
//! ([`RowEdit`](crate::current_attribute::row_edit::RowEdit)): the instances it
//! writes whole are upserted, the rows that only move take their new ordinal,
//! and the grains it removes are deleted. The four sets are disjoint, so the
//! statements run together without touching a row twice.

use crate::{
    current_attribute::{
        staged_grains::Grains,
        staging::{Instances, ROW_COLUMNS},
    },
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::current::{CurrentAttributes, Entities};
use andromeda_postgresql_seaquery::{conflict_image::Excluded, pg_array::any, pg_operator::PgBinaryOperator};
use sea_query::{Cond, DeleteStatement, DynIden, Expr, ExprTrait, InsertStatement, IntoIden, JoinType, OnConflict, Query, SimpleExpr, UpdateStatement};

/// The columns a stored row takes from the row an update writes in its place:
/// every staged column but the grain it is matched by and `created_at`, which
/// the stored row keeps (clause 4.8).
fn rewritten_columns() -> impl Iterator<Item = CurrentAttributes> {
    ROW_COLUMNS.into_iter().filter(|column| {
        !matches!(
            column,
            CurrentAttributes::AttributeNameKey | CurrentAttributes::DatasetId | CurrentAttributes::CreatedAt
        )
    })
}

/// The no-op guard of the upsert: the stored row's rewritten columns, as one
/// row value, are distinct from the proposed row's.
///
/// `IS DISTINCT FROM` over row values compares field by field with NULL
/// counting as a value, each field by its type's own equality: `numeric`
/// ignores scale, jsonb is structural, and the `sub_attribute[]` array compares
/// element by element, each element field by field. A row an update restates
/// equal leaves no new heap or index tuple.
fn stored_row_differs() -> SimpleExpr {
    let stored = Expr::tuple(rewritten_columns().map(|column| Expr::col((CurrentAttributes::Table, column))));
    let proposed = Expr::tuple(rewritten_columns().map(|column| Expr::col((Excluded, column))));
    stored.distinct_from(proposed)
}

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

/// The rows an update writes whole, for every Entity `anchor` holds: the
/// insert of [`inserted_rows`], a stored grain (clause 4.5.5) updated in place
/// with every staged column but its grain and `created_at`, unless it already
/// holds them:
/// `… ON CONFLICT (entity_key, attribute_name_key, dataset_id) DO UPDATE SET ordinal = excluded.ordinal, …
/// WHERE (current_attributes.ordinal, …) IS DISTINCT FROM (excluded.ordinal, …)`.
///
/// # Errors
///
/// Returns [`ContextStoreError::StatementBuild`] if the selection ever stops
/// matching the column list, which the shared column set makes unreachable.
pub(crate) fn written_rows(target: &ContextTarget, anchor: impl IntoIden) -> Result<InsertStatement, ContextStoreError> {
    let mut insert = inserted_rows(target, anchor)?;
    insert.on_conflict(
        OnConflict::columns([CurrentAttributes::EntityKey, CurrentAttributes::AttributeNameKey, CurrentAttributes::DatasetId])
            .update_columns(rewritten_columns())
            .action_and_where(stored_row_differs())
            .to_owned(),
    );
    Ok(insert)
}

/// The rows an update keeps as they are but at another written position, for
/// every Entity `anchor` holds:
/// `UPDATE current_attributes SET ordinal = grains.ordinal FROM anchor, grains
/// WHERE grains.id = anchor.id AND grains.ordinal IS NOT NULL AND <the row's grain is the staged one>`.
pub(crate) fn moved_rows(target: &ContextTarget, anchor: impl IntoIden) -> UpdateStatement {
    let anchor = anchor.into_iden();
    Query::update()
        .table(table_of(target, CurrentAttributes::Table))
        .value(CurrentAttributes::Ordinal, Expr::col((Grains, CurrentAttributes::Ordinal)))
        .from(anchor.clone()) // clone: the anchor names the source and both of its key columns
        .from(Grains)
        .cond_where(
            Cond::all()
                .add(Expr::col((Grains, CurrentAttributes::Ordinal)).is_not_null())
                .add(staged_grain_of_the_row(anchor)),
        )
        .take()
}

/// The stored rows an update removes, for every Entity `anchor` holds:
/// `DELETE FROM current_attributes WHERE entity_key IN (SELECT anchor.entity_key FROM anchor JOIN grains ON grains.id = anchor.id WHERE grains.ordinal IS NULL)
/// AND EXISTS (SELECT 1 FROM anchor, grains WHERE grains.ordinal IS NULL AND <the row's grain is the staged one>)`.
///
/// The leading key list reaches the Entities that remove a grain through the
/// grain index; an Entity that removes nothing is not read.
pub(crate) fn dropped_rows(target: &ContextTarget, anchor: impl IntoIden) -> DeleteStatement {
    let anchor = anchor.into_iden();
    let removed = || Expr::col((Grains, CurrentAttributes::Ordinal)).is_null();
    let removing = Query::select()
        .column((anchor.clone(), Entities::EntityKey)) // clone: the anchor is read by both subqueries
        .from(anchor.clone()) // clone: as above
        .join(JoinType::Join, Grains, Expr::col((Grains, Entities::Id)).equals((anchor.clone(), Entities::Id))) // clone: as above
        .and_where(removed())
        .take();
    let staged = Query::select()
        .expr(Expr::val(1))
        .from(anchor.clone()) // clone: as above
        .from(Grains)
        .cond_where(Cond::all().add(removed()).add(staged_grain_of_the_row(anchor)))
        .take();
    Query::delete()
        .from_table(table_of(target, CurrentAttributes::Table))
        .and_where(stored(CurrentAttributes::EntityKey).in_subquery(removing))
        .and_where(Expr::exists(staged))
        .take()
}

/// The stored rows of every Entity `anchor` holds whose identifier is in
/// `overwritten` and whose grain (clause 4.5.5) no staged row of that Entity
/// has:
/// `DELETE FROM current_attributes WHERE entity_key IN (SELECT entity_key FROM anchor WHERE id = ANY(<overwritten>))
/// AND NOT EXISTS (SELECT 1 FROM instances JOIN anchor ON anchor.id = instances.id
/// WHERE anchor.entity_key = current_attributes.entity_key AND instances.attribute_name_key = current_attributes.attribute_name_key
/// AND instances.dataset_id IS NOT DISTINCT FROM current_attributes.dataset_id)`.
///
/// The rows an update writes for an Entity it overwrites are every row that
/// Entity keeps, so a stored grain none of them has is one the written state
/// no longer holds.
pub(crate) fn vanished_rows(target: &ContextTarget, anchor: impl IntoIden, overwritten: SimpleExpr) -> DeleteStatement {
    let anchor = anchor.into_iden();
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
    let overwriting = Query::select()
        .column(Entities::EntityKey)
        .from(anchor)
        .and_where(Expr::col(Entities::Id).eq(any(overwritten)))
        .take();
    Query::delete()
        .from_table(table_of(target, CurrentAttributes::Table))
        .and_where(stored(CurrentAttributes::EntityKey).in_subquery(overwriting))
        .and_where(Expr::exists(staged).not())
        .take()
}

/// A stored row's column, `current_attributes.<column>`.
fn stored(column: CurrentAttributes) -> SimpleExpr {
    Expr::col((CurrentAttributes::Table, column))
}

/// The stored row is the one a staged grain names: the grain's Entity is the
/// anchor row it pairs with by identifier, and the row has the grain's
/// attribute name and `datasetId`.
fn staged_grain_of_the_row(anchor: DynIden) -> Cond {
    Cond::all()
        .add(Expr::col((Grains, Entities::Id)).equals((anchor.clone(), Entities::Id))) // clone: the anchor names both of its key columns
        .add(Expr::col((anchor, Entities::EntityKey)).equals((CurrentAttributes::Table, CurrentAttributes::EntityKey)))
        .add(Expr::col((Grains, CurrentAttributes::AttributeNameKey)).equals((CurrentAttributes::Table, CurrentAttributes::AttributeNameKey)))
        .add(SimpleExpr::from(Expr::col((Grains, CurrentAttributes::DatasetId))).not_distinct_from(stored(CurrentAttributes::DatasetId)))
}

#[cfg(test)]
mod tests {
    use crate::current_attribute::write::{dropped_rows, inserted_rows, moved_rows, vanished_rows, written_rows};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_postgresql_seaquery::placeholder::external_placeholder;
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

    /// Clause 4.8: a surviving grain is updated in place and keeps the
    /// `createdAt` it was stored with; every other staged column, its new
    /// ordinal and its element array included, replaces the stored one.
    #[test]
    fn a_surviving_grain_is_updated_in_place() {
        let sql = written_rows(&ContextTarget::Live, anchor())
            .expect("the upsert builds")
            .to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"ON CONFLICT ("entity_key", "attribute_name_key", "dataset_id") DO UPDATE SET"#),
            "{sql}"
        );
        for column in ["ordinal", "value_kind", "modified_at", "sub_attributes", "sub_eq_keys", "proof_seals"] {
            assert!(sql.contains(&format!(r#""{column}" = "excluded"."{column}""#)), "{column}: {sql}");
        }
        for kept in ["entity_key", "attribute_name_key", "dataset_id", "created_at"] {
            assert!(!sql.contains(&format!(r#""{kept}" = "excluded"."{kept}""#)), "{kept}: {sql}");
        }
    }

    /// A stored row the update restates equal is not rewritten: the guard
    /// compares every rewritten column, the element array among them, and no
    /// other.
    #[test]
    fn the_no_op_guard_compares_the_element_array() {
        let sql = written_rows(&ContextTarget::Live, anchor())
            .expect("the upsert builds")
            .to_string(PostgresQueryBuilder);
        let (_, guard) = sql.split_once(" WHERE ").expect("the conflict action is guarded");
        let (stored, proposed) = guard.split_once(" IS DISTINCT FROM ").expect("the guard compares two row values");
        for column in [
            "ordinal",
            "value_number",
            "value_object",
            "observed_at",
            "modified_at",
            "sub_attributes",
            "sub_eq_keys",
        ] {
            assert!(stored.contains(&format!(r#""current_attributes"."{column}""#)), "{column}: {sql}");
            assert!(proposed.contains(&format!(r#""excluded"."{column}""#)), "{column}: {sql}");
        }
        assert!(!guard.contains("created_at"), "{sql}");
    }

    /// A row that only moves takes its new ordinal and nothing else.
    #[test]
    fn a_moved_row_takes_only_its_new_ordinal() {
        let sql = moved_rows(&ContextTarget::Live, anchor()).to_string(PostgresQueryBuilder);
        assert!(
            sql.starts_with(r#"UPDATE "current_attributes" SET "ordinal" = "grains"."ordinal" FROM "ins", "grains" WHERE "grains"."ordinal" IS NOT NULL"#),
            "{sql}"
        );
        assert!(sql.contains(r#""grains"."id" = "ins"."id""#), "{sql}");
        assert!(sql.contains(r#""ins"."entity_key" = "current_attributes"."entity_key""#), "{sql}");
        assert!(
            sql.contains(r#""grains"."dataset_id" IS NOT DISTINCT FROM "current_attributes"."dataset_id""#),
            "{sql}"
        );
    }

    /// Clause 4.5.5: deleting one dataset instance deletes the row of that
    /// grain, reached through the Entities that remove one.
    #[test]
    fn a_vanished_grain_is_deleted() {
        let sql = dropped_rows(&ContextTarget::Live, anchor()).to_string(PostgresQueryBuilder);
        assert!(
            sql.starts_with(r#"DELETE FROM "current_attributes" WHERE "current_attributes"."entity_key" IN (SELECT "ins"."entity_key" FROM "ins" JOIN "grains" ON "grains"."id" = "ins"."id" WHERE "grains"."ordinal" IS NULL) AND EXISTS("#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""grains"."attribute_name_key" = "current_attributes"."attribute_name_key""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""grains"."dataset_id" IS NOT DISTINCT FROM "current_attributes"."dataset_id""#),
            "{sql}"
        );
    }

    /// An overwritten Entity's stored grain no staged row has is removed by
    /// its whole grain, for the overwritten Entities alone.
    #[test]
    fn a_grain_an_overwrite_does_not_restate_is_found_by_its_whole_grain() {
        let sql = vanished_rows(&ContextTarget::Live, anchor(), external_placeholder(9)).to_string(PostgresQueryBuilder);
        assert!(
            sql.starts_with(r#"DELETE FROM "current_attributes" WHERE "current_attributes"."entity_key" IN (SELECT "entity_key" FROM "ins" WHERE "id" = ANY($9)) AND (NOT EXISTS("#),
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
        let moved = moved_rows(&target, anchor()).to_string(PostgresQueryBuilder);
        assert!(moved.contains(r#"UPDATE "snap_5_stg"."current_attributes""#), "{moved}");
        for delete in [
            dropped_rows(&target, anchor()).to_string(PostgresQueryBuilder),
            vanished_rows(&target, anchor(), external_placeholder(1)).to_string(PostgresQueryBuilder),
        ] {
            assert!(delete.contains(r#"DELETE FROM "snap_5_stg"."current_attributes""#), "{delete}");
        }
    }
}
