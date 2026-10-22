//! Extended statistics: correlated columns carry statistics the planner uses.
//!
//! `attribute_name_key`, `value_kind` and `entity_key` are strongly correlated,
//! and the planner assumes independence unless told otherwise. The declared
//! statistics objects must exist where they take effect, must actually hold
//! collected data, and must measurably improve the row estimate of a correlated
//! predicate.

use crate::seeded_corpus::{SEED_PREFIX, analyze as analyze_corpus, seed};
use andromeda_postgresql_seaquery::{
    cast::cast,
    catalog_function::generate_series,
    command::Command,
    explain::Explain,
    maintenance::analyze,
    pg_array::array_of_subquery,
    pg_type::PostgresType,
    statement::Statement,
    statistics_object::{CreateStatistics, StatisticsKind},
    system_catalog::PgClass,
    temporary_table::{ColumnConstraint, TemporaryColumn, TemporaryTable},
    unnest::{UnnestAliased, UnnestColumn, unnest_call},
};
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use pretty_assertions::assert_eq;
use sea_query::{Alias, Asterisk, Expr, ExprTrait, InsertStatement, IntoIden, Query, TableName};
use sqlx::PgConnection;

/// The correlated two-column fixture relation.
const FIXTURE_TABLE: &str = "correlated_statistics_fixture";

/// The correlated two-column fixture relation.
fn fixture_table() -> TemporaryTable {
    TemporaryTable::new(
        Alias::new(FIXTURE_TABLE),
        vec![
            TemporaryColumn::new(Alias::new("attribute_name_key"), PostgresType::Integer, ColumnConstraint::NotNull),
            TemporaryColumn::new(Alias::new("value_kind"), PostgresType::Smallint, ColumnConstraint::NotNull),
        ],
    )
}

/// The fixture data. A perfect functional dependency: the value kind is a
/// function of the attribute, exactly the shape a compiled `q` predicate
/// probes.
fn fixture_rows() -> InsertStatement {
    let remainder = || Expr::expr(Expr::col(Alias::new("g")).modulo(50));
    Query::insert()
        .into_table(Alias::new(FIXTURE_TABLE))
        .columns(["attribute_name_key", "value_kind"].map(Alias::new))
        .select_from(
            Query::select()
                .expr(remainder().add(1))
                .expr(Expr::expr(remainder().modulo(7)).add(1))
                .from_function(generate_series(Expr::val(1), Expr::val(50_000)), Alias::new("g"))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_owned()
}

/// Read the planner's estimated row count for a statement.
async fn estimated_rows(connection: &mut PgConnection, statement: impl Statement) -> f64 {
    let plan: Vec<String> = Explain::new(statement)
        .to_query_scalar()
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
    analyze_corpus(pool).await;

    for (name, columns) in [
        ("stx_ca_attribute_value_kind", vec!["attribute_name_key", "value_kind"]),
        ("stx_ca_entity_attribute", vec!["entity_key", "attribute_name_key"]),
    ] {
        let statistic = |column: &str| Expr::col((Alias::new("pg_statistic_ext"), Alias::new(column)));
        let key_columns = [UnnestColumn::new(Alias::new("attnum"), PostgresType::SmallintArray)];
        let (relation, kinds, over): (String, Vec<String>, Vec<String>) = UnnestAliased::new(
            Query::select()
                .column((PgClass::Table, PgClass::Relname))
                .expr(cast(statistic("stxkind"), PostgresType::TextArray))
                .expr(array_of_subquery(
                    Query::select()
                        .column((Alias::new("pg_attribute"), Alias::new("attname")))
                        .from_function(unnest_call([(&key_columns[0], statistic("stxkeys"))]), Alias::new("k"))
                        .inner_join(
                            Alias::new("pg_attribute"),
                            Expr::col((Alias::new("pg_attribute"), Alias::new("attrelid")))
                                .equals((Alias::new("pg_statistic_ext"), Alias::new("stxrelid")))
                                .and(Expr::col((Alias::new("pg_attribute"), Alias::new("attnum"))).equals((Alias::new("k"), Alias::new("attnum")))),
                        )
                        .to_owned(),
                ))
                .from(Alias::new("pg_statistic_ext"))
                .inner_join(
                    PgClass::Table,
                    Expr::col((PgClass::Table, PgClass::Oid)).equals((Alias::new("pg_statistic_ext"), Alias::new("stxrelid"))),
                )
                .and_where(statistic("stxname").eq(name))
                .to_owned(),
        )
        .with_columns(Alias::new("k"), &key_columns)
        .to_query_as()
        .fetch_one(pool)
        .await
        .expect("the statistics object must exist");
        assert_eq!(relation, "current_attributes", "{name} must be declared where it takes effect");
        assert!(kinds.contains(&"d".to_owned()), "{name} must carry dependencies: {kinds:?}");
        assert!(kinds.contains(&"f".to_owned()), "{name} must carry n-distinct: {kinds:?}");
        assert_eq!(over, columns, "{name} must cover the correlated pair");

        let data = |column: &str| Expr::col((Alias::new("pg_statistic_ext_data"), Alias::new(column)));
        let collected: bool = Query::select()
            .expr(data("stxddependencies").is_not_null().and(data("stxdndistinct").is_not_null()))
            .from(Alias::new("pg_statistic_ext"))
            .inner_join(
                Alias::new("pg_statistic_ext_data"),
                data("stxoid").equals((Alias::new("pg_statistic_ext"), Alias::new("oid"))),
            )
            .and_where(statistic("stxname").eq(name))
            .to_query_scalar()
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
    fixture_table()
        .to_query()
        .execute(&mut *transaction)
        .await
        .expect("fixture statement must succeed");
    fixture_rows()
        .to_query()
        .execute(&mut *transaction)
        .await
        .expect("fixture statement must succeed");
    let analyze_fixture = analyze(Alias::new("pg_temp"), Alias::new(FIXTURE_TABLE));
    analyze_fixture
        .to_query()
        .execute(&mut *transaction)
        .await
        .expect("fixture statement must succeed");

    // One thousand rows carry (1, 1); independence estimates roughly 50000/50 *
    // 1/7 of that, an order of magnitude low.
    let probe = || {
        Query::select()
            .column(Asterisk)
            .from(Alias::new(FIXTURE_TABLE))
            .and_where(Expr::col(Alias::new("attribute_name_key")).eq(1))
            .and_where(Expr::col(Alias::new("value_kind")).eq(1))
            .to_owned()
    };
    let actual = 1000.0_f64;
    let without = estimated_rows(&mut transaction, probe()).await;

    CreateStatistics::new(
        Alias::new("correlated_statistics_fixture_stx"),
        vec![StatisticsKind::Dependencies, StatisticsKind::Ndistinct],
        vec![Alias::new("attribute_name_key").into_iden(), Alias::new("value_kind").into_iden()],
        TableName::from(Alias::new(FIXTURE_TABLE)),
    )
    .to_query()
    .execute(&mut *transaction)
    .await
    .expect("statistics creation must succeed");
    analyze_fixture.to_query().execute(&mut *transaction).await.expect("analyze must succeed");
    let with = estimated_rows(&mut transaction, probe()).await;

    assert!(
        (with - actual).abs() < (without - actual).abs(),
        "extended statistics must bring the estimate closer to {actual}: without={without}, with={with}"
    );

    transaction.rollback().await.expect("rollback must succeed");

    scope.finish().await.expect("cleanup failed");
}
