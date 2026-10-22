//! The discovery census recomputed from the search rows, as the oracle the
//! integration suites hold the maintained census to.
//!
//! The broker keeps three ledgers of signed changes (`entity_type_census`,
//! `entity_type_attribute_census`, `attribute_census`) beside the search rows
//! its statements write, and every count they sum to must equal what the search
//! rows' type and attribute instance arrays add up to. The recomputation here is
//! written independently of the broker's statements: the instance arrays are
//! paired by a multi-argument `unnest` in `FROM` rather than by lockstep
//! set-returning functions, so the two agreeing is evidence, not tautology.
//!
//! Every statement names the branch schema it reads: `public` for the live
//! branch, `snap_<key>` for a published Snapshot branch.

use andromeda_postgresql_schema::{
    census::{AttributeCensus, EntityTypeAttributeCensus, EntityTypeCensus},
    current::EntitySearch,
};
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    cast::cast,
    pg_type::PostgresType,
    statement::Statement,
    unnest::{UnnestAliased, UnnestColumn, unnest_call},
};
use sea_query::{
    Alias,
    CommonTableExpression,
    Condition,
    DynIden,
    Expr,
    ExprTrait,
    Func,
    Iden,
    IntoIden,
    JoinType,
    Order,
    Query,
    SelectStatement,
    WithClause,
    WithQuery,
    error::Error as SeaQueryError,
};
use sqlx::{Postgres, Row, Transaction, postgres::PgPool};
use thiserror::Error;

/// The unnest of the entity type keys, `t`.
#[derive(Iden, Clone, Copy)]
#[iden = "t"]
struct TypeUnnest;

/// The unnest of the attribute instances, `i`.
#[derive(Iden, Clone, Copy)]
#[iden = "i"]
struct InstanceUnnest;

/// The search row an unnest reads, `s`.
#[derive(Iden, Clone, Copy)]
#[iden = "s"]
struct SearchRow;

/// The maintained side of a comparison, `m`.
#[derive(Iden, Clone, Copy)]
#[iden = "m"]
struct Maintained;

/// The recomputed side of a comparison, `r`.
#[derive(Iden, Clone, Copy)]
#[iden = "r"]
struct Recomputed;

/// The count column of either side, `n`.
#[derive(Iden, Clone, Copy)]
#[iden = "n"]
struct Count;

/// The maintained count a comparison projects, `maintained`.
#[derive(Iden, Clone, Copy)]
#[iden = "maintained"]
struct MaintainedCount;

/// The recomputed count a comparison projects, `recomputed`.
#[derive(Iden, Clone, Copy)]
#[iden = "recomputed"]
struct RecomputedCount;

/// The rows a replacing statement deletes before it inserts, `cleared`.
#[derive(Iden, Clone, Copy)]
#[iden = "cleared"]
struct Cleared;

/// The census oracle could not run.
#[derive(Debug, Error)]
pub enum CensusError {
    /// A statement failed.
    #[error("a census statement failed: {0}")]
    Database(#[from] sqlx::Error),
    /// A statement could not be assembled.
    #[error("a census statement could not be built: {0}")]
    Statement(#[from] SeaQueryError),
}

/// The `UNNEST` sources of a recomputation, whose aliases name columns.
struct UnnestAliases(Vec<(DynIden, Vec<UnnestColumn>)>);

impl UnnestAliases {
    /// `statement`, which embeds the recomputation, with the aliases spelled.
    fn spelled<S>(&self, statement: S) -> UnnestAliased<S> {
        self.0.iter().fold(UnnestAliased::new(statement), |aliased, (alias, columns)| {
            aliased.with_columns(alias.clone(), columns)
        }) // clone: the alias owns the name it spells
    }
}

/// A recomputation of one ledger's counts.
struct Recomputation {
    /// `SELECT <keys>, count(*)` over the search rows, grouped by the keys.
    select: SelectStatement,
    /// The aliases of its unnest sources.
    aliases: UnnestAliases,
}

/// One census ledger and the recomputation of the counts it must sum to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Ledger {
    /// Entities per type.
    EntityTypes,
    /// Attribute instances per type, attribute and family.
    EntityTypeAttributes,
    /// Attribute instances per attribute.
    Attributes,
}

impl Ledger {
    /// The three ledgers.
    const ALL: [Ledger; 3] = [Ledger::EntityTypes, Ledger::EntityTypeAttributes, Ledger::Attributes];

