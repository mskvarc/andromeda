//! Replacing each census key's rows with their sum.
//!
//! Every write adds its own rows to a ledger, so a ledger grows with the
//! writes that changed a type or an attribute instance. A fold keeps it near
//! one row per live key: in one statement per ledger it deletes the rows of
//! every key that has more than one and inserts their sum, or nothing when the
//! sum is zero. A key with a single row is left alone, so a tenant at rest folds
//! nothing and writes nothing.
//!
//! A fold is safe beside any number of writers and other folds. Its delete sees
//! the statement's snapshot, so a row a writer adds meanwhile is neither deleted
//! nor summed and still counts afterwards; a row another fold deleted first is
//! skipped by this one's delete, so it is summed exactly once. Every count reads
//! the same before and after. No read depends on a fold having run, only how
//! many rows it sums.

use crate::{
    error::ContextStoreError,
    notification_delivery::branches::{drainable_targets, is_reaped_branch},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::{
    census::{AttributeCensus, EntityTypeAttributeCensus, EntityTypeCensus},
    temporal::StreamTypeCensus,
};
use andromeda_postgresql_seaquery::{aggregate::count_all, hashed_membership::hashed_membership, literal::int_literal, statement::Statement};
use derive_more::{Add, AddAssign};
use sea_query::{CommonTableExpression, DynIden, Expr, ExprTrait, Func, Iden, IntoIden, Query, WithClause, WithQuery};
use sqlx::{PgPool, Row};

/// The deleted rows a fold sums, `moved`.
#[derive(Iden, Clone, Copy)]
#[iden = "moved"]
struct Moved;

/// One census ledger: its relation, the columns its count is keyed by and the
/// column holding the signed change.
struct Ledger {
    /// The ledger relation.
    table: DynIden,
    /// The key columns, in declaration order.
    keys: Vec<DynIden>,
    /// The signed change column.
    delta: DynIden,
}

impl Ledger {
    /// The four census ledgers: the three of discovery and the stream type
    /// census a temporal selection by attribute alone reads.
    fn all() -> [Ledger; 4] {
        [
            Ledger {
                table: EntityTypeCensus::Table.into_iden(),
                keys: vec![EntityTypeCensus::EntityTypeNameKey.into_iden()],
                delta: EntityTypeCensus::EntityDelta.into_iden(),
            },
            Ledger {
                table: EntityTypeAttributeCensus::Table.into_iden(),
                keys: vec![
                    EntityTypeAttributeCensus::EntityTypeNameKey.into_iden(),
                    EntityTypeAttributeCensus::AttributeNameKey.into_iden(),
                    EntityTypeAttributeCensus::AttributeKind.into_iden(),
                ],
                delta: EntityTypeAttributeCensus::InstanceDelta.into_iden(),
            },
            Ledger {
                table: AttributeCensus::Table.into_iden(),
                keys: vec![AttributeCensus::AttributeNameKey.into_iden()],
                delta: AttributeCensus::InstanceDelta.into_iden(),
            },
            Ledger {
                table: StreamTypeCensus::Table.into_iden(),
                keys: vec![StreamTypeCensus::EntityTypeNameKey.into_iden(), StreamTypeCensus::AttributeNameKey.into_iden()],
                delta: StreamTypeCensus::StreamDelta.into_iden(),
            },
        ]
    }

    /// The fold of this ledger in `target`'s branch:
    ///
    /// ```sql
    /// WITH moved AS (DELETE FROM ledger WHERE ((key…) IN
    ///                  (SELECT key… FROM ledger GROUP BY key… HAVING COUNT(*) > 1)) IS TRUE
    ///                RETURNING key…, delta)
    /// INSERT INTO ledger (key…, delta)
    /// SELECT key…, SUM(delta) FROM moved GROUP BY key… HAVING SUM(delta) <> 0
    /// RETURNING delta
    /// ```
    ///
    /// The key test is a hashed subplan ([`hashed_membership`]), so the grouped
    /// keys are built once whatever the planner believes about the ledger's size.
    /// Every key column is `NOT NULL`, which the wrapper requires.
    ///
    /// The summed rows come back so a sum below zero, which only a census that
    /// drifted from its Entities can hold, is reported rather than folded away
    /// unnoticed.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::StatementBuild`] if the insert ever stops matching
    /// its column list, which the fixed shape makes unreachable.
    fn fold(&self, target: &ContextTarget) -> Result<WithQuery, ContextStoreError> {
        let key_tuple = || Expr::tuple(self.keys.iter().map(|key| Expr::col(key.clone()))); // clone: the tuple owns the idens it names.
        let mut crowded = Query::select();
        for key in &self.keys {
            crowded.column(key.clone()).group_by_col(key.clone()); // clone: the select and its grouping each own the key.
        }
        crowded.from(table_of(target, self.table.clone())).and_having(count_all().gt(int_literal(1))); // clone: the relation is named once per statement.
        let mut moved = Query::delete();
        moved
            .from_table(table_of(target, self.table.clone())) // clone: as above.
            .and_where(hashed_membership(key_tuple().in_subquery(crowded.take())))
            .returning(Query::returning().columns(self.keys.iter().cloned().chain([self.delta.clone()]))); // clone: the returning list owns its idens.
        let total = || Func::sum(Expr::col((Moved, self.delta.clone()))); // clone: the sum is written twice.
        let mut summed = Query::select();
        for key in &self.keys {
            summed.column((Moved, key.clone())).group_by_col((Moved, key.clone())); // clone: as above.
        }
        summed.expr(total()).from(Moved).and_having(total().ne(int_literal(0)));
        let mut insert = Query::insert();
        insert
            .into_table(table_of(target, self.table.clone())) // clone: as above.
            .columns(self.keys.iter().cloned().chain([self.delta.clone()])) // clone: the column list owns its idens.
            .select_from(summed.take())?
            .returning(Query::returning().column(self.delta.clone())); // clone: as above.
        Ok(WithClause::new()
            .cte(CommonTableExpression::new().table_name(Moved).query(moved).to_owned())
            .to_owned()
            .query(insert))
    }
}

/// What one fold of a branch's census did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Add, AddAssign)]
pub struct CensusFold {
    /// The rows the fold wrote in place of the ones it deleted.
    pub summed_rows: u64,
    /// The keys whose rows summed below zero, which a census kept by its
    /// statements never holds.
    pub negative_sums: u64,
}

/// Fold every census ledger of `target`'s branch, one statement per ledger in
/// one transaction.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when a fold cannot be built or fails.
pub async fn fold_census(pool: &PgPool, target: &ContextTarget) -> Result<CensusFold, ContextStoreError> {
    let mut tx = pool.begin().await?;
    let mut outcome = CensusFold::default();
    for ledger in Ledger::all() {
        let rows = ledger.fold(target)?.to_query().fetch_all(&mut *tx).await?;
        for row in rows {
            let sum: i64 = row.try_get(0)?;
            outcome.summed_rows += 1;
            if sum < 0 {
                outcome.negative_sums += 1;
            }
        }
    }
    tx.commit().await?;
    if outcome.negative_sums > 0 {
        tracing::error!(negative_sums = outcome.negative_sums, "the discovery census holds counts below zero");
    }
    Ok(outcome)
}

/// Fold the census of the live branch and of every published Snapshot branch,
/// each Snapshot branch being writable through the Core API.
///
/// A Snapshot reaped between the enumeration and its fold is skipped: its
/// census went with its schema.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when the branches cannot be enumerated or a
/// fold of a branch that still exists fails.
pub async fn fold_every_census(pool: &PgPool) -> Result<CensusFold, ContextStoreError> {
    let mut total = CensusFold::default();
    for target in drainable_targets(pool).await? {
        match fold_census(pool, &target).await {
            Ok(outcome) => total += outcome,
            Err(error) if is_reaped_branch(&error) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use crate::entity::census::fold::Ledger;
    use andromeda_context_space::context_target::ContextTarget;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn a_fold_sums_only_the_keys_with_more_than_one_row() {
        let [types, type_attributes, attributes, streams] = Ledger::all();
        let sql = type_attributes.fold(&ContextTarget::Live).expect("builds").to_string(PostgresQueryBuilder);
        assert!(
            sql.starts_with(r#"WITH "moved" AS (DELETE FROM "entity_type_attribute_census" WHERE (("entity_type_name_key", "attribute_name_key", "attribute_kind") IN (SELECT "entity_type_name_key", "attribute_name_key", "attribute_kind" FROM "entity_type_attribute_census" GROUP BY "entity_type_name_key", "attribute_name_key", "attribute_kind" HAVING (COUNT(*)) > (1))) IS TRUE RETURNING "entity_type_name_key", "attribute_name_key", "attribute_kind", "instance_delta")"#),
            "{sql}"
        );
        assert!(
            sql.ends_with(r#"FROM "moved" GROUP BY "moved"."entity_type_name_key", "moved"."attribute_name_key", "moved"."attribute_kind" HAVING SUM("moved"."instance_delta") <> (0) RETURNING "instance_delta""#),
            "{sql}"
        );
        for ledger in [types, attributes, streams] {
            let sql = ledger.fold(&ContextTarget::Live).expect("builds").to_string(PostgresQueryBuilder);
            assert!(sql.contains("HAVING (COUNT(*)) > (1))) IS TRUE RETURNING"), "{sql}");
        }
    }

    #[test]
    fn a_snapshot_branch_folds_its_own_census() {
        use andromeda_context_space::{snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
        let key = SnapshotKey::try_from(5).expect("positive key");
        let target = ContextTarget::Snapshot(SnapshotTarget::staging(key, "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri")));
        for ledger in Ledger::all() {
            let sql = ledger.fold(&target).expect("builds").to_string(PostgresQueryBuilder);
            assert_eq!(sql.matches(r#""snap_5_stg"."#).count(), 3, "{sql}");
        }
    }
}
