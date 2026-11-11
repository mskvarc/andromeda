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
//! statements run together without touching a row twice. A move or a deletion
//! tests the stored rows against its staged grains as a hashed set, and a move
//! reads each new ordinal from its grains aggregated once, so neither joins the
//! stored rows to the grains and neither cost hangs on statistics that may
//! still describe the table before a large write.

use crate::{
    current_attribute::{
        staged_grains::Grains,
        staging::{Instances, written_columns},
    },
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::current::{CurrentAttributes, Entities};
use andromeda_postgresql_seaquery::{
    conflict_image::Excluded,
    hashed_membership::hashed_membership,
    keyed_lookup::{keyed_map, keyed_value},
    pg_array::any,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    scalar_subquery::scalar_subquery,
};
use sea_query::{
    DeleteStatement,
    DynIden,
    Expr,
    ExprTrait,
    Iden,
    InsertStatement,
    IntoIden,
    JoinType,
    OnConflict,
    Query,
    SelectStatement,
    SimpleExpr,
    UpdateStatement,
};

/// The identifiers of the Entities whose edit stages a grain, `edited`.
#[derive(Iden, Clone, Copy)]
#[iden = "edited"]
struct Edited;

/// What an update does to the stored row of a grain it stages.
#[derive(Clone, Copy)]
enum GrainEdit {
    /// The row takes the ordinal the grain carries.
    Moved,
    /// The row is deleted; the grain carries no ordinal.
    Removed,
}

impl GrainEdit {
    /// The grains staged for this edit.
    fn staged(self) -> SimpleExpr {
        let ordinal = Expr::col((Grains, CurrentAttributes::Ordinal));
        match self {
            GrainEdit::Moved => ordinal.is_not_null(),
            GrainEdit::Removed => ordinal.is_null(),
        }
    }
}

/// The columns a stored row takes from the row an update writes in its place:
/// every written column but the grain it is matched by and `created_at`, which
/// the stored row keeps (clause 4.8).
fn rewritten_columns() -> impl Iterator<Item = CurrentAttributes> {
    written_columns().filter(|column| {
        !matches!(
            column,
            CurrentAttributes::AttributeNameKey | CurrentAttributes::DatasetKey | CurrentAttributes::CreatedAt
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
    columns.extend(written_columns());
    let mut select = Query::select();
    select.expr(Expr::col((anchor.clone(), Entities::EntityKey)));
    for column in written_columns() {
        select.expr(Expr::col((Instances, column)));
    }
    select
        .from(anchor.clone())
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
/// `… ON CONFLICT (entity_key, attribute_name_key, dataset_key) DO UPDATE SET ordinal = excluded.ordinal, …
/// WHERE (current_attributes.ordinal, …) IS DISTINCT FROM (excluded.ordinal, …)`.
///
/// # Errors
///
/// Returns [`ContextStoreError::StatementBuild`] if the selection ever stops
/// matching the column list, which the shared column set makes unreachable.
pub(crate) fn written_rows(target: &ContextTarget, anchor: impl IntoIden) -> Result<InsertStatement, ContextStoreError> {
    let mut insert = inserted_rows(target, anchor)?;
    insert.on_conflict(
        OnConflict::columns([CurrentAttributes::EntityKey, CurrentAttributes::AttributeNameKey, CurrentAttributes::DatasetKey])
            .update_columns(rewritten_columns())
            .action_and_where(stored_row_differs())
            .to_owned(),
    );
    Ok(insert)
}

/// The rows an update keeps as they are but at another written position, for
/// every Entity `anchor` holds:
/// `UPDATE current_attributes SET ordinal = ((SELECT jsonb_object_agg((anchor.entity_key, grains.attribute_name_key, grains.dataset_key)::text, grains.ordinal) FROM anchor JOIN grains ON grains.id = anchor.id WHERE grains.ordinal IS NOT NULL)
/// ->> (entity_key, attribute_name_key, dataset_key)::text)::integer
/// WHERE entity_key IN (<the Entities moving a grain>) AND ((<the row's grain>) IN (<the moved grains>)) IS TRUE`.
///
/// The moved rows are found as the removed ones are ([`dropped_rows`]), and
/// each reads its new ordinal from the moved grains aggregated once into an
/// object keyed by grain ([`keyed_value`]), so no join between the stored rows
/// and the grains is left for stale statistics to plan as a rescan.
pub(crate) fn moved_rows(target: &ContextTarget, anchor: impl IntoIden) -> UpdateStatement {
    let anchor = anchor.into_iden();
    let ordinals = scalar_subquery(
        edited_grains(&anchor, GrainEdit::Moved)
            .expr(keyed_map(staged_grain_parts(&anchor), Expr::col((Grains, CurrentAttributes::Ordinal))))
            .take(),
    );
    Query::update()
        .table(table_of(target, CurrentAttributes::Table))
        .value(CurrentAttributes::Ordinal, keyed_value(ordinals, stored_grain_parts(), PostgresType::Integer))
        .and_where(stored(CurrentAttributes::EntityKey).in_subquery(edited_entities(&anchor, GrainEdit::Moved)))
        .and_where(hashed_membership(stored_grain().in_subquery(staged_grains(&anchor, GrainEdit::Moved))))
        .take()
}

/// The stored rows an update removes, for every Entity `anchor` holds:
/// `DELETE FROM current_attributes WHERE entity_key IN (SELECT anchor.entity_key FROM anchor JOIN (SELECT DISTINCT grains.id FROM grains WHERE grains.ordinal IS NULL) AS edited ON edited.id = anchor.id)
/// AND ((entity_key, attribute_name_key, dataset_key) IN (SELECT anchor.entity_key, grains.attribute_name_key, grains.dataset_key FROM anchor JOIN grains ON grains.id = anchor.id WHERE grains.ordinal IS NULL)) IS TRUE`.
///
/// Only the rows of an Entity that removes a grain are read, and each is
/// tested against the removed grains as a hashed set ([`hashed_membership`]),
/// so the deletion costs the rows of those Entities plus the grains whatever
/// the planner believes about either: right after a large write the statistics
/// of `current_attributes` can still read one row over pages that hold tens of
/// thousands. No member of either grain is NULL.
pub(crate) fn dropped_rows(target: &ContextTarget, anchor: impl IntoIden) -> DeleteStatement {
    let anchor = anchor.into_iden();
    Query::delete()
        .from_table(table_of(target, CurrentAttributes::Table))
        .and_where(stored(CurrentAttributes::EntityKey).in_subquery(edited_entities(&anchor, GrainEdit::Removed)))
        .and_where(hashed_membership(stored_grain().in_subquery(staged_grains(&anchor, GrainEdit::Removed))))
        .take()
}

/// The keys of the Entities `anchor` holds that stage a grain for `edit`:
/// `SELECT anchor.entity_key FROM anchor JOIN (SELECT DISTINCT grains.id FROM grains WHERE <edit>) AS edited ON edited.id = anchor.id`.
fn edited_entities(anchor: &DynIden, edit: GrainEdit) -> SelectStatement {
    let edited = Query::select()
        .distinct()
        .column((Grains, Entities::Id))
        .from(Grains)
        .and_where(edit.staged())
        .take();
    Query::select()
        .column((anchor.clone(), Entities::EntityKey))
        .from(anchor.clone())
        .join_subquery(
            JoinType::Join,
            edited,
            Edited,
            Expr::col((Edited, Entities::Id)).equals((anchor.clone(), Entities::Id)),
        )
        .take()
}

/// The grains staged for `edit`, each as its primary key:
/// `SELECT anchor.entity_key, grains.attribute_name_key, grains.dataset_key FROM anchor JOIN grains ON grains.id = anchor.id WHERE <edit>`.
fn staged_grains(anchor: &DynIden, edit: GrainEdit) -> SelectStatement {
    edited_grains(anchor, edit).exprs(staged_grain_parts(anchor)).take()
}

/// The grains staged for `edit`, each paired with its Entity, with nothing
/// selected yet: `… FROM anchor JOIN grains ON grains.id = anchor.id WHERE <edit>`.
fn edited_grains(anchor: &DynIden, edit: GrainEdit) -> SelectStatement {
    Query::select()
        .from(anchor.clone())
        .join(JoinType::Join, Grains, Expr::col((Grains, Entities::Id)).equals((anchor.clone(), Entities::Id)))
        .and_where(edit.staged())
        .take()
}

/// A staged grain's primary key, its Entity's key from `anchor`:
/// `anchor.entity_key, grains.attribute_name_key, grains.dataset_key`.
fn staged_grain_parts(anchor: &DynIden) -> [SimpleExpr; 3] {
    [
        Expr::col((anchor.clone(), Entities::EntityKey)),
        Expr::col((Grains, CurrentAttributes::AttributeNameKey)),
        Expr::col((Grains, CurrentAttributes::DatasetKey)),
    ]
}

/// The stored rows of every Entity `anchor` holds whose identifier is in
/// `overwritten` and whose grain (clause 4.5.5) no staged row of that Entity
/// has:
/// `DELETE FROM current_attributes WHERE entity_key IN (SELECT entity_key FROM anchor WHERE id = ANY(<overwritten>))
/// AND (entity_key, attribute_name_key, dataset_key) NOT IN (SELECT anchor.entity_key, instances.attribute_name_key, instances.dataset_key FROM instances JOIN anchor ON anchor.id = instances.id)`.
///
/// The rows an update writes for an Entity it overwrites are every row that
/// Entity keeps, so a stored grain none of them has is one the written state
/// no longer holds. PostgreSQL never turns `NOT IN` into a join: each row of
/// the overwritten Entities is tested against the staged grains as a hashed
/// set, whatever the planner believes about either side. No member of either
/// grain is NULL, so `NOT IN` is the plain negation of membership.
pub(crate) fn vanished_rows(target: &ContextTarget, anchor: impl IntoIden, overwritten: SimpleExpr) -> DeleteStatement {
    let anchor = anchor.into_iden();
    let staged = Query::select()
        .column((anchor.clone(), Entities::EntityKey))
        .column((Instances, CurrentAttributes::AttributeNameKey))
        .column((Instances, CurrentAttributes::DatasetKey))
        .from(Instances)
        .join(
            JoinType::Join,
            anchor.clone(),
            Expr::col((anchor.clone(), Entities::Id)).equals((Instances, Entities::Id)),
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
        .and_where(stored_grain().not_in_subquery(staged))
        .take()
}

/// A stored row's grain, its primary key:
/// `(entity_key, attribute_name_key, dataset_key)`.
fn stored_grain() -> SimpleExpr {
    Expr::tuple(stored_grain_parts())
}

/// The parts of a stored row's grain: `entity_key, attribute_name_key, dataset_key`.
fn stored_grain_parts() -> [SimpleExpr; 3] {
    [
        stored(CurrentAttributes::EntityKey),
        stored(CurrentAttributes::AttributeNameKey),
        stored(CurrentAttributes::DatasetKey),
    ]
}

/// A stored row's column, `current_attributes.<column>`.
fn stored(column: CurrentAttributes) -> SimpleExpr {
    Expr::col((CurrentAttributes::Table, column))
}

#[cfg(test)]
mod tests {
    use crate::current_attribute::write::{dropped_rows, inserted_rows, moved_rows, vanished_rows, written_rows};
    use andromeda_context_space::context_target::ContextTarget;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, SimpleExpr};

    fn anchor() -> Alias {
        Alias::new("ins")
    }

    /// The overwritten Entities as the one bound text array the update binds.
    fn overwritten() -> SimpleExpr {
        Expr::val(vec!["urn:ngsi-ld:Building:1".to_owned()])
    }

    #[test]
    fn the_insert_pairs_every_staged_row_with_its_entity_by_identifier() {
        let sql = inserted_rows(&ContextTarget::Live, anchor())
            .expect("the insert builds")
            .to_string(PostgresQueryBuilder);
        assert!(
            sql.starts_with(r#"INSERT INTO "current_attributes" ("entity_key", "attribute_name_key", "dataset_key", "ordinal""#),
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
            sql.contains(r#"ON CONFLICT ("entity_key", "attribute_name_key", "dataset_key") DO UPDATE SET"#),
            "{sql}"
        );
        for column in ["ordinal", "value_kind", "modified_at", "sub_attributes", "sub_eq_keys"] {
            assert!(sql.contains(&format!(r#""{column}" = "excluded"."{column}""#)), "{column}: {sql}");
        }
        for kept in ["entity_key", "attribute_name_key", "dataset_key", "created_at"] {
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

    /// The repaired form of the written geometry is inserted beside it and
    /// replaced with it, so a rewritten geometry never keeps a stale repair.
    #[test]
    fn the_upsert_writes_and_refreshes_the_repaired_geometry() {
        let sql = written_rows(&ContextTarget::Live, anchor())
            .expect("the upsert builds")
            .to_string(PostgresQueryBuilder);
        let (columns, _) = sql.split_once(") SELECT ").expect("the insert lists its columns");
        assert!(columns.contains(r#""value_geometry", "#), "{sql}");
        assert!(columns.contains(r#""value_geometry_valid""#), "{sql}");
        assert!(sql.contains(r#""instances"."value_geometry_valid""#), "{sql}");
        assert!(sql.contains(r#""value_geometry_valid" = "excluded"."value_geometry_valid""#), "{sql}");
    }

    /// A row that only moves takes its new ordinal and nothing else. Only the
    /// Entities that move a grain are read, their rows are picked by hashed
    /// membership, and each reads its ordinal from the moved grains aggregated
    /// once, so no join pairs the stored rows with the grains.
    #[test]
    fn a_moved_row_reads_its_new_ordinal_by_its_grain() {
        let sql = moved_rows(&ContextTarget::Live, anchor()).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"UPDATE "current_attributes" SET "ordinal" = "#,
                r#"((SELECT jsonb_object_agg(("ins"."entity_key", "grains"."attribute_name_key", "grains"."dataset_key")::text, "grains"."ordinal") "#,
                r#"FROM "ins" JOIN "grains" ON "grains"."id" = "ins"."id" WHERE "grains"."ordinal" IS NOT NULL) "#,
                r#"->> (("current_attributes"."entity_key", "current_attributes"."attribute_name_key", "current_attributes"."dataset_key")::text))::integer "#,
                r#"WHERE "current_attributes"."entity_key" IN "#,
                r#"(SELECT "ins"."entity_key" FROM "ins" JOIN (SELECT DISTINCT "grains"."id" FROM "grains" WHERE "grains"."ordinal" IS NOT NULL) AS "edited" ON "edited"."id" = "ins"."id") "#,
                r#"AND ((("current_attributes"."entity_key", "current_attributes"."attribute_name_key", "current_attributes"."dataset_key") IN "#,
                r#"(SELECT "ins"."entity_key", "grains"."attribute_name_key", "grains"."dataset_key" FROM "ins" JOIN "grains" ON "grains"."id" = "ins"."id" WHERE "grains"."ordinal" IS NOT NULL)) IS TRUE)"#
            )
        );
    }

    /// Clause 4.5.5: deleting one dataset instance deletes the row of that
    /// grain. Only the Entities that remove a grain are read, and their rows
    /// are tested against the removed grains by membership wrapped in
    /// `IS TRUE`, which PostgreSQL hashes instead of joining.
    #[test]
    fn a_vanished_grain_is_deleted_by_hashed_membership() {
        let sql = dropped_rows(&ContextTarget::Live, anchor()).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"DELETE FROM "current_attributes" WHERE "current_attributes"."entity_key" IN "#,
                r#"(SELECT "ins"."entity_key" FROM "ins" JOIN (SELECT DISTINCT "grains"."id" FROM "grains" WHERE "grains"."ordinal" IS NULL) AS "edited" ON "edited"."id" = "ins"."id") "#,
                r#"AND ((("current_attributes"."entity_key", "current_attributes"."attribute_name_key", "current_attributes"."dataset_key") IN "#,
                r#"(SELECT "ins"."entity_key", "grains"."attribute_name_key", "grains"."dataset_key" FROM "ins" JOIN "grains" ON "grains"."id" = "ins"."id" WHERE "grains"."ordinal" IS NULL)) IS TRUE)"#
            )
        );
    }

    /// An overwritten Entity's stored grain no staged row has is removed by
    /// its whole grain, for the overwritten Entities alone, through `NOT IN`,
    /// which PostgreSQL hashes instead of joining.
    #[test]
    fn a_grain_an_overwrite_does_not_restate_is_removed_by_hashed_membership() {
        let (sql, values) = vanished_rows(&ContextTarget::Live, anchor(), overwritten()).build(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(values.0.len(), 1, "{sql}");
        assert_eq!(
            sql,
            concat!(
                r#"DELETE FROM "current_attributes" WHERE "current_attributes"."entity_key" IN (SELECT "entity_key" FROM "ins" WHERE "id" = ANY($1)) "#,
                r#"AND ("current_attributes"."entity_key", "current_attributes"."attribute_name_key", "current_attributes"."dataset_key") NOT IN "#,
                r#"(SELECT "ins"."entity_key", "instances"."attribute_name_key", "instances"."dataset_key" FROM "instances" JOIN "ins" ON "ins"."id" = "instances"."id")"#
            )
        );
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
            vanished_rows(&target, anchor(), overwritten()).to_string(PostgresQueryBuilder),
        ] {
            assert!(delete.contains(r#"DELETE FROM "snap_5_stg"."current_attributes""#), "{delete}");
        }
    }
}
