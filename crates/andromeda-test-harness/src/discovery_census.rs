//! The discovery census recomputed from the search rows, as the oracle the
//! integration suites hold the maintained census to.
//!
//! The broker keeps three ledgers of signed changes (`entity_type_census`,
//! `entity_type_attribute_census`, `attribute_census`) beside the search rows
//! its statements write, and every count they sum to must equal what the search
//! rows' type and attribute instance arrays add up to. The recomputation here is
//! written independently of the broker's statements — the instance arrays are
//! paired by a multi-argument `unnest` in `FROM` rather than by lockstep
//! set-returning functions — so the two agreeing is evidence, not tautology.
//!
//! Every statement names the branch schema it reads: `public` for the live
//! branch, `snap_<key>` for a published Snapshot branch.

use sqlx::{Postgres, Row, Transaction, postgres::PgPool};

/// One census ledger and the recomputation of the counts it must sum to.
struct LedgerOracle {
    /// The ledger relation.
    table: &'static str,
    /// The key columns, comma-separated.
    keys: &'static str,
    /// The signed change column.
    delta: &'static str,
    /// `SELECT <keys>, <count>` over the search rows of `{schema}`, grouped by
    /// the keys.
    recomputed: &'static str,
}

/// The three ledgers, each with its recomputation.
const LEDGERS: [LedgerOracle; 3] = [
    LedgerOracle {
        table: "entity_type_census",
        keys: "entity_type_name_key",
        delta: "entity_delta",
        recomputed: "SELECT t.entity_type_name_key, count(*) AS n \
                     FROM {schema}.entity_search s CROSS JOIN LATERAL unnest(s.entity_type_name_keys) AS t(entity_type_name_key) \
                     GROUP BY t.entity_type_name_key",
    },
    LedgerOracle {
        table: "entity_type_attribute_census",
        keys: "entity_type_name_key, attribute_name_key, attribute_kind",
        delta: "instance_delta",
        recomputed: "SELECT t.entity_type_name_key, i.attribute_name_key, i.attribute_kind, count(*) AS n \
                     FROM {schema}.entity_search s \
                     CROSS JOIN LATERAL unnest(s.entity_type_name_keys) AS t(entity_type_name_key) \
                     CROSS JOIN LATERAL unnest(s.attribute_instance_name_keys, s.attribute_instance_kinds) AS i(attribute_name_key, attribute_kind) \
                     GROUP BY t.entity_type_name_key, i.attribute_name_key, i.attribute_kind",
    },
    LedgerOracle {
        table: "attribute_census",
        keys: "attribute_name_key",
        delta: "instance_delta",
        recomputed: "SELECT i.attribute_name_key, count(*) AS n \
                     FROM {schema}.entity_search s CROSS JOIN LATERAL unnest(s.attribute_instance_name_keys) AS i(attribute_name_key) \
                     GROUP BY i.attribute_name_key",
    },
];

/// Every key of the census in `schema` whose maintained count differs from its
/// recomputed one, rendered `ledger(key…): maintained <m>, recomputed <r>`;
/// empty when the census is exact.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a comparison that fails.
pub async fn census_drift(pool: &PgPool, schema: &str) -> Result<Vec<String>, sqlx::Error> {
    let mut drift = Vec::new();
    for ledger in &LEDGERS {
        let LedgerOracle {
            table,
            keys,
            delta,
            recomputed,
        } = ledger;
        let recomputed = recomputed.replace("{schema}", schema);
        let sql = format!(
            "WITH maintained AS (SELECT {keys}, sum({delta})::bigint AS n FROM {schema}.{table} GROUP BY {keys} HAVING sum({delta}) <> 0), \
                  recomputed AS ({recomputed}) \
             SELECT concat_ws(',', {keys}) AS key, maintained.n AS maintained, recomputed.n AS recomputed \
             FROM maintained FULL JOIN recomputed USING ({keys}) \
             WHERE maintained.n IS DISTINCT FROM recomputed.n \
             ORDER BY 1"
        );
        for row in sqlx::query(sqlx::AssertSqlSafe(sql)).fetch_all(pool).await? {
            let key: String = row.try_get("key")?;
            let maintained: Option<i64> = row.try_get("maintained")?;
            let recomputed: Option<i64> = row.try_get("recomputed")?;
            drift.push(format!("{table}({key}): maintained {maintained:?}, recomputed {recomputed:?}"));
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
    let mut rows = 0;
    for ledger in &LEDGERS {
        let sql = format!("SELECT count(*) FROM {schema}.{}", ledger.table);
        rows += sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(sql)).fetch_one(pool).await?;
    }
    Ok(rows)
}

/// Replace the live census with its recomputation, in `transaction`.
///
/// Each ledger is replaced by one statement, so its delete and its
/// recomputation read the same snapshot: a writer that commits a change to a
/// search row after it keeps the census rows it added, and a writer that
/// committed before it is counted from the search rows instead. A cleanup that
/// removes Entities by a raw delete, which the broker's census never sees,
/// rebuilds the census this way so a concurrent test in the same tenant still
/// reads an exact one.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a rebuild that fails.
pub async fn rebuild_live_census(transaction: &mut Transaction<'_, Postgres>) -> Result<(), sqlx::Error> {
    for ledger in &LEDGERS {
        let LedgerOracle {
            table,
            keys,
            delta,
            recomputed,
        } = ledger;
        let recomputed = recomputed.replace("{schema}", "public");
        let sql = format!("WITH cleared AS (DELETE FROM {table}) INSERT INTO {table} ({keys}, {delta}) SELECT * FROM ({recomputed}) AS recomputed");
        sqlx::query(sqlx::AssertSqlSafe(sql)).execute(&mut **transaction).await?;
    }
    Ok(())
}