    /// The ledger relation.
    fn table(self) -> DynIden {
        match self {
            Ledger::EntityTypes => EntityTypeCensus::Table.into_iden(),
            Ledger::EntityTypeAttributes => EntityTypeAttributeCensus::Table.into_iden(),
            Ledger::Attributes => AttributeCensus::Table.into_iden(),
        }
    }

    /// The key columns.
    fn keys(self) -> Vec<DynIden> {
        match self {
            Ledger::EntityTypes => vec![EntityTypeCensus::EntityTypeNameKey.into_iden()],
            Ledger::EntityTypeAttributes => vec![
                EntityTypeAttributeCensus::EntityTypeNameKey.into_iden(),
                EntityTypeAttributeCensus::AttributeNameKey.into_iden(),
                EntityTypeAttributeCensus::AttributeKind.into_iden(),
            ],
            Ledger::Attributes => vec![AttributeCensus::AttributeNameKey.into_iden()],
        }
    }

    /// The signed change column.
    fn delta(self) -> DynIden {
        match self {
            Ledger::EntityTypes => EntityTypeCensus::EntityDelta.into_iden(),
            Ledger::EntityTypeAttributes => EntityTypeAttributeCensus::InstanceDelta.into_iden(),
            Ledger::Attributes => AttributeCensus::InstanceDelta.into_iden(),
        }
    }

    /// The counts the search rows of `schema` add up to.
    fn recomputed(self, schema: &DynIden) -> Recomputation {
        let type_column = UnnestColumn::new(EntityTypeCensus::EntityTypeNameKey, PostgresType::IntegerArray);
        let mut select = Query::select();
        select.from_as((schema.clone(), EntitySearch::Table), SearchRow); // clone: a table reference owns the schema it names
        let aliases = match self {
            Ledger::EntityTypes => {
                select
                    .column((TypeUnnest, EntityTypeCensus::EntityTypeNameKey))
                    .expr_as(count_all(), Count)
                    .from_function(
                        unnest_call([(&type_column, Expr::col((SearchRow, EntitySearch::EntityTypeNameKeys)))]),
                        TypeUnnest,
                    )
                    .group_by_col((TypeUnnest, EntityTypeCensus::EntityTypeNameKey));
                vec![(TypeUnnest.into_iden(), vec![type_column])]
            }
            Ledger::EntityTypeAttributes => {
                let instances = [
                    UnnestColumn::new(EntityTypeAttributeCensus::AttributeNameKey, PostgresType::IntegerArray),
                    UnnestColumn::new(EntityTypeAttributeCensus::AttributeKind, PostgresType::SmallintArray),
                ];
                select
                    .column((TypeUnnest, EntityTypeAttributeCensus::EntityTypeNameKey))
                    .column((InstanceUnnest, EntityTypeAttributeCensus::AttributeNameKey))
                    .column((InstanceUnnest, EntityTypeAttributeCensus::AttributeKind))
                    .expr_as(count_all(), Count)
                    .from_function(
                        unnest_call([(&type_column, Expr::col((SearchRow, EntitySearch::EntityTypeNameKeys)))]),
                        TypeUnnest,
                    )
                    .from_function(
                        unnest_call([
                            (&instances[0], Expr::col((SearchRow, EntitySearch::AttributeInstanceNameKeys))),
                            (&instances[1], Expr::col((SearchRow, EntitySearch::AttributeInstanceKinds))),
                        ]),
                        InstanceUnnest,
                    )
                    .group_by_columns([
                        (TypeUnnest.into_iden(), EntityTypeAttributeCensus::EntityTypeNameKey.into_iden()),
                        (InstanceUnnest.into_iden(), EntityTypeAttributeCensus::AttributeNameKey.into_iden()),
                        (InstanceUnnest.into_iden(), EntityTypeAttributeCensus::AttributeKind.into_iden()),
                    ]);
                vec![(TypeUnnest.into_iden(), vec![type_column]), (InstanceUnnest.into_iden(), instances.to_vec())]
            }
            Ledger::Attributes => {
                let instances = [UnnestColumn::new(AttributeCensus::AttributeNameKey, PostgresType::IntegerArray)];
                select
                    .column((InstanceUnnest, AttributeCensus::AttributeNameKey))
                    .expr_as(count_all(), Count)
                    .from_function(
                        unnest_call([(&instances[0], Expr::col((SearchRow, EntitySearch::AttributeInstanceNameKeys)))]),
                        InstanceUnnest,
                    )
                    .group_by_col((InstanceUnnest, AttributeCensus::AttributeNameKey));
                vec![(InstanceUnnest.into_iden(), instances.to_vec())]
            }
        };
        Recomputation {
            select,
            aliases: UnnestAliases(aliases),
        }
    }

