//! Writing the projection rows of the entities this transaction's own fused
//! insert created.
//!
//! An entity whose surrogate key this transaction minted owns no projection
//! row, committed or in flight, and no other transaction can write one (see
//! [`MintedEntityKey`]). Its rows therefore go in with a plain insert: no match
//! against stored grains, no row lock and no conflict arbitration, each of which
//! the general upsert pays per row. Repeats of one grain inside the batch are
//! still collapsed by the shared staging, the later row winning, exactly as the
//! upsert collapses them.

use crate::{
    current_attribute_values::{
        column_sets::{PROJECTION_COLUMNS, staging_columns},
        columns::ProjectionBinds,
        row::ProjectedAttributeRow,
        staging::{Input, Ordinality, Staging, input_cte},
    },
    entity::inserted_entity::MintedEntityKey,
    error::ContextStoreError,
    store_invariant::StoreInvariant,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::current::CurrentAttributeValues;
use andromeda_postgresql_seaquery::unnest::apply_with_ordinality;
use andromeda_telemetry::statement::{StatementName, observe_statement};
use sea_query::{PostgresQueryBuilder, Query, WithClause};
use sqlx::{AssertSqlSafe, Postgres, Transaction};

/// The statement name the minted-entity projection insert is observed under.
const CAV_MINTED_INSERT: StatementName = StatementName("cav-minted-insert");

/// The projection rows of entities this transaction minted, and only those.
#[derive(Debug, Default)]
pub struct MintedProjection {
    /// Rows whose entity key a [`MintedEntityKey`] vouched for.
    rows: Vec<ProjectedAttributeRow>,
}

impl MintedProjection {
    /// Add one projection row of the entity `entity` names.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Invariant`] when the row is keyed to an
    /// entity other than `entity`, which would let a row of an existing entity
    /// bypass the upsert.
    pub fn push(&mut self, entity: MintedEntityKey, row: ProjectedAttributeRow) -> Result<(), ContextStoreError> {
        if row.key.entity_key != entity.key() {
            return Err(StoreInvariant::ProjectionRowOfAnotherEntity {
                row: row.key.entity_key,
                minted: entity.key(),
            }
            .into());
        }
        self.rows.push(row);
        Ok(())
    }

    /// Whether no row was added.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Insert the projection rows of entities this transaction minted.
///
/// Runs in the caller's transaction, the one whose fused insert minted every
/// key the rows carry, so the rows and the entity they project commit together.
///
/// # Errors
///
/// Returns [`ContextStoreError::Encoding`] when a geometry cannot be encoded as
/// EWKB, and [`ContextStoreError::Database`] when the statement fails.
pub async fn insert_minted_projection_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    target: &ContextTarget,
    projection: &MintedProjection,
) -> Result<(), ContextStoreError> {
    if projection.is_empty() {
        return Ok(());
    }
    let binds = ProjectionBinds::from_rows(&projection.rows)?;
    let sql = minted_insert_sql(target)?;
    let statement = binds.bind_to(sqlx::query(AssertSqlSafe(sql.as_str()))).execute(&mut **tx);
    observe_statement(CAV_MINTED_INSERT, statement).await?;
    Ok(())
}

/// Build the minted-entity projection insert: the shared staging, then every
/// staged row inserted as it stands.
///
/// # Errors
///
/// Returns [`ContextStoreError::StatementBuild`] if the staged selection ever stops
/// matching the sixteen-wide column list, which the fixed shapes make
/// unreachable.
fn minted_insert_sql(target: &ContextTarget) -> Result<String, ContextStoreError> {
    let columns = staging_columns();
    let mut insert = Query::insert();
    insert.into_table(table_of(target, CurrentAttributeValues::Table)).columns(PROJECTION_COLUMNS);
    insert.select_from(Query::select().columns(PROJECTION_COLUMNS.map(|column| (Input, column))).from(Input).take())?;
    insert.with_cte(WithClause::new().cte(input_cte(&columns)).to_owned());
    let insert = insert;

    let sql = insert.to_string(PostgresQueryBuilder);
    Ok(apply_with_ordinality(sql, &Staging, &columns, &Ordinality))
}

#[cfg(test)]
mod tests {
    use crate::{
        current_attribute_values::{
            minted_insert::{MintedProjection, minted_insert_sql},
            row::{ProjectedAttributeKey, ProjectedAttributeRow, ProjectedAttributeValue},
        },
        entity::inserted_entity::MintedEntityKey,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_postgresql_codes::{attribute_kind::AttributeKind, entity_key::EntityKey};
    use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;

    fn rendered() -> String {
        minted_insert_sql(&ContextTarget::Live).expect("the fixed sixteen-wide insert builds")
    }

    fn row(entity_key: i64) -> ProjectedAttributeRow {
        ProjectedAttributeRow {
            key: ProjectedAttributeKey {
                entity_key: EntityKey::try_from(entity_key).expect("a positive surrogate key"),
                attribute_name_key: AttributeNameKey::from(3),
                dataset_id: None,
            },
            attribute_kind: AttributeKind::Property,
            value: ProjectedAttributeValue::Boolean(true),
            unit_code_id: None,
            expires_at: None,
        }
    }

    fn minted(entity_key: i64) -> MintedEntityKey {
        MintedEntityKey::new(EntityKey::try_from(entity_key).expect("a positive surrogate key"))
    }

    /// A minted entity owns no row to match or conflict with, so its rows are
    /// inserted without the upsert's lock, match or arbitration.
    #[test]
    fn the_minted_insert_neither_matches_nor_arbitrates() {
        let sql = rendered();
        assert!(sql.starts_with(r#"WITH "input""#), "{sql}");
        assert!(sql.contains(r#"INSERT INTO "current_attribute_values""#), "{sql}");
        for absent in ["ON CONFLICT", "FOR UPDATE", "matched", "ctid"] {
            assert!(!sql.contains(absent), "unexpected {absent}: {sql}");
        }
    }

    /// Repeats of one grain inside a batch are still collapsed, the later row
    /// winning, as the upsert collapses them.
    #[test]
    fn the_minted_insert_keeps_the_shared_last_row_wins_staging() {
        let sql = rendered();
        assert!(
            sql.contains(r#"DISTINCT ON ("d"."entity_key", "d"."attribute_name_key", "d"."dataset_id")"#),
            "{sql}"
        );
        assert!(sql.contains(r#""d"."ordinality" DESC"#), "{sql}");
        assert!(sql.contains("WITH ORDINALITY AS d("), "{sql}");
    }

    #[test]
    fn a_minted_insert_lands_in_the_snapshot_branch_schema() {
        use andromeda_context_space::{snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
        let key = SnapshotKey::try_from(5).expect("positive key");
        let target = ContextTarget::Snapshot(SnapshotTarget::staging(key, "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri")));
        let sql = minted_insert_sql(&target).expect("insert builds");
        assert!(sql.contains(r#"INSERT INTO "snap_5_stg"."current_attribute_values""#), "{sql}");
    }

    #[test]
    fn a_row_of_another_entity_cannot_join_a_minted_projection() {
        let mut projection = MintedProjection::default();
        projection.push(minted(1), row(1)).expect("the row's own minted key");
        assert!(projection.push(minted(1), row(2)).is_err());
        assert!(!projection.is_empty());
    }
}
