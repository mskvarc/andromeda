//! The upgrade leaves no `seal_conversion` schema: the tenant set drops it,
//! with every function it holds, once every branch is converted, and the two
//! functions that convert the queues are temporary and dropped by the file
//! that creates them.

use crate::fixtures::{
    rehearsal::{at_v1_0_0, intern_seal_attribute},
    statements::name,
    test_error::UpgradeTestError,
};
use andromeda_postgresql_seaquery::statement::Statement;
use pretty_assertions::assert_eq;
use sea_query::{Expr, ExprTrait, Query};
use sqlx::PgPool;

/// The schema the conversion functions live in.
const CONVERSION_SCHEMA: &str = "seal_conversion";

/// The schemas of the database named like the conversion schema.
async fn conversion_schemas(pool: &PgPool) -> Result<Vec<String>, UpgradeTestError> {
    let select = Query::select()
        .column(name("nspname"))
        .from((name("pg_catalog"), name("pg_namespace")))
        .and_where(Expr::col(name("nspname")).eq(CONVERSION_SCHEMA))
        .to_owned();
    Ok(select.to_query_scalar::<String>().fetch_all(pool).await?)
}

#[tokio::test]
async fn the_upgrade_leaves_no_conversion_schema() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    intern_seal_attribute(database.pool()).await?;

    database.migrate_to_current().await?;

    assert_eq!(conversion_schemas(database.pool()).await?, Vec::<String>::new());
    database.drop_database().await?;
    Ok(())
}