    /// The counts the ledger's changes sum to in `schema`, keys with a net
    /// change of zero left out.
    fn maintained(self, schema: &DynIden) -> SelectStatement {
        let sum = || Func::sum(Expr::col(self.delta()));
        Query::select()
            .columns(self.keys())
            .expr_as(cast(sum().into(), PostgresType::Bigint), Count)
            .from((schema.clone(), self.table())) // clone: a table reference owns the schema it names
            .group_by_columns(self.keys())
            .and_having(Expr::expr(sum()).ne(0))
            .to_owned()
    }

    /// Every key whose maintained count differs from its recomputed one in
    /// `schema`: the key columns, then the maintained and the recomputed count,
    /// either absent when the key has no row on that side. The statement comes
    /// with the aliases its recomputation needs spelled.
    fn drift(self, schema: &DynIden) -> (SelectStatement, UnnestAliases) {
        let Recomputation { select, aliases } = self.recomputed(schema);
        let mut drift = Query::select();
        for key in self.keys() {
            let merged = Func::coalesce([Expr::col((Maintained, key.clone())), Expr::col((Recomputed, key.clone()))]); // clone: each reference owns the column it names
            drift.expr_as(cast(merged.into(), PostgresType::Bigint), key);
        }
        let same_key = self.keys().into_iter().fold(Condition::all(), |all, key| {
            all.add(Expr::col((Maintained, key.clone())).equals((Recomputed, key)))
        }); // clone: each reference owns the column it names
        let side_missing = Expr::col((Maintained, Count)).is_null().or(Expr::col((Recomputed, Count)).is_null());
        drift
            .expr_as(Expr::col((Maintained, Count)), MaintainedCount)
            .expr_as(Expr::col((Recomputed, Count)), RecomputedCount)
            .from_subquery(self.maintained(schema), Maintained)
            .join_subquery(JoinType::FullOuterJoin, select, Recomputed, same_key)
            .and_where(side_missing.or(Expr::col((Maintained, Count)).ne(Expr::col((Recomputed, Count)))));
        for key in self.keys() {
            drift.order_by(key, Order::Asc);
        }
        (drift, aliases)
    }

