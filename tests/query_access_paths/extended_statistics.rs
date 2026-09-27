//! Extended statistics: correlated columns carry statistics the planner uses.
//!
//! `attribute_name_key`, `value_kind` and `entity_key` are strongly correlated,
//! and the planner assumes independence unless told otherwise. The declared
//! statistics objects must exist where they take effect, must actually hold
//! collected data, and must measurably improve the row estimate of a correlated
//! predicate.

use crate::seeded_corpus::{SEED_PREFIX, analyze, seed};
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use pretty_assertions::assert_eq;
use sqlx::{Executor, PgConnection};

/// Statements building a correlated two-column fixture, its data, and the
/// baseline statistics the planner has without an extended statistics object.
const CORRELATED_FIXTURE: [&str; 3] = [
    "CREATE TEMPORARY TABLE correlated_statistics_fixture (attribute_name_key INTEGER NOT NULL, value_kind SMALLINT NOT NULL) ON COMMIT DROP",
    // Perfect functional dependency: the value kind is a function of the
    // attribute, exactly the shape a compiled `q` predicate probes.
    "INSERT INTO correlated_statistics_fixture (attribute_name_key, value_kind) \
     SELECT (g % 50) + 1, ((g % 50) % 7) + 1 FROM generate_series(1, 50000) AS g",
    "ANALYZE correlated_statistics_fixture",
];

/// Read the planner's estimated row count for a statement.
async fn estimated_rows(connection: &mut PgConnection, statement: &str) -> f64 {
    let plan: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("EXPLAIN {statement}")))
        .fetch_all(&mut *connection)
        .await
        .expect("the plan must be readable");
    let first = plan.first().expect("a plan has at least one line").clone(); // clone: the estimate is parsed out of an owned line.
    let rows = first
        .split("rows=")
        .nth(1)
        .and_then(|tail| tail.split_whitespace().next())
        .expect("the top plan node reports an estimate");
    rows.parse().expect("the estimate is a number")
}

#[tokio::test]
async fn declared_statistics_objects_exist_and_hold_collected_data() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, SEED_PREFIX).await;
    assert_eq!(corpus.prefix(), SEED_PREFIX);
    analyze(pool).await;

    for (name, columns) in [
        ("stx_cav_attribute_value_kind", vec!["attribute_name_key", "value_kind"]),
        ("stx_cav_entity_attribute", vec!["entity_key", "attribute_name_key"]),
    ] {
        let (relation, kinds, over): (String, Vec<String>, Vec<String>) = sqlx::query_as(
            "SELECT c.relname, s.stxkind::text[], \
                    ARRAY(SELECT a.attname FROM unnest(s.stxkeys) AS k(attnum) \
                          JOIN pg_attribute AS a ON a.attrelid = s.stxrelid AND a.attnum = k.attnum) \
             FROM pg_statistic_ext AS s JOIN pg_class AS c ON c.oid = s.stxrelid WHERE s.stxname = $1",
        )
        .bind(name)
        .fetch_one(pool)
        .await
        .expect("the statistics object must exist");
        assert_eq!(relation, "current_attribute_values", "{name} must be declared where it takes effect");
        assert!(kinds.contains(&"d".to_owned()), "{name} must carry dependencies: {kinds:?}");
        assert!(kinds.contains(&"f".to_owned()), "{name} must carry n-distinct: {kinds:?}");
        assert_eq!(over, columns, "{name} must cover the correlated pair");

        let collected: bool = sqlx::query_scalar(
            "SELECT d.stxddependencies IS NOT NULL AND d.stxdndistinct IS NOT NULL \
             FROM pg_statistic_ext AS s JOIN pg_statistic_ext_data AS d ON d.stxoid = s.oid \
             WHERE s.stxname = $1",
        )
        .bind(name)
        .fetch_one(pool)
        .await
        .expect("statistics data must be readable");
        assert!(collected, "{name} must hold data after ANALYZE, not just a declaration");
    }

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn correlated_predicates_are_estimated_more_accurately_with_extended_statistics() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let mut transaction = scope.database_pool().begin().await.expect("transaction must start");
    for statement in CORRELATED_FIXTURE {
        transaction.execute(statement).await.expect("fixture statement must succeed");
    }

    // One thousand rows carry (1, 1); independence estimates roughly 50000/50 *
    // 1/7 of that, an order of magnitude low.
    let probe = "SELECT * FROM correlated_statistics_fixture WHERE attribute_name_key = 1 AND value_kind = 1";
    let actual = 1000.0_f64;
    let without = estimated_rows(&mut transaction, probe).await;

    transaction
        .execute(
            "CREATE STATISTICS correlated_statistics_fixture_stx (dependencies, ndistinct) ON attribute_name_key, value_kind FROM correlated_statistics_fixture",
        )
        .await
        .expect("statistics creation must succeed");
    transaction
        .execute("ANALYZE correlated_statistics_fixture")
        .await
        .expect("analyze must succeed");
    let with = estimated_rows(&mut transaction, probe).await;

    assert!(
        (with - actual).abs() < (without - actual).abs(),
        "extended statistics must bring the estimate closer to {actual}: without={without}, with={with}"
    );

    transaction.rollback().await.expect("rollback must succeed");

    scope.finish().await.expect("cleanup failed");
}
