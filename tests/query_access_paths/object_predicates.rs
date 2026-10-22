//! Object GIN operator class: every object predicate is one the index serves.
//!
//! `jsonb_path_ops` indexes containment (`@>`) and nothing else; the default
//! `jsonb_ops` class also serves key existence (`?`, `?|`, `?&`). Every
//! predicate compiled against `current_attributes.value_object` is a
//! containment test, so its GIN index uses the smaller `jsonb_path_ops` class.
//! The test states both halves: the two operator classes behave as declared, and
//! the `value_object` index is the containment one.

use crate::seeded_corpus::explain;
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    cast::cast,
    catalog_function::{generate_series, index_definition},
    command::Command,
    gin_index::{CreateGinIndex, JsonbOperatorClass},
    jsonb::jsonb_build_object,
    literal::text_literal,
    maintenance::analyze,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    statement::Statement,
    system_catalog::PgClass,
    temporary_table::{ColumnConstraint, TemporaryColumn, TemporaryTable},
};
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use pretty_assertions::assert_eq;
use sea_query::{Alias, Expr, ExprTrait, InsertStatement, Query, SelectStatement, TableName};

/// The two-index fixture relation whose plans distinguish the operator
/// classes. It lives in a temporary relation so the production schema keeps the
/// index set its consumers justify.
const FIXTURE_TABLE: &str = "object_predicate_fixture";

/// The fixture relation, rows `1..=20000` each holding the object
/// `{"city": "city-<g % 5000>"}` twice.
fn fixture_rows() -> InsertStatement {
    let city = || {
        jsonb_build_object([
            text_literal("city"),
            text_literal("city-").concat_text(cast(Expr::expr(Expr::col(Alias::new("g")).modulo(5000)), PostgresType::Text)),
        ])
    };
    Query::insert()
        .into_table(Alias::new(FIXTURE_TABLE))
        .columns(["id", "containment", "existence"].map(Alias::new))
        .select_from(
            Query::select()
                .column(Alias::new("g"))
                .expr(city())
                .expr(city())
                .from_function(generate_series(Expr::val(1), Expr::val(20_000)), Alias::new("g"))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_owned()
}

/// The ids of the fixture rows that satisfy `predicate`.
fn fixture_ids_where(predicate: Expr) -> SelectStatement {
    Query::select()
        .column(Alias::new("id"))
        .from(Alias::new(FIXTURE_TABLE))
        .and_where(predicate)
        .to_owned()
}

/// `containment @> '{"city": "city-42"}'::jsonb`.
fn city_42_contained() -> Expr {
    Expr::col(Alias::new("containment")).array_contains(cast(Expr::val("{\"city\": \"city-42\"}"), PostgresType::Jsonb))
}

#[tokio::test]
async fn object_containment_uses_path_ops_and_key_existence_needs_the_default_class() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    // One transaction, so the temporary relation and its indexes live on a
    // single connection and disappear at rollback.
    let mut transaction = scope.database_pool().begin().await.expect("transaction must start");
    TemporaryTable::new(
        Alias::new(FIXTURE_TABLE),
        vec![
            TemporaryColumn::new(Alias::new("id"), PostgresType::Bigint, ColumnConstraint::PrimaryKey),
            TemporaryColumn::new(Alias::new("containment"), PostgresType::Jsonb, ColumnConstraint::NotNull),
            TemporaryColumn::new(Alias::new("existence"), PostgresType::Jsonb, ColumnConstraint::NotNull),
        ],
    )
    .to_query()
    .execute(&mut *transaction)
    .await
    .expect("fixture table must be created");
    fixture_rows()
        .to_query()
        .execute(&mut *transaction)
        .await
        .expect("fixture rows must be written");
    CreateGinIndex::new(
        Alias::new("object_predicate_path_ops"),
        TableName::from(Alias::new(FIXTURE_TABLE)),
        Alias::new("containment"),
        JsonbOperatorClass::PathOps,
    )
    .to_query()
    .execute(&mut *transaction)
    .await
    .expect("path-ops index must be created");
    CreateGinIndex::new(
        Alias::new("object_predicate_default_ops"),
        TableName::from(Alias::new(FIXTURE_TABLE)),
        Alias::new("existence"),
        JsonbOperatorClass::Default,
    )
    .to_query()
    .execute(&mut *transaction)
    .await
    .expect("default-ops index must be created");
    analyze(Alias::new("pg_temp"), Alias::new(FIXTURE_TABLE))
        .to_query()
        .execute(&mut *transaction)
        .await
        .expect("fixture statistics must be collected");

    let containment = explain(&mut *transaction, fixture_ids_where(city_42_contained())).await;
    assert!(
        containment.contains("object_predicate_path_ops"),
        "a containment predicate must use the jsonb_path_ops index: {containment}"
    );

    let existence = explain(
        &mut *transaction,
        fixture_ids_where(Expr::col(Alias::new("existence")).jsonb_has_key(text_literal("city"))),
    )
    .await;
    assert!(
        !existence.contains("object_predicate_path_ops"),
        "key existence must never be answered by an index class that cannot serve it: {existence}"
    );

    // The residual oracle: both predicates select the same rows the planner
    // claims, whichever access path it chooses.
    let contained: i64 = Query::select()
        .expr(count_all())
        .from(Alias::new(FIXTURE_TABLE))
        .and_where(city_42_contained())
        .to_query_scalar()
        .fetch_one(&mut *transaction)
        .await
        .expect("containment count must be readable");
    assert_eq!(contained, 4, "20000 rows over 5000 distinct cities give four matches");

    transaction.rollback().await.expect("rollback must succeed");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn the_projection_indexes_value_object_for_containment_alone() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");

    let index_text = || index_definition(Expr::col((Alias::new("pg_index"), Alias::new("indexrelid"))));
    let object_indexes: Vec<String> = Query::select()
        .expr(index_text())
        .from(Alias::new("pg_index"))
        .inner_join(
            PgClass::Table,
            Expr::col((PgClass::Table, PgClass::Oid)).equals((Alias::new("pg_index"), Alias::new("indrelid"))),
        )
        .and_where(Expr::col((PgClass::Table, PgClass::Relname)).eq("current_attributes"))
        .and_where(index_text().like("%value_object%"))
        .to_query_scalar()
        .fetch_all(scope.database_pool())
        .await
        .expect("index list must be readable");
    assert_eq!(
        object_indexes.len(),
        1,
        "one index serves the containment arm of a q equality: {object_indexes:?}"
    );
    assert!(
        object_indexes.iter().all(|definition| definition.contains("value_object jsonb_path_ops")),
        "every compiled object predicate is containment, which jsonb_path_ops serves: {object_indexes:?}"
    );

    scope.finish().await.expect("cleanup failed");
}