    /// The statement replacing the ledger of the live branch by its
    /// recomputation.
    ///
    /// The delete and the recomputation are one statement, so they read the
    /// same snapshot: a writer that commits a change to a search row after it
    /// keeps the census rows it added, and a writer that committed before it is
    /// counted from the search rows instead.
    fn rebuild(self) -> Result<(WithQuery, UnnestAliases), SeaQueryError> {
        let Recomputation { select, aliases } = self.recomputed(&Alias::new("public").into_iden());
        let cleared = Query::delete().from_table(self.table()).to_owned();
        let mut insert = Query::insert();
        insert
            .into_table(self.table())
            .columns(self.keys().into_iter().chain([self.delta()]))
            .select_from(select)?;
        let mut replacing = WithClause::new();
        replacing.cte(CommonTableExpression::new().table_name(Cleared).query(cleared).to_owned());
        Ok((replacing.query(insert), aliases))
    }
}

/// Every key of the census in `schema` whose maintained count differs from its
/// recomputed one, rendered `ledger(key…): maintained <m>, recomputed <r>`;
/// empty when the census is exact.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a comparison that fails.
pub async fn census_drift(pool: &PgPool, schema: &str) -> Result<Vec<String>, sqlx::Error> {
    let schema = Alias::new(schema).into_iden();
    let mut drift = Vec::new();
    for ledger in Ledger::ALL {
        let keys = ledger.keys();
        let (statement, aliases) = ledger.drift(&schema);
        for row in aliases.spelled(statement).to_query().fetch_all(pool).await? {
            let key = keys
                .iter()
                .map(|key| row.try_get::<i64, _>(key.to_string().as_str()).map(|value| value.to_string()))
                .collect::<Result<Vec<_>, _>>()?
                .join(",");
            let maintained: Option<i64> = row.try_get(MaintainedCount.to_string().as_str())?;
            let recomputed: Option<i64> = row.try_get(RecomputedCount.to_string().as_str())?;
            drift.push(format!("{}({key}): maintained {maintained:?}, recomputed {recomputed:?}", ledger.table()));
        }
    }
    Ok(drift)
}

/// The number of rows the three ledgers in `schema` hold, which a fold brings
/// down to about one per live key.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a count that fails.
pub async fn census_rows(pool: &PgPool, schema: &str) -> Result<i64, sqlx::Error> {
    let schema = Alias::new(schema).into_iden();
    let mut rows = 0;
    for ledger in Ledger::ALL {
        let count: i64 = Query::select()
            .expr(count_all())
            .from((schema.clone(), ledger.table())) // clone: a table reference owns the schema it names
            .to_query_scalar()
            .fetch_one(pool)
            .await?;
        rows += count;
    }
    Ok(rows)
}

/// Replace the live census with its recomputation, in `transaction`.
///
/// Each ledger is replaced by one statement, so its delete and its
/// recomputation read the same snapshot. A cleanup that removes Entities by a
/// raw delete, which the broker's census never sees, rebuilds the census this
/// way so a concurrent test in the same tenant still reads an exact one.
///
/// # Errors
///
/// Returns the [`CensusError`] of a rebuild that fails.
pub async fn rebuild_live_census(transaction: &mut Transaction<'_, Postgres>) -> Result<(), CensusError> {
    for ledger in Ledger::ALL {
        let (statement, aliases) = ledger.rebuild()?;
        aliases.spelled(statement).to_query().execute(&mut **transaction).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::discovery_census::Ledger;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, DynIden, IntoIden, PostgresQueryBuilder};

    fn public() -> DynIden {
        Alias::new("public").into_iden()
    }

    #[test]
    fn the_three_ledgers_are_recomputed_from_the_search_rows_of_the_schema() {
        for ledger in Ledger::ALL {
            let sql = ledger.recomputed(&public()).select.to_string(PostgresQueryBuilder);
            assert!(sql.contains(r#"FROM "public"."entity_search" AS "s""#), "{sql}");
            assert!(sql.contains("GROUP BY"), "{sql}");
        }
    }

    #[test]
    fn the_attribute_family_ledger_pairs_the_instance_arrays_in_one_multi_argument_unnest() {
        let sql = Ledger::EntityTypeAttributes.recomputed(&public()).select.to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"UNNEST("s"."attribute_instance_name_keys"::integer[], "s"."attribute_instance_kinds"::smallint[]) AS "i""#),
            "{sql}"
        );
    }

    #[test]
    fn a_ledger_sums_its_signed_changes_and_leaves_out_a_net_zero() {
        assert_eq!(
            Ledger::Attributes.maintained(&public()).to_string(PostgresQueryBuilder),
            r#"SELECT "attribute_name_key", SUM("instance_delta")::bigint AS "n" FROM "public"."attribute_census" GROUP BY "attribute_name_key" HAVING SUM("instance_delta") <> 0"#
        );
    }

    #[test]
    fn a_drift_compares_the_two_sides_by_key_and_reports_a_key_either_side_lacks() {
        let (statement, _) = Ledger::EntityTypes.drift(&public());
        let sql = statement.to_string(PostgresQueryBuilder);
        assert!(sql.contains("FULL OUTER JOIN"), "{sql}");
        assert!(sql.contains(r#""m"."entity_type_name_key" = "r"."entity_type_name_key""#), "{sql}");
        assert!(sql.contains(r#""m"."n" IS NULL OR "r"."n" IS NULL OR "m"."n" <> "r"."n""#), "{sql}");
        assert!(sql.ends_with(r#"ORDER BY "entity_type_name_key" ASC"#), "{sql}");
    }

    #[test]
    fn a_rebuild_deletes_and_inserts_the_ledger_in_one_statement() {
        for ledger in Ledger::ALL {
            let (statement, _) = ledger.rebuild().expect("the rebuild builds");
            let sql = statement.to_string(PostgresQueryBuilder);
            let table = ledger.table();
            assert!(
                sql.starts_with(&format!(r#"WITH "cleared" AS (DELETE FROM "{table}") INSERT INTO "{table}" ("#)),
                "{sql}"
            );
            assert!(sql.contains(r#"FROM "public"."entity_search" AS "s""#), "{sql}");
        }
    }
}
