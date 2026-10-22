//! Explicit constraint names: no constraint keeps a generated name.
//!
//! Every constraint a migration declares carries an explicit name (`pk_`,
//! `fk_`, `uq_`, `chk_`), because a later migration that alters or drops a
//! constraint has to name it, and a name PostgreSQL generated is a function of
//! the table and column names at the time it was created -- rename either and
//! the generated name silently stops matching what the next migration expects.
//! A constraint left unnamed shows up in the catalogue with one of the
//! generated suffixes, so the catalogue is read back and any such name fails.
//!
//! PostgreSQL 18 records `NOT NULL` as catalogue constraints with generated
//! names of their own; those are declared by the column, never altered by
//! name, and are left out. So are the relations an extension owns (`PostGIS`
//! installs `spatial_ref_sys` into the same schema) and the migration ledgers,
//! whose names start with an underscore and whose shape sqlx dictates.

use crate::SUITE;
use andromeda_postgresql_seaquery::{
    cast::cast,
    catalog_function::left,
    introspection::current_schema,
    literal::{int_literal, text_literal},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    statement::Statement,
    system_catalog::{PgClass, PgNamespace},
};
use andromeda_test_harness::{database_access::connect_test_database, environment::TestEnvironment};
use sea_query::{Alias, Expr, ExprTrait, Order, Query};
use sqlx::PgPool;

/// The suffixes PostgreSQL gives a primary key, unique, foreign key and check
/// constraint it has to name itself.
const GENERATED_SUFFIXES: [&str; 4] = ["_pkey", "_key", "_fkey", "_check"];

/// Every `relation.constraint` in the connection's current schema that belongs
/// to an Andromeda relation and is not a `NOT NULL` constraint.
async fn declared_constraints(pool: &PgPool) -> Vec<String> {
    let constraint = |column: &str| Expr::col((Alias::new("pg_constraint"), Alias::new(column)));
    let dependency = |column: &str| Expr::col((Alias::new("pg_depend"), Alias::new(column)));
    Query::select()
        .expr(
            cast(Expr::col((PgClass::Table, PgClass::Relname)), PostgresType::Text)
                .concat_text(text_literal("."))
                .concat_text(cast(constraint("conname"), PostgresType::Text)),
        )
        .from(Alias::new("pg_constraint"))
        .inner_join(
            PgClass::Table,
            Expr::col((PgClass::Table, PgClass::Oid)).equals((Alias::new("pg_constraint"), Alias::new("conrelid"))),
        )
        .inner_join(
            PgNamespace::Table,
            Expr::col((PgNamespace::Table, PgNamespace::Oid)).equals((PgClass::Table, PgClass::Relnamespace)),
        )
        .and_where(Expr::col((PgNamespace::Table, PgNamespace::Nspname)).eq(current_schema()))
        .and_where(constraint("contype").ne(text_literal("n")))
        .and_where(left(Expr::col((PgClass::Table, PgClass::Relname)), Expr::val(1)).ne("_"))
        .and_where(
            Expr::exists(
                Query::select()
                    .expr(int_literal(1))
                    .from(Alias::new("pg_depend"))
                    .and_where(dependency("classid").eq(cast(text_literal("pg_class"), PostgresType::Regclass)))
                    .and_where(dependency("objid").equals((PgClass::Table, PgClass::Oid)))
                    .and_where(dependency("deptype").eq(text_literal("e")))
                    .to_owned(),
            )
            .not(),
        )
        .order_by_expr(int_literal(1), Order::Asc)
        .to_query_scalar()
        .fetch_all(pool)
        .await
        .expect("the constraint catalogue must be readable")
}

/// The constraints among `declared` whose name PostgreSQL generated.
fn generated_names(declared: &[String]) -> Vec<&str> {
    declared
        .iter()
        .map(String::as_str)
        .filter(|qualified| {
            qualified
                .split_once('.')
                .is_some_and(|(_, constraint)| GENERATED_SUFFIXES.iter().any(|suffix| constraint.ends_with(suffix)))
        })
        .collect()
}

#[tokio::test]
async fn tenant_schema_no_constraint_carries_a_generated_name() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let platform = connect_test_database().await.expect("test database connection");

    for (database, pool) in [("tenant", scope.database_pool()), ("platform", &platform)] {
        let declared = declared_constraints(pool).await;
        assert!(
            !declared.is_empty(),
            "the {database} schema declares constraints, so the scan read the right schema"
        );
        let generated = generated_names(&declared);
        assert!(
            generated.is_empty(),
            "every {database} constraint carries an explicit pk_/fk_/uq_/chk_ name, but these were named by PostgreSQL: {generated:#?}"
        );
    }

    platform.close().await;
    scope.finish().await.expect("cleanup failed");
}
